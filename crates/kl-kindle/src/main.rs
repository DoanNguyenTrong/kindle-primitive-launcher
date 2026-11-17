//! Primitive Launcher on the Kindle. Started by the supervisor (`launcher.sh`) with the stock framework
//! stopped. Draws the app grid on the e-ink panel and reads touch, page buttons and power events. To open
//! an app it writes `next` and exits with code 10; the supervisor runs the app and starts the launcher
//! again. Exit 0 asks the supervisor for the stock Kindle UI.
//!
//! Environment (defaults for the Kindle):
//!   KL_DIR        /mnt/us/launcher    apps.toml, settings.txt, fonts/, cache/, ssh.sh
//!   KL_RUN        /var/tmp/kl         next, heartbeat, orientation, fonts copied at start
//!   KL_DOCUMENTS  /mnt/us/documents   scriptlets

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::Ordering,
        mpsc::{channel, Sender},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use kl_core::{discover, Action, App, Config, DeviceCmd, Input, Launch, Layout, Rotation, Screen, Settings, Status, Ui};
use kl_render::Renderer;

mod device;
mod eink;
mod input;
mod profile;

const W: u32 = profile::ACTIVE.screen_w;
const H: u32 = profile::ACTIVE.screen_h;

/// Exit codes the supervisor acts on.
const EXIT_KINDLE_UI: i32 = 0;
const EXIT_LAUNCH: i32 = 10;

pub enum Ev {
    Input(Input),
    Tick,
    /// The device was turned (auto-rotation, after the debouncer).
    Turn(Rotation),
    /// A background app ended: its name, last output line and whether it succeeded.
    Background { name: String, line: String, ok: bool },
}

pub fn log(msg: &str) {
    eprintln!("[kl] {msg}");
}

fn env_dir(name: &str, default: &str) -> PathBuf {
    PathBuf::from(std::env::var(name).unwrap_or_else(|_| default.to_string()))
}

/// Write a file through a temporary name, so a reader never sees half of it.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// Fonts: the copy in KL_RUN (tmpfs, made by the supervisor) when present, else KL_DIR.
fn font_paths(dir: &Path, run: &Path) -> Vec<String> {
    let names = ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"];
    let base = if run.join("fonts").join(names[0]).exists() { run.join("fonts") } else { dir.join("fonts") };
    names.iter().map(|n| base.join(n).to_string_lossy().into_owned()).collect()
}

/// Shows frames on the panel: a full GC16 update on request, otherwise only the changed rows with a
/// non-flashing waveform (DU for pure black and white, AUTO when icons bring gray).
struct Display {
    panel: Option<eink::Panel>,
    last: Vec<u8>,
}

impl Display {
    fn open() -> Display {
        let panel = match eink::Panel::open() {
            Ok(p) => Some(p),
            Err(e) => {
                log(&format!("panel: {e}"));
                None
            }
        };
        Display { panel, last: Vec::new() }
    }

    fn fb_turn(&self) -> u32 {
        self.panel.as_ref().map_or(0, |p| p.turn())
    }

    fn show(&mut self, upright: Vec<u8>, settings: &Settings, full: bool) {
        let fb_turn = self.fb_turn();
        let frame = kl_render::for_panel(upright, settings.rotation, fb_turn);
        let Some(panel) = self.panel.as_mut() else {
            self.last = frame;
            return;
        };
        let t = Instant::now();
        let result = if full || self.last.len() != frame.len() {
            panel.draw(W, 0, H, &frame, eink::WAVE_GC16, true).map(|ok| (ok, "GC16 full", H))
        } else if let Some((y, rows)) = kl_render::changed_rows(&self.last, &frame, W as usize) {
            let band = &frame[y * W as usize..(y + rows) * W as usize];
            let (wave, name) = if kl_render::is_black_and_white(band) { (eink::WAVE_DU, "DU") } else { (eink::WAVE_AUTO, "AUTO") };
            panel.draw(W, y as u32, rows as u32, band, wave, false).map(|ok| (ok, name, rows as u32))
        } else {
            Ok((true, "unchanged", 0))
        };
        match result {
            Ok((true, wave, rows)) => log(&format!("draw {wave} {rows} rows in {} ms (fb turn {fb_turn})", t.elapsed().as_millis())),
            Ok((false, ..)) => log("draw: the framebuffer is not in a drawable state"),
            Err(e) => log(&format!("draw: {e}")),
        }
        self.last = frame;
    }
}

