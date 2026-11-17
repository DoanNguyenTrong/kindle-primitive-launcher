//! Desktop driver: draws the launcher's screens to PNG files for review, with the same core and renderer
//! as the Kindle build. No device calls.
//!
//!   kl-desktop --documents DIR [--config apps.toml] [--fonts DIR] --out DIR
//!   kl-desktop --launcher-icon FILE.png [--fonts DIR]
//!
//! Screens written: home (each page), pressed tile, settings, confirm, sleep.

use std::{fs, path::PathBuf};

use kl_core::{discover, Config, Input, Layout, Rotation, Status, Ui};
use kl_render::Renderer;

const W: u32 = 1264;
const H: u32 = 1680;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn write_png(path: &PathBuf, w: u32, h: u32, gray: &[u8]) {
    let file = fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(gray).unwrap();
    println!("wrote {}", path.display());
}

fn fonts(dir: &str) -> Vec<String> {
    ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"].iter().map(|f| format!("{dir}/{f}")).collect()
}

fn demo_status() -> Status {
    Status {
        clock: "10:42".into(),
        date: "Tue 17 Nov".into(),
        battery: Some(78),
        charging: false,
        wifi_on: true,
        wifi_state: "connected".into(),
        light: 12,
        warmth: 6,
        ssh: false,
        autostart: true,
        sleep_launcher: false,
        auto_rotation: true,
        rotation: Rotation::Upright,
        memory: "MemAvailable 312 MB".into(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let font_dir = arg(&args, "--fonts").unwrap_or_else(|| "assets/fonts".into());
    if let Some(path) = arg(&args, "--launcher-icon") {
        let mut r = Renderer::new(600, 920, &fonts(&font_dir)).expect("fonts");
        let gray = r.launcher_icon();
        write_png(&PathBuf::from(path), 600, 920, &gray);
        return;
    }
    let documents = PathBuf::from(arg(&args, "--documents").expect("--documents DIR"));
    let out = PathBuf::from(arg(&args, "--out").expect("--out DIR"));
    fs::create_dir_all(&out).unwrap();
    let (config, mut problems) = match arg(&args, "--config") {
        Some(p) => Config::parse(&fs::read_to_string(&p).unwrap_or_default()),
        None => (Config::default(), Vec::new()),
    };
    let (apps, more) = discover(&documents, &config);
    problems.extend(more);
    let extensions_root = documents.parent().map(|p| p.join("extensions")).unwrap_or_default();
    let (extensions, more) = kl_core::kual::discover_root(&extensions_root);
    problems.extend(more);
    let layout = Layout::new(W, H, config.columns, config.rows);
    let mut renderer = Renderer::new(W, H, &fonts(&font_dir)).expect("fonts");
    problems.extend(kl_render::load_icons(&mut renderer, &apps, &layout, None));
    for p in &problems {
        eprintln!("problem: {p}");
    }
    let mut ui = Ui::new(apps, layout);
    ui.set_extensions(extensions);
    let status = demo_status();
    let shot = |r: &mut Renderer, ui: &Ui, name: &str| {
        let t = std::time::Instant::now();
        let gray = r.draw(ui, &status);
        eprintln!("{name}: drawn in {} ms", t.elapsed().as_millis());
        write_png(&out.join(format!("{name}.png")), W, H, &gray);
    };
    for page in 0..ui.pages() {
        ui.page = page;
        shot(&mut renderer, &ui, &format!("home-{}", page + 1));
    }
    ui.page = 0;
    ui.notice = problems.first().cloned();
    if ui.notice.is_some() {
        shot(&mut renderer, &ui, "home-notice");
        ui.notice = None;
    }
    // Low battery: the footer message and the status strip must agree.
    let mut low = demo_status();
    low.battery = Some(9);
    ui.set_battery(low.battery, low.charging);
    let gray = renderer.draw(&ui, &low);
    write_png(&out.join("home-low-battery.png"), W, H, &gray);
    ui.set_battery(Some(90), false);
    // Status icons: charging with Wi-Fi on but not connected, and full with Wi-Fi off.
    let mut charging = demo_status();
    charging.battery = Some(37);
    charging.charging = true;
    charging.wifi_state = "not connected".into();
    write_png(&out.join("status-charging.png"), W, H, &renderer.draw(&ui, &charging));
    let mut full = demo_status();
    full.battery = Some(100);
    full.wifi_on = false;
    full.wifi_state = "off".into();
    write_png(&out.join("status-wifi-off.png"), W, H, &renderer.draw(&ui, &full));
    ui.pressed = Some(1);
    shot(&mut renderer, &ui, "home-pressed");
    ui.pressed = None;
    ui.handle(Input::Tap(600.0, 40.0));
    shot(&mut renderer, &ui, "settings");
    ui.screen = kl_core::Screen::ConfirmKindle;
    shot(&mut renderer, &ui, "confirm");
    ui.screen = kl_core::Screen::Home;
    let first = ui.layout.cell(0);
    ui.handle(Input::LongTap(first.x as f32 + 5.0, first.y as f32 + 5.0));
    shot(&mut renderer, &ui, "app-menu");
    ui.screen = kl_core::Screen::Home;
    if ui.apps().iter().any(|a| a.id == "extensions") {
        ui.open_extensions();
        shot(&mut renderer, &ui, "extensions");
        if let Some(i) = ui.ext_items().iter().position(|item| item.is_submenu()) {
            let row = ui.layout.list_row(i);
            ui.handle(Input::Tap(row.x as f32 + 10.0, row.y as f32 + 10.0));
            shot(&mut renderer, &ui, "extensions-submenu");
        }
        ui.screen = kl_core::Screen::Home;
    }
    ui.handle(Input::Sleep);
    shot(&mut renderer, &ui, "sleep");
    // The launcher's own sleep screen with a picture (the launcher icon stands in for one).
    match kl_render::sleep_image(std::path::Path::new("assets/launcher-icon.png"), W, H) {
        Ok(picture) => {
            renderer.set_sleep_image(Some(picture));
            shot(&mut renderer, &ui, "sleep-picture");
            renderer.set_sleep_image(None);
        }
        Err(e) => eprintln!("sleep picture: {e}"),
    }
    for step in [0, 4] {
        let gray = renderer.waiting("Opening the Kindle UI", "This takes about a minute.", step);
        write_png(&out.join(format!("waiting-{step}.png")), W, H, &gray);
    }
}
