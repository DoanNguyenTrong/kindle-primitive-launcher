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
    /// The USB state changed (the cable, or the drive shown on a computer).
    Usb(kl_core::Usb),
    /// Draw the drive-mode screen again: the stock system draws its own USB screen when the drive goes to the computer.
    UsbRedraw,
    /// The device was turned (auto-rotation, after the debouncer).
    Turn(Rotation),
    /// A background app ended: its name, last output line and whether it succeeded.
    Background { name: String, line: String, ok: bool },
    /// A Wi-Fi scan finished.
    Wifi(kl_core::wifi::WifiInfo),
    /// A Wi-Fi join or forget finished: whether it worked, and what to tell the user.
    WifiResult { ok: bool, message: String },
    /// A background SSH job finished (set the password, check the server): what to tell the user.
    Notice(String),
    /// The store's catalog refresh finished: the store's entries again, and what to tell the user.
    StoreRefreshed { entries: Vec<kl_core::store::StoreEntry>, message: String },
    /// An app from the store was installed (or not): the store's entries, the library's apps again, and the message.
    StoreInstalled { entries: Vec<kl_core::store::StoreEntry>, message: String },
}

/// The catalog's published site (the store's source of listings, icons and screenshots).
const STORE_CATALOG: &str = "https://doannguyentrong.github.io/kindle-primitive-catalog/v1";

/// The store's catalog files over HTTPS, through the shared download code (only the store's hosts are reached).
struct NetSource;

impl kl_store::refresh::Source for NetSource {
    fn text(&self, url: &str) -> Result<String, String> {
        primitive_net::fetch_text(url, &primitive_net::Policy::store(kl_store::refresh::MAX_INDEX)).map_err(|e| e.to_string())
    }
    fn bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        primitive_net::fetch_bytes(url, &primitive_net::Policy::store(max)).map_err(|e| e.to_string())
    }
}

/// Install an app from the store on a thread: download its package for this Kindle, check it, install it, and tell the
/// store and the library. The download is refused when Wi-Fi is not connected (the caller checks).
fn start_store_install(tx: Sender<Ev>, dir: PathBuf, id: String) {
    thread::spawn(move || {
        log(&format!("store: install of {id} started"));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store_install_app(&dir, &id)))
            .unwrap_or_else(|_| Err("an internal error".to_string()));
        let message = match outcome {
            Ok(version) => format!("Installed {id} {version}."),
            Err(e) => format!("Could not install {id}: {e}"),
        };
        log(&format!("store: {message}"));
        let entries = load_store(&dir);
        let _ = tx.send(Ev::StoreInstalled { entries, message });
    });
}

fn store_install_app(dir: &Path, id: &str) -> Result<String, String> {
    let listing_text = fs::read_to_string(dir.join("store/cache/apps").join(format!("{id}.json")))
        .map_err(|_| "the app is not in the catalog the store has".to_string())?;
    let listing: kl_store::catalog::Listing = serde_json::from_str(&listing_text).map_err(|e| e.to_string())?;
    let device = device::store_device().ok_or("the Kindle's firmware version cannot be read")?;
    let version = kl_store::resolve::pick_version(&listing, &device).map_err(|e| e.to_string())?;
    let asset = version.assets.get(&device.arch).ok_or("no package for this Kindle")?;
    let root = dir.parent().unwrap_or(Path::new("/mnt/us")).to_path_buf();
    let paths = kl_store::install::Paths::under(&root);
    let downloads = dir.join("store/downloads");
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let zip = downloads.join(format!("{id}-{}-{}.zip", version.version, device.arch));
    let policy = primitive_net::Policy::store(asset.size + 1);
    primitive_net::download(&asset.url, &zip, &policy, |_, _| {}).map_err(|e| e.to_string())?;
    let installed_at = std::process::Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    kl_store::pipeline::install_downloaded(&paths, &listing, &device, &zip, &asset.url, &installed_at)
}