extern "C" fn on_fatal(sig: libc::c_int, info: *mut libc::siginfo_t, _ctx: *mut libc::c_void) {
    // Only write(2) here: safe in a signal handler. Copied from Primitive Browser.
    fn put(bytes: &[u8]) {
        unsafe { libc::write(2, bytes.as_ptr() as *const libc::c_void, bytes.len()) };
    }
    let addr = if info.is_null() { 0 } else { unsafe { (*info).si_addr() as usize } };
    let mut buf = [0u8; 18];
    let mut at = buf.len();
    let mut v = addr;
    loop {
        at -= 1;
        buf[at] = b"0123456789abcdef"[v & 15];
        v >>= 4;
        if v == 0 {
            break;
        }
    }
    put(b"\n[crash] fatal signal ");
    put(&[b'0' + (sig as u8 / 10), b'0' + (sig as u8 % 10)]);
    put(b" at address 0x");
    put(&buf[at..]);
    put(b"\n");
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

fn install_crash_log() {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_fatal as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGBUS, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGSEGV, &action, std::ptr::null_mut());
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn main() {
    install_crash_log();
    let started = Instant::now();
    let dir = env_dir("KL_DIR", "/mnt/us/launcher");
    let run = env_dir("KL_RUN", "/var/tmp/kl");
    let documents = env_dir("KL_DOCUMENTS", "/mnt/us/documents");
    let _ = fs::create_dir_all(&run);
    log(&format!("start v{} dir {} run {}", kl_core::VERSION, dir.display(), run.display()));

    let settings_path = dir.join("settings.txt");
    let mut settings = Settings::parse(&fs::read_to_string(&settings_path).unwrap_or_default());
    if settings.sleep_screen_launcher {
        device::unload_stock_screensaver();
    }
    // With automatic rotation the saved turn can be stale (it is only saved on manual changes): read the
    // device's turn now, so the first screen is drawn the right way up.
    if settings.auto_rotation {
        if let Some(turn) = device::accelerometer_turn() {
            settings.rotation = turn;
        }
    }
    input::TURN.store(settings.rotation.turn(), Ordering::Relaxed);
    let _ = write_atomic(&run.join("orientation"), settings.rotation.letter());
    if std::env::args().any(|a| a == "--wait-screen") {
        wait_screen(&dir, &run, &settings);
        return;
    }

    let (config, mut problems) = match fs::read_to_string(dir.join("apps.toml")) {
        Ok(text) => Config::parse(&text),
        Err(_) => (Config::default(), Vec::new()),
    };
    let (apps, more) = discover(&documents, &config);
    problems.extend(more);
    let extensions_root = documents.parent().unwrap_or(Path::new("/mnt/us")).join("extensions");
    let (extensions, more) = kl_core::kual::discover_root(&extensions_root);
    problems.extend(more);
    let layout = Layout::new(W, H, config.columns, config.rows);

    let mut renderer = match Renderer::new(W, H, &font_paths(&dir, &run)) {
        Ok(r) => r,
        Err(e) => {
            log(&format!("renderer: {e}"));
            std::process::exit(2);
        }
    };
    problems.extend(kl_render::load_icons(&mut renderer, &apps, &layout, Some(&dir.join("cache"))));
    for p in &problems {
        log(&format!("problem: {p}"));
    }
    log(&format!("{} apps; ready in {} ms", apps.len(), started.elapsed().as_millis()));

    let mut ui = Ui::new(apps, layout);
    ui.set_icon_choices(png_files(&dir.join("icons")));
    ui.set_extensions(extensions);
    for app in ui.apps() {
        log(&format!("app {}: {}", app.id, app.name));
    }
    ui.notice = problems.first().cloned();
    let mut display = Display::open();
    let mut status = device::status(&settings);
    ui.set_battery(status.battery, status.charging);

    let (tx, rx) = channel();
    match input::find(profile::ACTIVE.touch_name) {
        Some(path) => {
            let t = tx.clone();
            thread::spawn(move || input::touch_loop(path, t));
        }
        None => log("no touch device found"),
    }
    match input::find("gpio-keys") {
        Some(path) => {
            let t = tx.clone();
            thread::spawn(move || input::button_loop(path, t));
        }
        None => log("no page-button device found"),
    }
    {
        let t = tx.clone();
        thread::spawn(move || device::power_loop(t));
    }
    {
        let t = tx.clone();
        let start = settings.rotation;
        thread::spawn(move || device::orientation_loop(t, start));
    }
    {
        let t = tx.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            if t.send(Ev::Tick).is_err() {
                return;
            }
        });
    }

    display.show(renderer.draw(&ui, &status), &settings, true);
    log(&format!("first screen after {} ms", started.elapsed().as_millis()));
    let mut minute = device::local_time().2;
    let mut sleep_picture = 0usize;

    while let Ok(ev) = rx.recv() {
        let action = match ev {
            Ev::Background { name, line, ok } => {
                log(&format!("background end {name}: ok={ok} {line}"));
                let state = if ok { "done" } else { "failed" };
                ui.notice = Some(if line.is_empty() { format!("{name}: {state}") } else { format!("{name}: {line}") });
                if ui.screen == Screen::Sleep {
                    Action::None
                } else {
                    Action::Redraw { full: false }
                }
            }
            Ev::Turn(rotation) => {
                if settings.auto_rotation && rotation != settings.rotation {
                    settings.rotation = rotation;
                    input::TURN.store(rotation.turn(), Ordering::Relaxed);
                    let _ = write_atomic(&run.join("orientation"), rotation.letter());
                    log(&format!("screen turned: {}", rotation.letter()));
                    status = device::status(&settings);
                    Action::Redraw { full: true }
                } else {
                    Action::None
                }
            }
            Ev::Tick => {
                let _ = write_atomic(&run.join("heartbeat"), &now_secs().to_string());
                let m = device::local_time().2;
                if m != minute && ui.screen == Screen::Home {
                    minute = m;
                    status = device::status(&settings);
                    ui.set_battery(status.battery, status.charging);
                    Action::Redraw { full: false }
                } else {
                    Action::None
                }
            }
            Ev::Input(input) => {
                if !matches!(input, Input::Sleep | Input::Wake) {
                    log(&format!("input {input:?}"));
                }
                // The launcher's own sleep screen: the next picture from the screensavers folder, if any.
                if input == Input::Sleep && settings.sleep_screen_launcher && ui.screen != Screen::Sleep {
                    let pictures = png_files(&dir.join("screensavers"));
                    let picture = if pictures.is_empty() {
                        None
                    } else {
                        let path = &pictures[sleep_picture % pictures.len()];
                        sleep_picture += 1;
                        match kl_render::sleep_image(Path::new(path), W, H) {
                            Ok(picture) => Some(picture),
                            Err(e) => {
                                log(&format!("sleep picture: {e}"));
                                None
                            }
                        }
                    };
                    renderer.set_sleep_image(picture);
                }
                ui.handle(input)
            }
        };
        match action {
            Action::None => {}
            Action::Redraw { full } => {
                if full {
                    status = device::status(&settings);
                }
                display.show(renderer.draw(&ui, &status), &settings, full);
            }
            Action::Launch(app) => {
                display.show(renderer.draw(&ui, &status), &settings, false);
                let Some(next) = app.next_file() else {
                    ui.pressed = None;
                    ui.notice = Some(format!("{}: cannot be started", app.name));
                    display.show(renderer.draw(&ui, &status), &settings, false);
                    continue;
                };
                if let Err(e) = write_atomic(&run.join("next"), &next) {
                    ui.pressed = None;
                    ui.notice = Some(format!("cannot write {}: {e}", run.join("next").display()));
                    display.show(renderer.draw(&ui, &status), &settings, false);
                    continue;
                }
                log(&format!("launch {} ({})", app.name, app.id));
                std::process::exit(EXIT_LAUNCH);
            }
            Action::SetHidden(changes) => {
                save_apps_toml(&dir, &mut ui, "hidden apps", |c| {
                    for (id, hidden) in &changes {
                        c.set_hidden(id, *hidden);
                    }
                });
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::RunBackground(app) => {
                run_background(app, tx.clone());
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::SetOrder(order) => {
                save_apps_toml(&dir, &mut ui, "order", |c| {
                    for (id, place) in &order {
                        c.set_order(id, *place);
                    }
                });
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::SetIcon { id, icon } => {
                save_apps_toml(&dir, &mut ui, "icon", |c| c.set_icon(&id, icon.clone()));
                renderer.forget_icon(&id);
                if let Some(app) = ui.apps().iter().find(|a| a.id == id).cloned() {
                    let layout = ui.layout;
                    for problem in kl_render::load_icons(&mut renderer, &[app], &layout, Some(&dir.join("cache"))) {
                        log(&format!("icon: {problem}"));
                        ui.notice = Some(problem);
                    }
                }
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::KindleMode => {
                log("leaving for the Kindle UI");
                std::process::exit(EXIT_KINDLE_UI);
            }
            Action::Device(cmd) => {
                run_device(cmd, &mut status, &mut settings, &dir, &run, &settings_path);
                let full = matches!(cmd, DeviceCmd::ToggleRotation | DeviceCmd::AutoRotation);
                display.show(renderer.draw(&ui, &status), &settings, full);
            }
        }
    }
}

/// `kl-launcher --wait-screen`: run by the supervisor while the stock Kindle UI starts (about a minute) and
/// stopped by it once the library is ready. One full update, then one block of the bar every 1.5 s with a small
/// partial update.
fn wait_screen(dir: &Path, run: &Path, settings: &Settings) {
    const TITLE: &str = "Opening the Kindle UI";
    const BODY: &str = "This takes about a minute.";
    let mut renderer = match Renderer::new(W, H, &font_paths(dir, run)) {
        Ok(r) => r,
        Err(e) => return log(&format!("wait screen: {e}")),
    };
    log("wait screen");
    let mut display = Display::open();
    let mut step = 0;
    display.show(renderer.waiting(TITLE, BODY, step), settings, true);
    loop {
        thread::sleep(Duration::from_millis(1500));
        step += 1;
        display.show(renderer.waiting(TITLE, BODY, step), settings, false);
    }
}

/// Run an app without leaving the launcher (apps.toml `mode = "background"`, KUAL `exitmenu: false`), with the
/// same environment the supervisor gives apps. Its output is collected and the last line sent back when it ends.
fn run_background(app: App, tx: Sender<Ev>) {
    let args: Vec<String> = match &app.launch {
        Launch::Script { path, hooks: false } => vec![path.clone()],
        Launch::Script { path, hooks: true } => vec!["-c".into(), ". \"$1\"; on_run".into(), "sh".into(), path.clone()],
        Launch::Shell { command } => vec!["-c".into(), command.clone()],
        _ => return,
    };
    log(&format!("background start {} ({})", app.name, app.id));
    thread::spawn(move || {
        let result = Command::new("/bin/sh")
            .args(&args)
            .current_dir(&app.dir)
            .env("PRIMITIVE_LAUNCHER", "1")
            .env("PRIMITIVE_FRAMEWORK", "stopped")
            .stdin(Stdio::null())
            .output();
        let (ok, line) = match result {
            Ok(out) => {
                let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                (out.status.success(), last)
            }
            Err(e) => (false, e.to_string()),
        };
        let _ = tx.send(Ev::Background { name: app.name, line, ok });
    });
}

/// Apply `change` to apps.toml and write it back. A file that is not valid TOML is never overwritten.
fn save_apps_toml(dir: &Path, ui: &mut Ui, what: &str, change: impl FnOnce(&mut Config)) {
    let path = dir.join("apps.toml");
    let text = fs::read_to_string(&path).unwrap_or_default();
    match Config::load(&text) {
        Ok(mut config) => {
            change(&mut config);
            match write_atomic(&path, &config.to_toml()) {
                Ok(()) => log(&format!("apps.toml: {what} saved")),
                Err(e) => ui.notice = Some(format!("cannot save apps.toml: {e}")),
            }
        }
        Err(e) => {
            log(&format!("apps.toml is not valid TOML; not saved: {e}"));
            ui.notice = Some("apps.toml has an error, so the change was not saved".into());
        }
    }
}

/// The PNG files in a folder, sorted: icons for the app menu, pictures for the sleep screen.
fn png_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")))
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("._")))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    files.sort();
    files
}

