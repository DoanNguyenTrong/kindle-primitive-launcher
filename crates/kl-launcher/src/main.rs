//! Primitive Launcher on the Kindle. Started by the supervisor (`launcher.sh`) with the stock framework
//! stopped. Draws the app grid on the e-ink panel and reads touch, page buttons and power events. To open
//! an app it writes `next` and exits with code 10; the supervisor runs the app and starts the launcher
//! again. Exit 0 asks the supervisor for the stock Kindle UI.
//!
//! Environment (defaults for the Kindle):
//!   KL_DIR        /mnt/us/launcher    apps.toml, settings.txt, fonts/, cache/, ssh.sh
//!   KL_RUN        /var/tmp/kl         next, heartbeat, orientation, fonts copied at start
//!   KL_DOCUMENTS  /mnt/us/documents   scriptlets

mod bootstrap;
mod transfer;

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

use kl_core::store_open;
use kl_core::{discover, Action, App, Config, DeviceCmd, Input, Launch, Layout, Screen, Settings, Status, Ui};
use kl_render::Renderer;

use kl_kindle::{device, display::Display, input, log, profile, Ev};

const W: u32 = profile::ACTIVE.screen_w;
const H: u32 = profile::ACTIVE.screen_h;

/// Exit codes the supervisor acts on.
const EXIT_KINDLE_UI: i32 = 0;
const EXIT_LAUNCH: i32 = 10;


/// Join a network on a thread, then send the result and a fresh scan. The password is passed on, never logged.
fn start_wifi_join(tx: Sender<Ev>, name: String, secured: bool, password: Option<String>, saved: bool) {
    thread::spawn(move || {
        log(&format!("join {name} (saved: {saved}, secured: {secured})"));
        let result = device::join_network(&name, secured, password.as_deref(), saved);
        let (ok, message) = match result {
            Ok(()) => (true, format!("Connected to {name}")),
            Err(e) => (false, e),
        };
        log(&format!("join {name}: {}", if ok { "connected" } else { "failed" }));
        let _ = tx.send(Ev::WifiResult { ok, message });
        let _ = tx.send(Ev::Wifi(device::wifi_info()));
    });
}

/// Run an SSH job on a thread (with the launcher's folder), then send its message to the event loop as a notice.
fn start_ssh_job(tx: Sender<Ev>, job: impl FnOnce(&str) -> String + Send + 'static) {
    thread::spawn(move || {
        let message = job(&device::launcher_dir());
        let _ = tx.send(Ev::Notice(message));
    });
}

/// Forget a saved network on a thread, then send the result and a fresh scan.
fn start_wifi_forget(tx: Sender<Ev>, name: String) {
    thread::spawn(move || {
        let (ok, message) = match device::forget_network(&name) {
            Ok(()) => (true, format!("Forgot {name}")),
            Err(e) => (false, e),
        };
        log(&format!("forget {name}: {ok}"));
        let _ = tx.send(Ev::WifiResult { ok, message });
        let _ = tx.send(Ev::Wifi(device::wifi_info()));
    });
}

/// Scan for Wi-Fi on a thread and send the result to the event loop.
fn start_wifi_scan(tx: Sender<Ev>) {
    start_wifi_scan_after(tx, 0);
}

/// Scan after `secs` seconds (Wi-Fi just turned on needs a moment to come up).
fn start_wifi_scan_after(tx: Sender<Ev>, secs: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(secs));
        let info = device::wifi_info();
        let _ = tx.send(Ev::Wifi(info));
    });
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

/// The config, discovered apps and extensions, and any problems found reading them.
fn load_library(dir: &Path, documents: &Path) -> (Config, Vec<App>, Vec<kl_core::kual::MenuItem>, Vec<String>) {
    let (config, mut problems) = match fs::read_to_string(dir.join("apps.toml")) {
        Ok(text) => Config::parse(&text),
        Err(_) => (Config::default(), Vec::new()),
    };
    let (mut apps, more) = discover(documents, &config);
    problems.extend(more);
    bootstrap::reconcile_tiles(&mut apps, documents);
    kl_core::apps::use_koreader_wrapper(&mut apps, &config, &dir.join("run-koreader.sh"));
    let extensions_root = documents.parent().unwrap_or(Path::new("/mnt/us")).join("extensions");
    let (extensions, more) = kl_core::kual::discover_root(&extensions_root, kl_core::kual::OASIS_3_MODEL);
    problems.extend(more);
    (config, apps, extensions, problems)
}