/// Refresh the store's catalog cache on a thread, then send the store's entries and a message. The screen is not held
/// while the files come in.
fn start_store_refresh(tx: Sender<Ev>, dir: PathBuf) {
    thread::spawn(move || {
        let cache = dir.join("store/cache");
        log("store: refresh started");
        // A panic must still end the refresh (the button waits for it), so it is caught and reported as an error.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| kl_store::refresh::refresh(STORE_CATALOG, &cache, &NetSource)))
            .unwrap_or_else(|_| Err("an internal error".to_string()));
        let message = match outcome {
            Ok(report) => {
                log(&format!("store: refreshed {} apps, {} failed, {} removed", report.refreshed, report.failed.len(), report.removed));
                if report.failed.is_empty() {
                    format!("Store updated: {} apps.", report.refreshed)
                } else {
                    format!("Store updated: {} apps. {} could not be updated.", report.refreshed, report.failed.len())
                }
            }
            Err(e) => {
                log(&format!("store: refresh failed: {e}"));
                format!("The store could not be updated: {e}")
            }
        };
        let entries = load_store(&dir);
        let _ = tx.send(Ev::StoreRefreshed { entries, message });
    });
}

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
/// The config (apps.toml), the apps (in documents), the KUAL extensions, and the problems found reading them. The
/// boot copy and /mnt/us read the same files; the directory decides which.
/// The store's entries: one per listing in the catalog cache (`store/cache/apps/*.json`), with what is installed.
fn load_store(dir: &Path) -> Vec<kl_core::store::StoreEntry> {
    let record = kl_store::record::load(&dir.join("store/installed.json")).unwrap_or_default();
    let Ok(read) = fs::read_dir(dir.join("store/cache/apps")) else {
        return Vec::new();
    };
    let mut entries: Vec<kl_core::store::StoreEntry> = read
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<kl_store::catalog::Listing>(&text).ok())
        .map(|listing| kl_core::store::StoreEntry::from_listing(&listing, record.apps.get(&listing.id)))
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn load_library(dir: &Path, documents: &Path) -> (Config, Vec<App>, Vec<kl_core::kual::MenuItem>, Vec<String>) {
    let (config, mut problems) = match fs::read_to_string(dir.join("apps.toml")) {
        Ok(text) => Config::parse(&text),
        Err(_) => (Config::default(), Vec::new()),
    };
    let (mut apps, more) = discover(documents, &config);
    problems.extend(more);
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

/// Shows frames on the panel: a full GC16 update on request, otherwise only the box around the changed
/// pixels, with a non-flashing waveform (DU for pure black and white, AUTO when icons bring gray). A tap on one
/// icon updates that icon, not the full-width band of its row.
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
            panel.draw(W, (0, 0, W, H), &frame, eink::WAVE_GC16, true).map(|ok| (ok, "GC16 full", (0, 0, W, H)))
        } else if let Some((x, y, w, h)) = kl_render::changed_rect(&self.last, &frame, W as usize) {
            let stride = W as usize;
            let bw = (y..y + h).all(|r| kl_render::is_black_and_white(&frame[r * stride + x..r * stride + x + w]));
            let (wave, name) = if bw { (eink::WAVE_DU, "DU") } else { (eink::WAVE_AUTO, "AUTO") };
            let region = (x as u32, y as u32, w as u32, h as u32);
            panel.draw(W, region, &frame, wave, false).map(|ok| (ok, name, region))
        } else {
            Ok((true, "unchanged", (0, 0, 0, 0)))
        };
        match result {
            Ok((true, wave, (x, y, w, h))) => log(&format!("draw {wave} {w}x{h} at {x},{y} in {} ms (fb turn {fb_turn})", t.elapsed().as_millis())),
            Ok((false, ..)) => log("draw: the framebuffer is not in a drawable state"),
            Err(e) => log(&format!("draw: {e}")),
        }
        self.last = frame;
    }

    /// Show only `area` of `upright` (a pop-up), leaving the rest of the panel as it is. The next `show` draws
    /// the whole screen, as what is on the panel is no longer known.
    fn show_region(&mut self, upright: Vec<u8>, area: kl_core::Rect, settings: &Settings) {
        let fb_turn = self.fb_turn();
        let turned = (settings.rotation.turn() + fb_turn) % 4 == 2;
        let frame = kl_render::for_panel(upright, settings.rotation, fb_turn);
        self.last.clear();
        let Some(panel) = self.panel.as_mut() else { return };
        // Upside down, the area sits at the opposite corner of the panel.
        let (x, y) = if turned { (W as i32 - area.right(), H as i32 - area.bottom()) } else { (area.x, area.y) };
        let x0 = x.clamp(0, W as i32) as u32 / 8 * 8;
        let x1 = ((x + area.w).clamp(0, W as i32) as u32).div_ceil(8) * 8;
        let (y0, y1) = (y.clamp(0, H as i32) as u32, (y + area.h).clamp(0, H as i32) as u32);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let region = (x0, y0, x1.min(W) - x0, y1 - y0);
        let stride = W as usize;
        let bw = (y0..y1).all(|r| kl_render::is_black_and_white(&frame[r as usize * stride + x0 as usize..r as usize * stride + (x0 + region.2) as usize]));
        let wave = if bw { eink::WAVE_DU } else { eink::WAVE_AUTO };
        if let Err(e) = panel.draw(W, region, &frame, wave, false) {
            log(&format!("draw: {e}"));
        }
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
    let mut dir = env_dir("KL_DIR", "/mnt/us/launcher");
    // Started from the copy on /var/local (the cable was in at boot): when /mnt/us is back, this same launcher reads its
    // settings, apps and extensions from /mnt/us.
    let mut from_mirror = dir.starts_with("/var/local");
    let run = env_dir("KL_RUN", "/var/tmp/kl");
    let documents = env_dir("KL_DOCUMENTS", "/mnt/us/documents");
    let _ = fs::create_dir_all(&run);
    log(&format!("start v{} dir {} run {}", kl_core::VERSION, dir.display(), run.display()));

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
    ui.set_icon_choices(png_files(&dir.join("icons")));
    ui.set_extensions(extensions);
    // An install that a power cut interrupted is finished or undone before the store reads what is installed.
    let store_root = dir.parent().unwrap_or(Path::new("/mnt/us")).to_path_buf();
    match kl_store::install::recover(&kl_store::install::Paths::under(&store_root)) {
        Ok(Some(what)) => log(&format!("store: {what}")),
        Ok(None) => {}
        Err(e) => log(&format!("store: cannot check an unfinished install: {e}")),
    }
    kl_render::load_store_pictures(&mut renderer, &dir.join("store/cache"));
    ui.set_store(load_store(&dir));
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

    while let Ok(ev) = rx.recv() {
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
            Ev::StoreInstalled { entries, message } => {
                // The library reads the app's entries from documents/ and its folder again, as after an eject.
                let (_config, apps, extensions, _problems) = load_library(&dir, &documents);
                ui.set_apps(apps);
                ui.set_extensions(extensions);
                for problem in kl_render::load_icons(&mut renderer, ui.apps(), &layout, Some(&dir.join("cache"))) {
                    log(&format!("problem: {problem}"));
                }
                ui.set_store(entries);
                ui.store_installing = None;
                if ui.screen == Screen::StoreApp {
                    ui.screen = Screen::Store;
                }
                ui.notice = Some(message);
                Action::Redraw { full: false }
            }
            Ev::StoreRefreshed { entries, message } => {
                kl_render::load_store_pictures(&mut renderer, &dir.join("store/cache"));
                ui.set_store(entries);
                ui.store_busy = false;
                if ui.screen == Screen::StoreApp {
                    ui.screen = Screen::Store;
                }
                ui.notice = Some(message);
                Action::Redraw { full: false }
            }
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
                        let (_config, apps, extensions, problems) = load_library(&dir, &documents);
                        ui.set_apps(apps);
                        ui.set_extensions(extensions);
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
            Action::Launch(app) => {
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
            Action::Device(DeviceCmd::StoreRefresh) => {
                log(&format!("store: refresh requested (wifi on: {}, state: {})", status.wifi_on, status.wifi_state));
                if status.wifi_on && status.wifi_state == "connected" {
                    start_store_refresh(tx.clone(), dir.clone());
                    ui.notice = Some("Updating the store...".to_string());
                } else {
                    ui.store_busy = false;
                    ui.notice = Some("Connect to Wi-Fi to update the store.".to_string());
                }
                display.show(renderer.draw(&ui, &status), &settings, false);
            }
            Action::Device(cmd) => {
                if let DeviceCmd::StoreRemove(index) = cmd {
                    let name = ui.store.get(index).map(|e| (e.id.clone(), e.name.clone()));
                    if let Some((id, name)) = name {
                        let root = dir.parent().unwrap_or(Path::new("/mnt/us")).to_path_buf();
                        match kl_store::install::remove(&kl_store::install::Paths::under(&root), &id) {
                            Ok(()) => {
                                let (_config, apps, extensions, _problems) = load_library(&dir, &documents);
                                ui.set_apps(apps);
                                ui.set_extensions(extensions);
                                for problem in kl_render::load_icons(&mut renderer, ui.apps(), &layout, Some(&dir.join("cache"))) {
                                    log(&format!("problem: {problem}"));
                                }
                                ui.set_store(load_store(&dir));
                                ui.screen = Screen::Store;
                                ui.notice = Some(format!("Removed {name} from this Kindle."));
                                log(&format!("store: removed {id}"));
                            }
                            Err(e) => ui.notice = Some(format!("Could not remove {name}: {e}")),
                        }
                    }
                    display.show(renderer.draw(&ui, &status), &settings, false);
                    continue;
                }
                if let DeviceCmd::StoreInstall(index) = cmd {
                    match ui.store.get(index).map(|e| (e.id.clone(), e.name.clone())) {
                        Some((id, name)) if status.wifi_on && status.wifi_state == "connected" => {
                            start_store_install(tx.clone(), dir.clone(), id);
                            ui.notice = Some(format!("Installing {name}..."));
                        }
                        Some(_) => {
                            ui.store_installing = None;
                            ui.notice = Some("Connect to Wi-Fi to install from the store.".to_string());
                        }
                        None => ui.store_installing = None,
                    }
                    display.show(renderer.draw(&ui, &status), &settings, false);
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
        // The download of a store app is the store's network step (implementation plan D2): not wired yet.
        // The catalog download runs on a thread (see `start_store_refresh`), not here.
        DeviceCmd::StoreRefresh => {}
        // Remove runs in the device loop (it reloads the library): see the Action::Device arm.
        DeviceCmd::StoreRemove(_) => {}
        DeviceCmd::StoreInstall(index) => log(&format!("store: install of entry {index} requested; the download is not wired yet")),
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