fn run_device(cmd: DeviceCmd, status: &mut Status, settings: &mut Settings, dir: &Path, run: &Path, settings_path: &Path) {
    match cmd {
        DeviceCmd::Light(step) => device::set_light((status.light as i32 + step * 2).clamp(0, device::LEVEL_MAX as i32) as u32),
        DeviceCmd::Warmth(step) => device::set_warmth((status.warmth as i32 + step * 2).clamp(0, device::LEVEL_MAX as i32) as u32),
        DeviceCmd::ToggleWifi => device::set_wifi(!status.wifi_on),
        DeviceCmd::ToggleSsh => device::set_ssh(&dir.to_string_lossy(), !status.ssh),
        DeviceCmd::ToggleAutostart => device::set_autostart(&dir.to_string_lossy(), !status.autostart),
        DeviceCmd::ToggleSleepScreen => {
            settings.sleep_screen_launcher = !settings.sleep_screen_launcher;
            status.sleep_launcher = settings.sleep_screen_launcher;
            if let Err(e) = write_atomic(settings_path, &settings.to_text()) {
                log(&format!("settings: {e}"));
            }
            if settings.sleep_screen_launcher {
                device::unload_stock_screensaver();
            } else {
                log("sleep screen: the Kindle's; it returns after Kindle mode or a reboot");
            }
        }
        DeviceCmd::AutoRotation => {
            settings.auto_rotation = true;
            if let Some(turn) = device::accelerometer_turn() {
                settings.rotation = turn;
                input::TURN.store(turn.turn(), Ordering::Relaxed);
                let _ = write_atomic(&run.join("orientation"), turn.letter());
            }
            if let Err(e) = write_atomic(settings_path, &settings.to_text()) {
                log(&format!("settings: {e}"));
            }
        }
        DeviceCmd::ToggleRotation => {
            settings.auto_rotation = false;
            settings.rotation = settings.rotation.flipped();
            input::TURN.store(settings.rotation.turn(), Ordering::Relaxed);
            if let Err(e) = write_atomic(settings_path, &settings.to_text()) {
                log(&format!("settings: {e}"));
            }
            let _ = write_atomic(&run.join("orientation"), settings.rotation.letter());
        }
    }
    *status = device::status(settings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kl_core::IconSource;

    fn shell(name: &str, command: &str) -> App {
        App {
            id: name.into(),
            name: name.into(),
            icon: IconSource::None,
            launch: Launch::Shell { command: command.into() },
            dir: "/".into(),
            order: None,
            hidden: false,
            background: true,
        }
    }

    fn finished(rx: &std::sync::mpsc::Receiver<Ev>) -> (String, String, bool) {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ev::Background { name, line, ok }) => (name, line, ok),
            _ => panic!("no background result"),
        }
    }

    #[test]
    fn a_background_app_reports_its_last_output_line() {
        let (tx, rx) = channel();
        run_background(shell("Echo", "echo one; echo two; echo; echo \"$PRIMITIVE_LAUNCHER\""), tx);
        assert_eq!(finished(&rx), ("Echo".into(), "1".into(), true));
    }

    #[test]
    fn a_failing_background_app_is_reported_as_failed() {
        let (tx, rx) = channel();
        run_background(shell("Fail", "echo trouble >&2; exit 3"), tx);
        assert_eq!(finished(&rx), ("Fail".into(), "trouble".into(), false));
    }
}