fn font_paths(dir: &Path, run: &Path) -> Vec<String> {
    let names = ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"];
    let base = if run.join("fonts").join(names[0]).exists() { run.join("fonts") } else { dir.join("fonts") };
    names.iter().map(|n| base.join(n).to_string_lossy().into_owned()).collect()
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

fn launcher_update_command(dir: &Path, command: &str) -> i32 {
    use kl_store::install::{self, LauncherUpdate, Paths};

    let Some(root) = dir.parent() else {
        eprintln!("Launcher update needs a mounted user-storage directory.");
        return 1;
    };
    let paths = Paths::under(root);
    let result = match command {
        "begin" => install::launcher_update_begin(&paths).map(|outcome| match outcome {
            LauncherUpdate::Idle => "idle",
            LauncherUpdate::Trial => "trial",
            LauncherUpdate::Reverted => "reverted",
        }),
        "accept" => install::launcher_update_accept(&paths).map(|changed| if changed { "accepted" } else { "idle" }),
        "revert" => install::launcher_update_revert(&paths).map(|changed| if changed { "reverted" } else { "idle" }),
        _ => {
            eprintln!("Expected launcher update command begin, accept or revert.");
            return 2;
        }
    };
    match result {
        Ok(outcome) => {
            println!("{outcome}");
            0
        }
        Err(error) => {
            eprintln!("Launcher update: {error}");
            1
        }
    }
}

fn store_launch_descriptor(dir: &Path, documents: &Path, apps: &[App], id: &str) -> std::io::Result<String> {
    let filename = store_open::installed_entry(dir, documents, id)?;
    let entry = documents.join(&filename);
    apps.iter().find(|app| {
        app.id == filename && !app.background
            && matches!(&app.launch, Launch::Script { path, .. } if Path::new(path) == entry)
    })
    .and_then(App::next_file)
    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Open this app from the launcher grid."))
}

fn main() {
    let mut dir = env_dir("KL_DIR", "/mnt/us/launcher");
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--launcher-update") {
        std::process::exit(launcher_update_command(&dir, args.next().as_deref().unwrap_or("")));
    }
    install_crash_log();
    let started = Instant::now();
    // Started from the copy on /var/local (the cable was in at boot): when /mnt/us is back, this same launcher reads its
    // settings, apps and extensions from /mnt/us.
    let mut from_mirror = dir.starts_with("/var/local");
    let run = env_dir("KL_RUN", "/var/tmp/kl");
    let documents = env_dir("KL_DOCUMENTS", "/mnt/us/documents");
    let _ = fs::create_dir_all(&run);
    log(&format!("start v{} dir {} run {}", env!("CARGO_PKG_VERSION"), dir.display(), run.display()));

    let mut settings_path = dir.join("settings.txt");
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

    // Recover before discovery so the grid reflects an interrupted install.
    if let Some(root) = documents.parent().filter(|root| root.is_dir() && !from_mirror) {
        match kl_store::install::recover(&kl_store::install::Paths::under(root)) {
            Ok(Some(what)) => log(&format!("store: {what}")),
            Ok(None) => {}
            Err(e) => log(&format!("store: cannot check an unfinished install: {e}")),
        }
    }
    let (config, apps, extensions, mut problems) = load_library(&dir, &documents);
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

    ui.version = env!("CARGO_PKG_VERSION").to_string();
    ui.set_icon_choices(png_files(&dir.join("icons")));
    ui.set_extensions(extensions);
    ui.store_update_count = bootstrap::available_updates(&dir);
    ui.store_daily_check = settings.store_daily_check;
    for app in ui.apps() {
        log(&format!("app {}: {}", app.id, app.name));
    }
    ui.notice = problems.first().cloned();
    let mut display = Display::open();
    let mut status = device::status(&settings);
    ui.set_battery(status.battery, status.charging);

    let update_trial = std::env::var("KL_LAUNCHER_UPDATE_TRIAL").as_deref() == Ok("1");
    match store_open::take_request(&run) {
        Ok(Some(id)) if !from_mirror && status.usb != kl_core::Usb::Drive && !update_trial => {
            match store_launch_descriptor(&dir, &documents, ui.apps(), &id)
                .and_then(|next| write_atomic(&run.join("next"), &next))
            {
                Ok(()) => {
                    log(&format!("opening {id} from the Store"));
                    std::process::exit(EXIT_LAUNCH);
                }
                Err(error) => ui.notice = Some(format!("Could not open the app from the Store. {error}")),
            }
        }
        Ok(Some(_)) if update_trial => ui.notice = Some("You can open this app from the grid after the launcher update.".into()),
        Ok(Some(_)) => ui.notice = Some("Eject the Kindle drive before opening the app.".into()),
        Ok(None) => {}
        Err(error) => ui.notice = Some(format!("Could not read the Store's request. {error}")),
    }

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
        // The USB state, each second: an event is sent only when it changes.
        let t = tx.clone();
        thread::spawn(move || {
            let mut last = None;
            loop {
                let now = device::usb_state();
                if last != Some(now) {
                    last = Some(now);
                    if t.send(Ev::Usb(now)).is_err() {
                        return;
                    }
                    // Drive mode: the stock system draws its own USB screen soon after; draw ours over it again.
                    if now == kl_core::Usb::Drive {
                        for secs in [2, 3] {
                            thread::sleep(Duration::from_secs(secs));
                            if t.send(Ev::UsbRedraw).is_err() {
                                return;
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_secs(1));
            }
        });
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
    let mut sleep_picture = 0usize;
    let mut bootstrapping_store = false;
    let mut mark_trial_ready = update_trial;
    let mut transfer_session = None;

    loop {
        if mark_trial_ready && display.has_presented_frame() {
            let path = run.join("launcher-update-ready");
            let pid = std::process::id().to_string();
            if write_atomic(&path, &pid).is_ok() {
                mark_trial_ready = false;
            }
        }
        let Ok(ev) = rx.recv() else { break };
        let previous_screen = ui.screen;
        let previous_usb = ui.usb;
        let action = match ev {
            Ev::Wifi(info) => {
                ui.set_wifi(info);
                // The Wi-Fi state (connected or not) is read again, so the rows show what the scan shows.
                status = device::status(&settings);
                Action::Redraw { full: false }
            }
            Ev::WifiResult { ok, message } => {
                ui.set_join_result(ok, message);
                status = device::status(&settings);
                Action::Redraw { full: false }
            }
            Ev::StoreBootstrapped(result) => {
                bootstrapping_store = false;
                match result {
                    Err(error) => {
                        log(&format!("Store bootstrap failed: {error}"));
                        ui.notice = Some(format!("Could not install Primitive Store. {error}"));
                        Action::Redraw { full: false }
                    }
                    Ok(()) => {
                        let (_, apps, extensions, _) = load_library(&dir, &documents);
                        let app = apps.iter().find(|app| app.id == bootstrap::ENTRY && !app.hidden && !app.background).cloned();
                        ui.set_apps(apps);
                        ui.set_extensions(extensions);
                        ui.store_update_count = bootstrap::available_updates(&dir);
                        for problem in kl_render::load_icons(&mut renderer, ui.apps(), &layout, Some(&dir.join("cache"))) {
                            log(&format!("problem: {problem}"));
                        }
                        ui.notice = Some("Primitive Store is ready.".into());
                        match app {
                            Some(app) if ui.screen == Screen::Home && ui.usb != kl_core::Usb::Drive => Action::Launch(app),
                            _ => Action::Redraw { full: false },
                        }
                    }
                }
            }
            Ev::StoreInstalled { .. } | Ev::StoreRefreshed { .. } | Ev::StoreProgress { .. } => Action::None,
            Ev::Notice(message) => {
                ui.notice = Some(message);
                status = device::status(&settings);
                Action::Redraw { full: false }
            }
            Ev::Background { name, line, ok } => {
                log(&format!("background end {name}: ok={ok} {line}"));
                ui.background_done();
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
                // Screens with the status bar re-read battery, charging, Wi-Fi and the clock every tick (5 s) and
                // redraw only when the bar would show something new.
                let shown = matches!(ui.screen, Screen::Home | Screen::QuickSettings | Screen::AppMenu | Screen::ConfirmKindle);
                if shown {
                    let fresh = device::status(&settings);
                    // Quick settings also shows CPU, memory and storage: redraw it when those change.
                    let changed = status.bar_differs(&fresh) || (ui.screen == Screen::QuickSettings && status.system != fresh.system);
                    let warned = ui.set_battery(fresh.battery, fresh.charging);
                    status = fresh;
                    if changed || warned {
                        Action::Redraw { full: false }
                    } else {
                        Action::None
                    }
                } else if matches!(ui.screen, Screen::Settings | Screen::Page(_) | Screen::NetworkDetail | Screen::Password | Screen::NetworkName | Screen::SshPassword) {
                    // The Settings screens show Wi-Fi, SSH and the start-up switch: redraw when those change.
                    let fresh = device::status(&settings);
                    let changed = status.settings_differs(&fresh);
                    status = fresh;
                    if changed {
                        Action::Redraw { full: false }
                    } else {
                        Action::None
                    }
                } else {
                    Action::None
                }
            }
            Ev::UsbRedraw => {
                if ui.usb == kl_core::Usb::Drive {
                    Action::Redraw { full: true }
                } else {
                    Action::None
                }
            }
            Ev::Usb(usb) => {
                if usb == ui.usb {
                    Action::None
                } else {
                    log(&format!("usb: {usb:?}"));
                    if ui.usb == kl_core::Usb::Drive && from_mirror && Path::new("/mnt/us/launcher/apps.toml").exists() {
                        // Ejected: /mnt/us is back. This launcher reads its settings, apps and extensions from it again.
                        dir = PathBuf::from("/mnt/us/launcher");
                        settings_path = dir.join("settings.txt");
                        from_mirror = false;
                        settings = Settings::parse(&fs::read_to_string(&settings_path).unwrap_or_default());
                        ui.store_daily_check = settings.store_daily_check;
                        let (_config, apps, extensions, problems) = load_library(&dir, &documents);
                        ui.set_apps(apps);
                        ui.set_extensions(extensions);
                        ui.store_update_count = bootstrap::available_updates(&dir);
                        ui.notice = problems.first().cloned();
                        for problem in kl_render::load_icons(&mut renderer, ui.apps(), &layout, Some(&dir.join("cache"))) {
                            log(&format!("problem: {problem}"));
                        }
                        log(&format!("the user storage is back: {} apps read from {}", ui.apps().len(), documents.display()));
                    }
                    ui.usb = usb;
                    status.usb = usb;
                    Action::Redraw { full: true }
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
                // A key of the keyboard is drawn as pressed before its action runs: the tap is seen at once.
                if ui.press_feedback(input) {
                    display.show(renderer.draw(&ui, &status), &settings, false);
                }
                let before = ui.screen;
                let action = ui.handle(input);
                // A Settings page opens with a fresh status: the About text shows the current address and storage.
                if matches!(ui.screen, Screen::Page(_)) && ui.screen != before {
                    status = device::status(&settings);
                }
                // The Wi-Fi page scans when it opens.
                if ui.screen == Screen::Page(kl_core::layout::Page::Wifi) && ui.screen != before {
                    ui.wifi_scanning = true;
                    start_wifi_scan(tx.clone());
                }
                action
            }
        };
        let transfer_screen = Screen::Page(kl_core::layout::Page::FileTransfer);
        if ui.screen != transfer_screen || ui.usb == kl_core::Usb::Drive {
            transfer_session = None;
            ui.file_transfer = None;
        } else if previous_screen != ui.screen || previous_usb == kl_core::Usb::Drive {
            match device::wlan_ip().parse() {
                Ok(ip) => match transfer::Session::start(ip, documents.clone()) {
                    Ok(session) => {
                        ui.file_transfer = Some((session.address.clone(), session.code.clone()));
                        transfer_session = Some(session);
                    }
                    Err(error) => ui.notice = Some(format!("File transfer could not start. {error}")),
                },
                Err(_) => ui.notice = Some("Connect to Wi-Fi, then reopen File transfer.".into()),
            }
        }
        // Keep the receiver in scope for as long as the page is open.
        let _session = transfer_session.as_ref();
        match action {
            Action::None => {}
            // Going to sleep draws nothing unless there is something to show: a battery warning, or a picture
            // for the launcher's own sleep screen. Otherwise the Kindle's screensaver (or the last screen) stays.
            Action::Redraw { .. }
                if ui.screen == Screen::Sleep
                    && ui.battery_message().is_none()
                    && !(settings.sleep_screen_launcher && renderer.has_sleep_image()) => {}
            Action::Redraw { full } => {
                if full {
                    status = device::status(&settings);
                }
                display.show(renderer.draw(&ui, &status), &settings, full);
            }
            Action::Launch(mut app) => {
                if bootstrapping_store {
                    ui.pressed = None;
                    ui.notice = Some("Primitive Store is being installed. Please wait.".into());
                    display.show(renderer.draw(&ui, &status), &settings, false);
                    continue;
                }
                if app.launch == Launch::Store {
                    status = device::status(&settings);
                    if let Some(entry) = bootstrap::entry_for(app, &documents) {
                        app = entry;
                    } else {
                        ui.pressed = None;
                        ui.notice = Some(if from_mirror || status.usb == kl_core::Usb::Drive {
                            "Eject the Kindle drive to install Primitive Store.".into()
                        } else if !status.wifi_on || status.wifi_state != "connected" {
                            "Connect to Wi-Fi to install Primitive Store.".into()
                        } else {
                            bootstrapping_store = true;
                            bootstrap::start(tx.clone(), documents.clone());
                            "Preparing Primitive Store…".into()
                        });
                        display.show(renderer.draw(&ui, &status), &settings, false);
                        continue;
                    }
                }
                let pressed = renderer.draw(&ui, &status);
                display.show(pressed.clone(), &settings, false);
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
                // The opening zoom (Settings > Opening animation): a card grows from the icon to the screen in
                // three fast steps.
                if let Some(icon) = ui.pressed.filter(|_| settings.animation) {
                    for step in kl_core::layout::zoom_steps(icon, ui.layout.screen(), 3) {
                        let mut frame = pressed.clone();
                        let radius = (step.w.min(step.h) as f32 * 0.12).min(48.0);
                        let area = (step.x, step.y, step.w, step.h);
                        kl_render::image::card(&mut frame, W as usize, area, radius, 4.0, settings.dark_mode);
                        display.show(frame, &settings, false);
                        thread::sleep(Duration::from_millis(70));
                    }
                }
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
            Action::ScanWifi => {
                start_wifi_scan(tx.clone());
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::JoinWifi { name, secured, password, saved } => {
                start_wifi_join(tx.clone(), name, secured, password, saved);
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::ForgetWifi(name) => {
                start_wifi_forget(tx.clone(), name);
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::SetSshPassword(password) => {
                start_ssh_job(tx.clone(), move |dir| match device::set_ssh_password(dir, &password) {
                    Ok(()) => "SSH password set".to_string(),
                    Err(e) => format!("SSH password: {e}"),
                });
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::CheckSsh => {
                start_ssh_job(tx.clone(), |dir| format!("SSH: {}", device::check_ssh(dir)));
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
                // The dialog turns into "Opening the Kindle UI"; the wait screen fills its blocks from here.
                let (frame, area) = renderer.waiting_dialog(&ui.layout, 0, settings.dark_mode);
                display.show_region(frame, area, &settings);
                std::process::exit(EXIT_KINDLE_UI);
            }
            Action::Device(cmd) => {
                if matches!(cmd, DeviceCmd::StoreToggleDailyCheck | DeviceCmd::StoreClearCache) {
                    ui.notice = if from_mirror || device::usb_state() == kl_core::Usb::Drive {
                        Some("Eject the Kindle drive before changing Store settings.".into())
                    } else if bootstrapping_store {
                        Some("Wait for the Store to finish installing.".into())
                    } else {
                        match apply_store_setting(cmd, &dir, &settings_path, &mut settings) {
                            Ok(()) => {
                                ui.store_daily_check = settings.store_daily_check;
                                if cmd == DeviceCmd::StoreClearCache {
                                    ui.store_update_count = 0;
                                    Some("Cleared the saved store catalog.".into())
                                } else {
                                    None
                                }
                            }
                            Err(error) => Some(format!("The Store settings could not be changed. {error}")),
                        }
                    };
                    display.show(renderer.draw(&ui, &status), &settings, cmd == DeviceCmd::StoreClearCache);
                    continue;
                }
                // Power off: the screen says so, and stays that way once the Kindle is off (e-ink keeps its picture).
                if cmd == DeviceCmd::PowerOff {
                    let frame = renderer.message("Powered Off", "Press the power button to turn the Kindle on again.");
                    display.show(frame, &settings, true);
                    // Let the screen finish its refresh before the system halts.
                    std::thread::sleep(std::time::Duration::from_secs(3));
                }
                run_device(cmd, &mut status, &mut settings, &dir, &run, &settings_path);
                // The Kindle is going off: keep the "Powered Off" picture, do not draw the screen again.
                if cmd == DeviceCmd::PowerOff {
                    continue;
                }
                // Wi-Fi turned on from the Wi-Fi page: look for networks once it is up.
                if cmd == DeviceCmd::ToggleWifi && status.wifi_on && ui.screen == Screen::Page(kl_core::layout::Page::Wifi) {
                    ui.wifi_scanning = true;
                    start_wifi_scan_after(tx.clone(), 4);
                }
                let full = matches!(cmd, DeviceCmd::ToggleRotation | DeviceCmd::AutoRotation | DeviceCmd::ToggleDarkMode);
                display.show(renderer.draw(&ui, &status), &settings, full);
            }
        }
    }
}

/// `kl-launcher --wait-screen`: run by the supervisor while the stock Kindle UI starts (about a minute) and
/// stopped by it once the library is ready. It draws only the "Opening the Kindle UI" dialog, over what is on the
/// screen (the grid the launcher left), and fills one block of its bar every 1.5 s with a small partial update.
fn wait_screen(dir: &Path, run: &Path, settings: &Settings) {
    let mut renderer = match Renderer::new(W, H, &font_paths(dir, run)) {
        Ok(r) => r,
        Err(e) => return log(&format!("wait screen: {e}")),
    };
    log("wait screen");
    let layout = Layout::new(W, H, kl_core::config::DEFAULT_COLUMNS, kl_core::config::DEFAULT_ROWS);
    let mut display = Display::open();
    let mut step = 0;
    loop {
        let (frame, area) = renderer.waiting_dialog(&layout, step, settings.dark_mode);
        display.show_region(frame, area, settings);
        thread::sleep(Duration::from_millis(1500));
        step += 1;
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

fn apply_store_setting(cmd: DeviceCmd, launcher: &Path, settings_path: &Path, settings: &mut Settings) -> std::io::Result<()> {
    if cmd == DeviceCmd::StoreClearCache {
        return kl_store::refresh::clear_cache(launcher);
    }
    let mut updated = settings.clone();
    updated.store_daily_check = !settings.store_daily_check;
    write_atomic(settings_path, &updated.to_text())?;
    *settings = updated;
    Ok(())
}

fn run_device(cmd: DeviceCmd, status: &mut Status, settings: &mut Settings, dir: &Path, run: &Path, settings_path: &Path) {
    match cmd {
        DeviceCmd::Light(step) => device::set_light((status.light as i32 + step).clamp(0, device::LEVEL_MAX as i32) as u32),
        DeviceCmd::Warmth(step) => device::set_warmth((status.warmth as i32 + step).clamp(0, device::LEVEL_MAX as i32) as u32),
        DeviceCmd::ToggleWifi => device::set_wifi(!status.wifi_on),
        DeviceCmd::ToggleSsh => device::set_ssh(&dir.to_string_lossy(), !status.ssh),
        DeviceCmd::ToggleDns => device::set_public_dns(&dir.to_string_lossy(), !status.public_dns),
        DeviceCmd::Restart => device::system_command("reboot"),
        DeviceCmd::PowerOff => device::system_command("halt -f -p"),
        DeviceCmd::RestartLauncher => device::restart_launcher(),
        DeviceCmd::StoreRefresh
        | DeviceCmd::StoreRemove(_)
        | DeviceCmd::StoreOpen(_)
        | DeviceCmd::StoreInstall(_)
        | DeviceCmd::StoreUpdateAll
        | DeviceCmd::StoreToggleDailyCheck
        | DeviceCmd::StoreClearCache => {}
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
        DeviceCmd::SetLight(level) => device::set_light(level),
        DeviceCmd::SetWarmth(level) => device::set_warmth(level),
        DeviceCmd::ToggleAutoRotation => {
            // Quick settings' Auto-rotate: the same as Settings > Screen > Auto, from the device's turn.
            settings.auto_rotation = !settings.auto_rotation;
            if settings.auto_rotation {
                if let Some(turn) = device::accelerometer_turn() {
                    settings.rotation = turn;
                    input::TURN.store(turn.turn(), Ordering::Relaxed);
                    let _ = write_atomic(&run.join("orientation"), turn.letter());
                }
            }
            if let Err(e) = write_atomic(settings_path, &settings.to_text()) {
                log(&format!("settings: {e}"));
            }
        }
        DeviceCmd::ToggleDarkMode | DeviceCmd::ToggleAnimation => {
            if cmd == DeviceCmd::ToggleDarkMode {
                settings.dark_mode = !settings.dark_mode;
            } else {
                settings.animation = !settings.animation;
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
    fn store_settings_are_shared_and_cache_clear_preserves_install_data() {
        let root = std::env::temp_dir().join(format!("kl-launcher-store-settings-{}", std::process::id()));
        let store = root.join("store");
        let settings_path = root.join("settings.txt");
        let cache = store.join("cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("index.json"), "saved catalog").unwrap();
        fs::write(store.join("installed.json"), "installed records").unwrap();
        let mut settings = Settings::default();
        assert!(settings.store_daily_check);
        apply_store_setting(DeviceCmd::StoreToggleDailyCheck, &root, &settings_path, &mut settings).unwrap();
        let stored = Settings::parse(&fs::read_to_string(&settings_path).unwrap());
        assert!(!settings.store_daily_check);
        assert!(!stored.store_daily_check);
        for _ in 0..2 {
            apply_store_setting(DeviceCmd::StoreClearCache, &root, &settings_path, &mut settings).unwrap();
            assert!(!cache.exists());
            assert_eq!(fs::read_to_string(store.join("installed.json")).unwrap(), "installed records");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_store_request_uses_the_installed_apps_normal_launcher_entry() {
        let root = std::env::temp_dir().join(format!("kl-launcher-store-open-{}", std::process::id()));
        let documents = root.join("documents");
        let dir = root.join("launcher");
        fs::create_dir_all(&documents).unwrap();
        let filename = documents.join("Example App.sh");
        fs::write(&filename, "#!/bin/sh\n# Name: Example App\n").unwrap();
        let app = kl_store::record::Installed {
            id: "example-app".into(), files: vec!["documents/Example App.sh".into()], ..Default::default()
        };
        let mut record = kl_store::record::Record::new();
        record.apps.insert(app.id.clone(), app);
        kl_store::record::save(&dir.join("store/installed.json"), &record).unwrap();
        let mut apps = kl_core::apps::scriptlets(&documents);
        let next = store_launch_descriptor(&dir, &documents, &apps, "example-app").unwrap();
        assert_eq!(next, format!("{}\nscript\n{}\n", documents.display(), filename.display()));
        assert!(store_launch_descriptor(&dir, &documents, &apps, "uninstalled-app").is_err());
        apps[0].launch = Launch::Shell { command: "echo ignored".into() };
        assert!(store_launch_descriptor(&dir, &documents, &apps, "example-app").is_err());
        fs::remove_dir_all(root).unwrap();
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
