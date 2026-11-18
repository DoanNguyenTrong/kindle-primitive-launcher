//! Draws the launcher's screens into an 8-bit gray buffer: shapes with `vello_cpu`, text with `parley`
//! (the same libraries and versions as Primitive Browser), icons as pre-dithered gray tiles.
//!
//! E-ink rules (see the `kindle-eink` skill): chrome and text are pure black on pure white after
//! `solidify`, so page changes do not flash; only icons carry gray, dithered to the panel's 16 steps.

pub mod image;

use std::collections::HashMap;

use kl_core::{
    layout::{Button, QuickTile, Rect, Row, LABEL_SIZE, QUICK_LEVEL_MAX},
    App, IconSource, Launch, Layout, Rotation, Screen, Status, Ui,
};
use parley::{FontContext, FontWeight, LayoutContext, PositionedLayoutItem, StyleProperty};
use vello_cpu::{
    kurbo::{self, Affine, Shape},
    peniko::Color,
    Glyph, Pixmap, RenderContext, Resources,
};

pub use image::GrayImage;

const BLACK: u8 = 0;
const WHITE: u8 = 255;
/// The horizontal margin of the Settings cards.
const MARGIN_X: i32 = 40;

/// The switch's rectangle in a row: at the right, centred vertically.
fn switch_rect(r: Rect) -> Rect {
    Rect::new(r.right() - 40 - kl_core::layout::SWITCH_W, r.y + (r.h - kl_core::layout::SWITCH_H) / 2, kl_core::layout::SWITCH_W, kl_core::layout::SWITCH_H)
}

/// The Wi-Fi icon's lit arcs for a status: None for Wi-Fi off (the slash), the signal's arcs when connected,
/// and no arcs while Wi-Fi is on without a network.
fn wifi_lit(s: &Status) -> Option<usize> {
    match (s.wifi_on, s.wifi_state == "connected") {
        (false, _) => None,
        (true, false) => Some(0),
        (true, true) => Some(match s.wifi_signal {
            Some((level, max)) => kl_core::wifi::arcs(level, max),
            None => 3,
        }),
    }
}

/// Pop-ups (the app menu, the Kindle UI dialog): corner radius, and the white margin around them.
const POPUP_RADIUS: f32 = 28.0;
/// The power symbol of the Power tile: a stroke from the top and an arc round the bottom (24-unit grid).
const POWER_ICON: primitive_quick::Icon = primitive_quick::Icon { stroke: &["M12 3 V11", "M6.3 6.3 A8 8 0 1 0 17.7 6.3"], fill: &[] };
const POPUP_HALO: i32 = 10;

// Icon paths copied from Primitive Browser (`crates/kb-app/src/ui.rs`, the `icon` set and `battery_icon`).
const WIFI_ARC_OUTER: &str = "M2.5 9 A14 14 0 0 1 21.5 9";
const WIFI_ARC_MIDDLE: &str = "M5.5 12.5 A9.5 9.5 0 0 1 18.5 12.5";
const WIFI_ARC_INNER: &str = "M8.5 16 A5 5 0 0 1 15.5 16";
const BOLT: &str = "M8 0 L1 12 H6 L4 22 L13 9 H8 Z";
/// Wi-Fi arcs above the signal strength.
const UNLIT: u8 = 187;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Start,
    Center,
    End,
}

/// One text block: wrapped to `w`, at most `lines` lines (the last one shortened with "…").
struct Text<'a> {
    text: &'a str,
    x: f32,
    y: f32,
    w: f32,
    size: f32,
    bold: bool,
    align: Align,
    lines: usize,
    gray: u8,
}

pub struct Renderer {
    width: u16,
    ctx: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
    fonts: FontContext,
    layout_cx: LayoutContext<()>,
    icons: HashMap<String, GrayImage>,
    /// The launcher's own sleep screen picture, when one is set (screensavers folder).
    sleep_image: Option<GrayImage>,
    /// The drive-mode picture (`assets/usb-drive-mode.png`), fitted to the screen and snapped to black and white.
    usb_picture: Option<GrayImage>,
}

/// The drive-mode picture: a laptop and a Kindle joined by a cable.
const USB_PICTURE: &[u8] = include_bytes!("../../../assets/usb-drive-mode.png");

fn gray(v: u8) -> Color {
    Color::from_rgb8(v, v, v)
}

fn krect(r: Rect) -> kurbo::Rect {
    kurbo::Rect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64)
}

impl Renderer {
    /// A renderer for a `width` × `height` screen with the fonts at `font_paths` (Noto Sans regular and
    /// bold). Fails when no font could be loaded.
    pub fn new(width: u32, height: u32, font_paths: &[String]) -> Result<Renderer, String> {
        let mut fonts = FontContext::new();
        let existing: Vec<&String> = font_paths.iter().filter(|p| std::fs::metadata(p).is_ok()).collect();
        if existing.is_empty() {
            return Err(format!("no fonts found in {font_paths:?}"));
        }
        fonts.collection.load_fonts_from_paths(existing.iter().map(|p| p.as_str()));
        let Some(family) = fonts.collection.family_id("Noto Sans") else {
            return Err("font family Noto Sans not found".into());
        };
        fonts.collection.set_generic_families(parley::GenericFamily::SansSerif, std::iter::once(family));
        let (w, h) = (width as u16, height as u16);
        Ok(Renderer {
            width: w,
            ctx: RenderContext::new(w, h),
            resources: Resources::new(),
            pixmap: Pixmap::new(w, h),
            fonts,
            layout_cx: LayoutContext::new(),
            icons: HashMap::new(),
            sleep_image: None,
            usb_picture: image::decode_png(USB_PICTURE).ok().map(|img| {
                let mut fitted = image::fit(&img, width - 120, height - 240);
                image::solidify(&mut fitted.pixels);
                fitted
            }),
        })
    }

    /// Set the dithered icon tile for an app id (sized to the layout's icon box).
    pub fn set_icon(&mut self, id: &str, tile: GrayImage) {
        self.icons.insert(id.to_string(), tile);
    }

    /// The picture for the next sleep screen (`None`: the text sleep screen).
    pub fn set_sleep_image(&mut self, image: Option<GrayImage>) {
        self.sleep_image = image;
    }

    /// Drop an app's tile (its icon was changed; the driver loads the new one).
    pub fn forget_icon(&mut self, id: &str) {
        self.icons.remove(id);
    }

    /// Whether a sleep picture is set (the launcher's own sleep screen has something to show).
    pub fn has_sleep_image(&self) -> bool {
        self.sleep_image.is_some()
    }

    pub fn has_icon(&self, id: &str) -> bool {
        self.icons.contains_key(id)
    }

    // ----- primitives -----

    fn fill(&mut self, r: Rect, v: u8) {
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.set_paint(gray(v));
        self.ctx.fill_rect(&krect(r));
    }

    fn frame(&mut self, r: Rect, width: i32, v: u8) {
        self.fill(Rect::new(r.x, r.y, r.w, width), v);
        self.fill(Rect::new(r.x, r.bottom() - width, r.w, width), v);
        self.fill(Rect::new(r.x, r.y, width, r.h), v);
        self.fill(Rect::new(r.right() - width, r.y, width, r.h), v);
    }

    fn rounded(&mut self, r: Rect, radius: f64, v: u8) {
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.set_paint(gray(v));
        let path = kurbo::RoundedRect::from_rect(krect(r), radius).to_path(0.1);
        self.ctx.fill_path(&path);
    }

    /// Stroke an SVG path drawn on the browser's 24-unit icon grid, scaled to `size` pixels at (`x`, `y`).
    fn stroke_icon(&mut self, d: &str, x: f64, y: f64, size: f64, width: f64, v: u8) {
        let Ok(path) = kurbo::BezPath::from_svg(d) else { return };
        self.ctx.set_transform(Affine::translate((x, y)) * Affine::scale(size / 24.0));
        self.ctx.set_stroke(kurbo::Stroke::new(width).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
        self.ctx.set_paint(gray(v));
        self.ctx.stroke_path(&path);
        self.ctx.set_transform(Affine::IDENTITY);
    }

    /// Fill an SVG path at (`x`, `y`) with `scale` (path units to pixels).
    fn fill_svg(&mut self, d: &str, x: f64, y: f64, scale: f64, v: u8) {
        let Ok(path) = kurbo::BezPath::from_svg(d) else { return };
        self.ctx.set_transform(Affine::translate((x, y)) * Affine::scale(scale));
        self.ctx.set_paint(gray(v));
        self.ctx.fill_path(&path);
        self.ctx.set_transform(Affine::IDENTITY);
    }

    /// The Wi-Fi icon from Primitive Browser (`kb-app/src/ui.rs`, "wifi" and "wifi-off"): three arcs and a dot
    /// on a 24-unit grid, stroke 2. `lit` arcs (from the inside, by signal strength) are black and the rest light
    /// gray; `None` draws the slash of Wi-Fi off.
    fn wifi_icon(&mut self, x: f64, y: f64, size: f64, lit: Option<usize>) {
        let shown = lit.unwrap_or(3);
        for (i, arc) in [WIFI_ARC_INNER, WIFI_ARC_MIDDLE, WIFI_ARC_OUTER].iter().enumerate() {
            self.stroke_icon(arc, x, y, size, 2.0, if i < shown { BLACK } else { UNLIT });
        }
        let k = size / 24.0;
        self.ctx.set_paint(gray(BLACK));
        let dot = kurbo::Circle::new((x + 12.0 * k, y + 19.5 * k), 1.6 * k).to_path(0.1);
        self.ctx.fill_path(&dot);
        if lit.is_none() {
            // A white gap under the slash keeps it readable over the arcs.
            self.stroke_icon("M3 3 L21 21", x, y, size, 5.0, WHITE);
            self.stroke_icon("M3 3 L21 21", x, y, size, 2.4, BLACK);
        }
    }

    /// The battery icon from Primitive Browser (`kb-app/src/ui.rs`, `battery_icon`): a 128 × 56 battery filled
    /// to the level in black, the number in its middle, black on the empty part and white on the filled part
    /// (split at the level's edge), and a bolt while charging.
    fn battery_icon(&mut self, x: i32, y: i32, level: u32, charging: bool) {
        let pct = level.min(100);
        let fill = (104.0 * pct as f32 / 100.0).round() as i32;
        self.rounded_frame(Rect::new(x + 2, y + 4, 116, 48), 12.0, 3, BLACK, WHITE);
        self.rounded(Rect::new(x + 122, y + 18, 5, 20), 2.0, BLACK);
        if fill > 0 {
            self.rounded(Rect::new(x + 8, y + 10, fill, 36), 6.0f64.min(fill as f64 / 2.0), BLACK);
        }
        let edge = (x + 8 + fill) as f64;
        let number = pct.to_string();
        let draw_number = |r: &mut Renderer, v: u8| {
            let w = r.text_width(&number, 28.0, true);
            let bolt_w = if charging { 18.0 } else { 0.0 };
            let left = x as f32 + (128.0 - w - bolt_w) / 2.0;
            if charging {
                r.fill_svg(BOLT, (left - 2.0) as f64, (y + 17) as f64, 1.0, v);
            }
            r.text(Text { text: &number, x: left + bolt_w, y: (y + 11) as f32, w: w + 4.0, size: 28.0, bold: true, align: Align::Start, lines: 1, gray: v });
        };
        draw_number(self, BLACK);
        if fill > 0 {
            let clip = kurbo::Rect::new(x as f64, y as f64, edge, (y + 56) as f64).to_path(0.1);
            self.ctx.set_transform(Affine::IDENTITY);
            self.ctx.push_clip_layer(&clip);
            draw_number(self, WHITE);
            self.ctx.pop_layer();
        }
    }

    /// A rounded outline: an outer fill in `line`, an inner fill in `inside`.
    fn rounded_frame(&mut self, r: Rect, radius: f64, width: i32, line: u8, inside: u8) {
        self.rounded(r, radius, line);
        self.rounded(r.inset(width), (radius - width as f64).max(0.0), inside);
    }

    /// Lay out and draw text. Returns the height used.
    fn text(&mut self, t: Text) -> f32 {
        let mut content = t.text.to_string();
        // Shorten until it fits in `lines` lines.
        loop {
            let layout = self.layout_text(&content, t.size, t.bold, t.w);
            let count = layout.lines().count();
            if count <= t.lines || content.chars().count() <= 1 {
                return self.draw_layout(&layout, &t);
            }
            let keep: String = content.trim_end_matches('…').chars().collect();
            let mut chars: Vec<char> = keep.chars().collect();
            let drop = (chars.len() / 10).max(1);
            chars.truncate(chars.len().saturating_sub(drop));
            content = format!("{}…", chars.into_iter().collect::<String>().trim_end());
        }
    }

    fn layout_text(&mut self, text: &str, size: f32, bold: bool, w: f32) -> parley::Layout<()> {
        let mut builder = self.layout_cx.ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::FontWeight(if bold { FontWeight::BOLD } else { FontWeight::NORMAL }));
        builder.push_default(StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(1.25)));
        let mut layout: parley::Layout<()> = builder.build(text);
        layout.break_all_lines(Some(w));
        layout
    }

    fn draw_layout(&mut self, layout: &parley::Layout<()>, t: &Text) -> f32 {
        let mut height = 0.0;
        for line in layout.lines().take(t.lines) {
            let m = line.metrics();
            let dx = match t.align {
                Align::Start => 0.0,
                Align::Center => ((t.w - m.advance) / 2.0).max(0.0),
                Align::End => (t.w - m.advance).max(0.0),
            };
            height = m.offset + m.line_height;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
                let run = glyph_run.run();
                self.ctx.set_transform(Affine::translate(((t.x + dx) as f64, t.y as f64)));
                self.ctx.set_paint(gray(t.gray));
                let _ = self
                    .ctx
                    .glyph_run(&mut self.resources, run.font())
                    .font_size(run.font_size())
                    .hint(true)
                    // The bundled Noto fonts are static, so there are no variation coordinates.
                    .normalized_coords(&[])
                    .fill_glyphs(glyph_run.positioned_glyphs().map(|g| Glyph { id: g.id, x: g.x, y: g.y }));
            }
        }
        self.ctx.set_transform(Affine::IDENTITY);
        height
    }

    /// The pixel width of one line of text.
    fn text_width(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        let layout = self.layout_text(text, size, bold, f32::MAX);
        let width = layout.lines().next().map_or(0.0, |l| l.metrics().advance);
        width
    }

    /// Rasterise the recorded scene into a solidified gray buffer (premultiplied RGBA over an opaque white
    /// page: the red channel is the gray).
    /// The drive-mode screen: the Kindle shows on a computer. Eject it there, and the Kindle charges.
    fn usb_drive_frame(&mut self, ui: &Ui) -> Vec<u8> {
        self.ctx.reset();
        let l = ui.layout;
        self.fill(l.screen(), WHITE);
        let mut out = self.finish_gray();
        // The picture, centred on the white screen.
        if let Some(pic) = &self.usb_picture {
            let (w, h) = (l.width as usize, l.height as usize);
            let (pw, ph) = (pic.width as usize, pic.height as usize);
            let (ox, oy) = ((w - pw) / 2, (h - ph) / 2);
            for y in 0..ph {
                let dst = (oy + y) * w + ox;
                out[dst..dst + pw].copy_from_slice(&pic.pixels[y * pw..(y + 1) * pw]);
            }
        }
        out
    }

    fn finish_gray(&mut self) -> Vec<u8> {
        self.ctx.flush();
        self.ctx.render(self.pixmap.as_mut(), &mut self.resources);
        let mut out: Vec<u8> = self.pixmap.data_as_u8_slice().iter().step_by(4).copied().collect();
        image::solidify(&mut out);
        out
    }

    // ----- screens -----

    /// Draw the current screen. Returns `width × height` gray pixels, upright.
    pub fn draw(&mut self, ui: &Ui, status: &Status) -> Vec<u8> {
        // While the drive is shown on a computer, the drive-mode screen is all there is.
        if ui.usb == kl_core::Usb::Drive {
            return self.usb_drive_frame(ui);
        }
        self.ctx.reset();
        let l = ui.layout;
        self.fill(l.screen(), WHITE);
        let mut icon_blits: Vec<(Rect, String)> = Vec::new();
        match ui.screen {
            Screen::Home => self.home(ui, status, &mut icon_blits),
            Screen::Settings => self.settings_home(ui, status),
            Screen::Page(page) => self.settings_page(ui, status, page),
            Screen::NetworkDetail => self.network_detail(ui),
            Screen::Password => self.password_screen(ui),
            Screen::NetworkName => self.network_name_screen(ui),
            Screen::SshPassword => self.ssh_password_screen(ui),
            // The dialog is a pop-up over the grid, drawn last (below).
            Screen::ConfirmKindle => self.home(ui, status, &mut icon_blits),
            Screen::PowerMenu => self.home(ui, status, &mut icon_blits),
            // A sleep picture replaces the text, except for a battery warning, which matters more.
            Screen::Sleep => {
                if self.sleep_image.is_none() || ui.battery_message().is_some() {
                    self.sleep(&l, ui.battery_message());
                }
            }
            Screen::AppMenu => self.app_menu(ui, status, &mut icon_blits),
            Screen::Extensions => self.extensions(ui),
            // The panel is a pop-up over the home screen, drawn last (below).
            Screen::QuickSettings => self.home(ui, status, &mut icon_blits),
        }
        let width = self.width as usize;
        let mut out = self.finish_gray();
        // Pictures (icons, the sleep picture) keep their look in dark mode.
        let mut pictures = Vec::new();
        for (rect, id) in icon_blits {
            if let Some(tile) = self.icons.get(&id) {
                blit(&mut out, width, rect, tile);
                pictures.push(((rect.x, rect.y, rect.w, rect.h), rect.w as f32 * image::ICON_RADIUS));
            }
        }
        if ui.screen == Screen::Sleep && ui.battery_message().is_none() {
            if let Some(picture) = &self.sleep_image {
                let s = l.screen();
                let r = Rect::new((s.w - picture.width as i32) / 2, (s.h - picture.height as i32) / 2, picture.width as i32, picture.height as i32);
                blit(&mut out, width, r, picture);
                pictures.push(((r.x, r.y, r.w, r.h), 0.0));
            }
        }
        // Pop-ups are drawn in a second pass and copied over the screen, so icon pictures stay under them.
        let popup = match ui.screen {
            Screen::ConfirmKindle => Some(l.confirm_box()),
            Screen::PowerMenu => Some(l.power_box()),
            Screen::AppMenu => Some(l.menu_box(ui.menu_anchor(), ui.menu_actions().len())),
            Screen::QuickSettings => Some(l.quick_panel()),
            _ => None,
        };
        // Quick settings is a sheet from the top edge: square, with no margin.
        let sheet = ui.screen == Screen::QuickSettings;
        let (radius, halo) = if sheet { (0.0, 0) } else { (POPUP_RADIUS, POPUP_HALO) };
        if let Some(b) = popup {
            self.ctx.reset();
            self.fill(l.screen(), WHITE);
            match ui.screen {
                Screen::ConfirmKindle => self.confirm(&l),
                Screen::PowerMenu => self.power_menu_box(&l),
                Screen::QuickSettings => self.quick_panel(&l, status),
                _ => self.app_menu_box(ui),
            }
            let layer = self.finish_gray();
            image::copy_rounded(&mut out, &layer, width, (b.x - halo, b.y - halo, b.w + 2 * halo, b.h + 2 * halo), radius + halo as f32);
        }
        if status.dark_mode && ui.screen != Screen::Sleep {
            let over = popup.map(|b| (b.x - halo, b.y - halo, b.w + 2 * halo, b.h + 2 * halo));
            image::invert_except(&mut out, width, &pictures, over);
        }
        // The pressed look: only the touched icon darkens.
        if let (Screen::Home, Some(r)) = (ui.screen, ui.pressed) {
            image::darken_icon(&mut out, width, r.x, r.y, r.w);
        }
        out
    }

    /// The "Opening the Kindle UI" dialog, where the confirm dialog was: a frame of the whole screen in which only
    /// the returned rectangle (the dialog and its margin) is drawn. The caller shows that rectangle over what is on
    /// the screen. `step` fills one more block each time.
    pub fn waiting_dialog(&mut self, l: &Layout, step: usize, dark: bool) -> (Vec<u8>, Rect) {
        const BLOCKS: i32 = 10;
        self.ctx.reset();
        self.fill(l.screen(), WHITE);
        let b = l.confirm_box();
        self.rounded_frame(b, POPUP_RADIUS as f64, 6, BLACK, WHITE);
        let inner = b.inset(48);
        self.text(Text { text: "Opening the Kindle UI", x: inner.x as f32, y: inner.y as f32, w: inner.w as f32, size: 50.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.text(Text { text: "This takes about a minute. The Kindle library opens by itself.", x: inner.x as f32, y: inner.y as f32 + 100.0, w: inner.w as f32, size: 36.0, bold: false, align: Align::Start, lines: 3, gray: BLACK });
        let (bw, gap, bh) = (72, 18, 40);
        let x0 = b.x + (b.w - (BLOCKS * bw + (BLOCKS - 1) * gap)) / 2;
        let filled = (step % (BLOCKS as usize + 1)) as i32;
        for i in 0..BLOCKS {
            let r = Rect::new(x0 + i * (bw + gap), b.bottom() - 48 - bh - 30, bw, bh);
            self.rounded_frame(r, 8.0, 4, BLACK, if i < filled { BLACK } else { WHITE });
        }
        let mut out = self.finish_gray();
        let halo = b.inset(-POPUP_HALO);
        if dark {
            image::invert_except(&mut out, self.width as usize, &[], None);
        }
        (out, halo)
    }

    /// The status bar, laid out like the quick settings header: the clock and date on the left; on the right the
    /// indicators (SSH, a running background job), then Wi-Fi with its signal (a slash when off), then the battery.
    fn status_strip(&mut self, l: &Layout, s: &Status, running: usize) {
        let r = l.status();
        let clock = if s.clock.is_empty() { "--:--" } else { s.clock.as_str() };
        let c = l.header_clock();
        self.text(Text { text: clock, x: c.x as f32, y: 14.0, w: 400.0, size: 52.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        if !s.date.is_empty() {
            self.text(Text { text: &s.date, x: c.x as f32, y: 76.0, w: 400.0, size: 24.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
        }
        if let Some(level) = s.battery {
            let b = l.header_battery();
            self.battery_icon(b.x, b.y, level, s.charging);
        }
        let w = l.header_wifi();
        self.wifi_icon(w.x as f64, w.y as f64, 64.0, wifi_lit(s));
        // Text and badges to the left of Wi-Fi, laid out from right to left; `right` is the left edge of what is drawn.
        let mut right = w.x - 20;
        if s.wifi_on && s.wifi_state != "connected" {
            let note = if s.wifi_state.is_empty() { "Wi-Fi …".to_string() } else { "No network".to_string() };
            let width = self.text_width(&note, 30.0, false);
            let left = right as f32 - width;
            self.text(Text { text: &note, x: left, y: 44.0, w: width + 4.0, size: 30.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            right = (left - 20.0) as i32;
        }
        if running > 0 {
            right -= self.badge_width("Running") as i32;
            self.badge(right as f64, "Running");
            right -= 12;
        }
        if s.ssh {
            right -= self.badge_width("SSH") as i32;
            self.badge(right as f64, "SSH");
        }
        self.fill(Rect::new(0, r.bottom() - 2, r.w, 2), BLACK);
    }

    /// The width a badge with `label` takes.
    fn badge_width(&mut self, label: &str) -> f64 {
        self.text_width(label, 26.0, true) as f64 + 24.0
    }

    /// A small outlined label in the status bar at `x`. Returns its width.
    fn badge(&mut self, x: f64, label: &str) -> f64 {
        let w = self.badge_width(label);
        let r = Rect::new(x as i32, 38, w as i32, 44);
        self.rounded_frame(r, 10.0, 3, BLACK, WHITE);
        self.text(Text { text: label, x: (x + 12.0) as f32, y: 42.0, w: w as f32, size: 26.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        w
    }

    /// Quick settings, as Primitive Browser's panel: a white sheet from the top edge with the clock and date, the
    /// Wi-Fi and battery icons, two level rows of 25 steps with round − and + buttons, a grid of cards, and a
    /// Close handle.
    fn quick_panel(&mut self, l: &Layout, s: &Status) {
        let p = l.quick_panel();
        self.fill(p, WHITE);
        self.fill(Rect::new(0, p.bottom() - 3, p.w, 3), BLACK);
        let clock = if s.clock.is_empty() { "--:--" } else { s.clock.as_str() };
        let c = l.header_clock();
        self.text(Text { text: clock, x: c.x as f32, y: 14.0, w: 400.0, size: 52.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.text(Text { text: &s.date, x: c.x as f32, y: 76.0, w: 400.0, size: 24.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
        if let Some(level) = s.battery {
            let b = l.header_battery();
            self.battery_icon(b.x, b.y, level, s.charging);
        }
        let w = l.header_wifi();
        self.wifi_icon(w.x as f64, w.y as f64, 64.0, wifi_lit(s));
        // Round icon buttons, above the sliders: black with a white icon when on, outlined when off, with a small
        // caption underneath.
        for (tile, slot) in l.quick_buttons() {
            let (label, on) = match tile {
                QuickTile::Airplane => ("Airplane", !s.wifi_on),
                QuickTile::AutoRotate => ("Auto-rotate", s.auto_rotation),
                QuickTile::DarkMode => ("Dark mode", s.dark_mode),
                QuickTile::Animation => ("Animation", s.animation),
                QuickTile::Ssh => ("SSH", s.ssh),
                QuickTile::AllSettings => ("Settings", false),
                QuickTile::Power => ("Power", false),
            };
            let (bg, fg) = if on { (BLACK, WHITE) } else { (WHITE, BLACK) };
            let c = l.quick_button_circle(slot);
            self.rounded_frame(c, c.w as f64 / 2.0, 3, BLACK, bg);
            let size = 52.0;
            let (cx, cy) = ((c.x + c.w / 2) as f64, (c.y + c.h / 2) as f64);
            let (ix, iy) = (cx - size / 2.0, cy - size / 2.0);
            // The shared icons (`primitive-quick`), the same as Primitive Browser's panel.
            let icon = match tile {
                QuickTile::Airplane => primitive_quick::icons::PLANE,
                QuickTile::AutoRotate => primitive_quick::icons::ROTATE,
                QuickTile::DarkMode => primitive_quick::icons::MOON,
                QuickTile::Animation => primitive_quick::icons::ANIMATION,
                QuickTile::Ssh => primitive_quick::icons::SSH,
                QuickTile::AllSettings => primitive_quick::icons::SETTINGS,
                QuickTile::Power => POWER_ICON,
            };
            let _ = (cx, cy, bg);
            self.draw_icon(icon, ix, iy, size, fg);
            self.text(Text { text: label, x: slot.x as f32, y: (c.bottom() + 6) as f32, w: slot.w as f32, size: 24.0, bold: false, align: Align::Center, lines: 1, gray: BLACK });
        }
        for row in 0..2 {
            let (icon, label, level) =
                if row == 0 { (primitive_quick::icons::SUN, "Brightness", s.light) } else { (primitive_quick::icons::WARMTH, "Warmth", s.warmth) };
            let r = l.quick_row(row);
            self.draw_icon(icon, (r.x + 28) as f64, (r.y + 14) as f64, 40.0, BLACK);
            self.text(Text { text: label, x: (r.x + 92) as f32, y: (r.y + 18) as f32, w: 400.0, size: 30.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            let value = level.to_string();
            self.text(Text { text: &value, x: 0.0, y: (r.y + 22) as f32, w: (r.w - 28) as f32, size: 26.0, bold: false, align: Align::End, lines: 1, gray: 85 });
            let bar = l.quick_bar(row);
            let gap = 3;
            let step = (bar.w - gap * QUICK_LEVEL_MAX as i32) / (QUICK_LEVEL_MAX as i32 + 1);
            for n in 0..=QUICK_LEVEL_MAX as i32 {
                let seg = Rect::new(bar.x + n * (step + gap), bar.y, step, bar.h);
                if n <= level.min(QUICK_LEVEL_MAX) as i32 {
                    self.rounded(seg, 6.0, BLACK);
                } else {
                    self.rounded_frame(seg, 6.0, 2, 189, WHITE);
                }
            }
            self.round_button(l.quick_minus(row), false);
            self.round_button(l.quick_plus(row), true);
        }
        self.quick_system(l, s);
        let handle = l.quick_handle();
        self.stroke_icon("M6 15 L12 9 L18 15", (handle.x + handle.w / 2 - 16) as f64, (handle.y + 8) as f64, 32.0, 2.4, BLACK);
        self.text(Text { text: "Swipe up to close", x: handle.x as f32, y: (handle.y + 40) as f32, w: handle.w as f32, size: 24.0, bold: false, align: Align::Center, lines: 1, gray: BLACK });
    }

    /// A shared icon (`primitive-quick`) at (`x`, `y`), `size` pixels, in gray `v`: its stroked paths, then its filled ones.
    fn draw_icon(&mut self, icon: primitive_quick::Icon, x: f64, y: f64, size: f64, v: u8) {
        for d in icon.stroke {
            self.stroke_icon(d, x, y, size, 2.0, v);
        }
        for d in icon.fill {
            self.fill_svg(d, x, y, size / 24.0, v);
        }
    }

    /// The system section of quick settings: a header, then CPU, memory and storage, each with its icon, a bar of how
    /// full it is, and its value.
    fn quick_system(&mut self, l: &Layout, s: &Status) {
        let header = l.quick_system_header();
        self.text(Text { text: "System", x: 28.0 + 8.0, y: (header + 10) as f32, w: 400.0, size: 26.0, bold: true, align: Align::Start, lines: 1, gray: 60 });
        for (i, row) in s.system.rows().iter().enumerate() {
            let r = l.quick_stat_row(i);
            self.draw_icon(row.icon, 28.0, (r.y + 18) as f64, 40.0, BLACK);
            self.text(Text { text: row.label, x: 92.0, y: (r.y + 20) as f32, w: 200.0, size: 30.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            self.text(Text { text: &row.value, x: 0.0, y: (r.y + 22) as f32, w: (r.w - 28) as f32, size: 26.0, bold: false, align: Align::End, lines: 1, gray: 85 });
            let bar = Rect::new(280, r.y + 26, r.w - 280 - 340, 26);
            self.rounded_frame(bar, 13.0, 2, BLACK, WHITE);
            if let Some(f) = row.fraction {
                let inner = bar.inset(5);
                let w = (inner.w as f32 * f).round() as i32;
                if w > 0 {
                    self.rounded(Rect::new(inner.x, inner.y, w.max(inner.h), inner.h), inner.h as f64 / 2.0, BLACK);
                }
            }
        }
    }

    /// A round button with a bar (−) or a bar and a bar (+) in its middle, as Primitive Browser's level rows.
    fn round_button(&mut self, r: Rect, plus: bool) {
        self.rounded_frame(r, r.w as f64 / 2.0, 2, BLACK, WHITE);
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        self.fill(Rect::new(cx - 18, cy - 2, 36, 4), BLACK);
        if plus {
            self.fill(Rect::new(cx - 2, cy - 18, 4, 36), BLACK);
        }
    }


    /// The home screen: status bar, the page's icons with their names, and the page dots.
    fn home(&mut self, ui: &Ui, s: &Status, icon_blits: &mut Vec<(Rect, String)>) {
        let l = ui.layout;
        self.status_strip(&l, s, ui.running);
        let page: Vec<(usize, App)> = ui.page_apps().map(|(i, a)| (i, a.clone())).collect();
        for (slot, app) in page {
            self.app_icon(l.icon(slot), &app, icon_blits);
            let label = l.label(slot);
            self.text(Text {
                text: &app.name,
                x: label.x as f32,
                y: label.y as f32,
                w: label.w as f32,
                size: LABEL_SIZE,
                bold: false,
                align: Align::Center,
                lines: 1,
                gray: BLACK,
            });
        }
        self.home_footer(ui);
    }

    /// An app's icon in a rounded square: its picture (blitted after rendering), a glyph for the built-ins, or
    /// its initials.
    fn app_icon(&mut self, r: Rect, app: &App, icon_blits: &mut Vec<(Rect, String)>) {
        if !matches!(app.icon, IconSource::None) && self.icons.contains_key(&app.id) {
            icon_blits.push((r, app.id.clone()));
        } else if !self.glyph_icon(r, &app.launch) {
            self.monogram(r, app);
        }
    }

    fn icon_radius(r: Rect) -> f64 {
        (r.w as f32 * image::ICON_RADIUS) as f64
    }

    /// The built-ins' icons: white glyphs on black. Returns false for other apps.
    fn glyph_icon(&mut self, r: Rect, launch: &Launch) -> bool {
        let glyph: fn(&mut Renderer, f64, f64, f64) = match launch {
            Launch::Settings => Renderer::gear,
            Launch::KindleMode => Renderer::book,
            Launch::Extensions => Renderer::four_squares,
            _ => return false,
        };
        self.rounded(r, Self::icon_radius(r), BLACK);
        glyph(self, (r.x + r.w / 2) as f64, (r.y + r.h / 2) as f64, r.w as f64);
        true
    }

    /// Settings: a gear with eight teeth, centred at (`cx`, `cy`) on an icon of side `s`.
    fn gear(&mut self, cx: f64, cy: f64, s: f64) {
        self.gear_in(cx, cy, s, WHITE, BLACK);
    }

    /// The gear in colour `fg`, with its hole in `bg`.
    fn gear_in(&mut self, cx: f64, cy: f64, s: f64, fg: u8, bg: u8) {
        self.ctx.set_paint(gray(fg));
        for i in 0..8 {
            let turn = Affine::translate((cx, cy)) * Affine::rotate(i as f64 * std::f64::consts::FRAC_PI_4);
            self.ctx.set_transform(turn);
            let tooth = kurbo::RoundedRect::new(-0.065 * s, -0.36 * s, 0.065 * s, -0.2 * s, 0.02 * s).to_path(0.1);
            self.ctx.fill_path(&tooth);
        }
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.fill_path(&kurbo::Circle::new((cx, cy), 0.27 * s).to_path(0.1));
        self.ctx.set_paint(gray(bg));
        self.ctx.fill_path(&kurbo::Circle::new((cx, cy), 0.11 * s).to_path(0.1));
    }

    /// The Kindle UI: an open book, two pages with lines of text.
    fn book(&mut self, cx: f64, cy: f64, s: f64) {
        let (pw, ph, gap) = (0.27 * s, 0.44 * s, 0.035 * s);
        for side in [-1.0, 1.0] {
            let x0 = if side < 0.0 { cx - gap / 2.0 - pw } else { cx + gap / 2.0 };
            self.ctx.set_transform(Affine::IDENTITY);
            self.ctx.set_paint(gray(WHITE));
            self.ctx.fill_path(&kurbo::RoundedRect::new(x0, cy - ph / 2.0, x0 + pw, cy + ph / 2.0, 0.03 * s).to_path(0.1));
            self.ctx.set_paint(gray(BLACK));
            for line in 0..4 {
                let y = cy - ph / 2.0 + 0.09 * s + line as f64 * 0.085 * s;
                self.ctx.fill_rect(&kurbo::Rect::new(x0 + 0.05 * s, y, x0 + pw - 0.05 * s, y + 0.025 * s));
            }
        }
    }

    /// Extensions: four rounded squares.
    fn four_squares(&mut self, cx: f64, cy: f64, s: f64) {
        let (q, gap) = (0.2 * s, 0.07 * s);
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.set_paint(gray(WHITE));
        for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let x0 = if dx < 0.0 { cx - gap / 2.0 - q } else { cx + gap / 2.0 };
            let y0 = if dy < 0.0 { cy - gap / 2.0 - q } else { cy + gap / 2.0 };
            self.ctx.fill_path(&kurbo::RoundedRect::new(x0, y0, x0 + q, y0 + q, 0.05 * s).to_path(0.1));
        }
    }

    /// The bottom row: a notice, a battery warning, or the page dots.
    fn home_footer(&mut self, ui: &Ui) {
        let f = ui.layout.home_footer();
        let message = ui.notice.clone().or_else(|| ui.battery_message());
        if let Some(text) = message {
            let bold = ui.notice.is_none();
            self.text(Text { text: &text, x: 40.0, y: f.y as f32 + 18.0, w: f.w as f32 - 80.0, size: 30.0, bold, align: Align::Center, lines: 1, gray: BLACK });
            return;
        }
        let pages = ui.pages();
        if pages < 2 {
            return;
        }
        let (d, step) = (18, 44);
        let x0 = (f.w - (pages as i32 - 1) * step - d) / 2;
        for p in 0..pages {
            let dot = Rect::new(x0 + p as i32 * step, f.y + (f.h - d) / 2, d, d);
            if p == ui.page {
                self.rounded(dot, d as f64 / 2.0, BLACK);
            } else {
                self.rounded_frame(dot, d as f64 / 2.0, 3, BLACK, WHITE);
            }
        }
    }

    /// An app without an icon: its initials in a white rounded square with an outline.
    fn monogram(&mut self, r: Rect, app: &App) {
        let (bg, fg) = (WHITE, BLACK);
        self.rounded_frame(r, Self::icon_radius(r), 4, BLACK, bg);
        let initials: String = app
            .name
            .split_whitespace()
            .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
            .take(2)
            .flat_map(char::to_uppercase)
            .collect();
        let size = (r.w as f32 * 0.36).min(120.0);
        self.text(Text {
            text: if initials.is_empty() { "?" } else { &initials },
            x: r.x as f32,
            y: r.y as f32 + r.h as f32 / 2.0 - size * 0.68,
            w: r.w as f32,
            size,
            bold: true,
            align: Align::Center,
            lines: 1,
            gray: fg,
        });
    }

    fn button(&mut self, r: Rect, label: &str, filled: bool) {
        let (bg, fg) = if filled { (BLACK, WHITE) } else { (WHITE, BLACK) };
        self.rounded_frame(r, 16.0, 4, BLACK, bg);
        let size = 36.0;
        self.text(Text { text: label, x: r.x as f32, y: r.y as f32 + (r.h as f32 - size * 1.25) / 2.0, w: r.w as f32, size, bold: true, align: Align::Center, lines: 1, gray: fg });
    }

    /// The Extensions list: one row per item, "›" on submenus, Back (or Done at the top), and pages.
    fn extensions(&mut self, ui: &Ui) {
        let l = ui.layout;
        let title = l.title();
        let heading = ui.ext_title();
        let name_w = (l.done_button().x - 80) as f32;
        self.text(Text { text: &heading, x: 40.0, y: 38.0, w: name_w, size: 48.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), if ui.ext_depth() == 0 { "Done" } else { "Back" }, true);
        self.fill(Rect::new(0, title.bottom() - 4, title.w, 4), BLACK);
        let per = l.list_rows_per_page();
        let items: Vec<_> = ui.ext_items().iter().skip(ui.ext_page * per).take(per).cloned().collect();
        if items.is_empty() {
            self.text(Text { text: "Nothing here", x: 0.0, y: (title.bottom() + 80) as f32, w: title.w as f32, size: 38.0, bold: false, align: Align::Center, lines: 1, gray: BLACK });
        }
        for (i, item) in items.iter().enumerate() {
            let r = l.list_row(i);
            let chevron_w = if item.is_submenu() { 60.0 } else { 0.0 };
            self.text(Text { text: &item.name, x: r.x as f32 + 8.0, y: r.y as f32 + 36.0, w: r.w as f32 - 16.0 - chevron_w, size: 40.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            if item.is_submenu() {
                self.text(Text { text: "›", x: (r.right() - 60) as f32, y: r.y as f32 + 26.0, w: 52.0, size: 56.0, bold: true, align: Align::End, lines: 1, gray: BLACK });
            }
            self.fill(Rect::new(r.x, r.bottom() - 2, r.w, 2), BLACK);
        }
        let f = l.footer();
        self.fill(Rect::new(0, f.y, f.w, 3), BLACK);
        let pages = ui.ext_pages();
        if pages > 1 {
            let label = format!("{} / {}", ui.ext_page + 1, pages);
            self.text(Text { text: &label, x: 0.0, y: f.y as f32 + 22.0, w: f.w as f32, size: 38.0, bold: true, align: Align::Center, lines: 1, gray: BLACK });
            if ui.ext_page > 0 {
                self.text(Text { text: "‹  Previous", x: 40.0, y: f.y as f32 + 26.0, w: 240.0, size: 34.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            }
            if ui.ext_page + 1 < pages {
                self.text(Text { text: "Next  ›", x: f.w as f32 - 280.0, y: f.y as f32 + 26.0, w: 240.0, size: 34.0, bold: false, align: Align::End, lines: 1, gray: BLACK });
            }
        }
    }

    /// The app menu under its box: the home screen, with a ring around the icon being edited.
    fn app_menu(&mut self, ui: &Ui, s: &Status, icon_blits: &mut Vec<(Rect, String)>) {
        self.home(ui, s, icon_blits);
        let Some(app) = ui.menu_app().cloned() else { return };
        let anchor = ui.menu_anchor();
        let ring = anchor.inset(-10);
        self.rounded_frame(ring, Self::icon_radius(ring), 4, BLACK, WHITE);
        self.app_icon(anchor, &app, icon_blits);
    }

    /// The app menu's box beside the icon: the app's name, then one row per action (a pop-up, drawn last).
    fn app_menu_box(&mut self, ui: &Ui) {
        let l = ui.layout;
        let Some(app) = ui.menu_app().cloned() else { return };
        let (anchor, actions) = (ui.menu_anchor(), ui.menu_actions());
        let b = l.menu_box(anchor, actions.len());
        self.rounded_frame(b, POPUP_RADIUS as f64, 4, BLACK, WHITE);
        let name_row = Rect::new(b.x, b.y, b.w, l.menu_row(anchor, actions.len(), 0).y - b.y);
        self.text(Text { text: &app.name, x: (b.x + 36) as f32, y: (name_row.y + 28) as f32, w: (b.w - 72) as f32, size: 36.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        for (i, action) in actions.iter().enumerate() {
            let r = l.menu_row(anchor, actions.len(), i);
            self.fill(Rect::new(b.x + 4, r.y, b.w - 8, 2), BLACK);
            let label = match action {
                Button::MenuEarlier => "Move earlier".to_string(),
                Button::MenuLater => "Move later".to_string(),
                Button::MenuIcon => match &app.icon {
                    IconSource::File(path) => format!("Change icon ({})", path.rsplit('/').next().unwrap_or(path)),
                    _ => "Change icon".to_string(),
                },
                Button::MenuHide => "Hide".to_string(),
                _ => String::new(),
            };
            self.text(Text { text: &label, x: (r.x + 36) as f32, y: (r.y + 28) as f32, w: (r.w - 72) as f32, size: 36.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
        }
    }

    /// The network screen: the network's name, its details as rows, Connect, and Forget for a saved network.
    fn network_detail(&mut self, ui: &Ui) {
        let l = ui.layout;
        let title = l.title();
        let Some(net) = ui.wifi_target.clone() else { return };
        let name_w = (l.done_button().x - 80) as f32;
        self.text(Text { text: &net.name, x: 40.0, y: 36.0, w: name_w, size: 56.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), "Back", true);
        self.fill(Rect::new(0, title.bottom() - 4, title.w, 4), BLACK);
        let card = Rect::new(MARGIN_X, l.title().bottom() + 40, self.width as i32 - 2 * MARGIN_X, 3 * 96);
        self.card(card);
        let rows = [
            ("Security", if net.secured { "Secured" } else { "Open" }.to_string()),
            ("Signal", format!("{} of {}", net.level, net.max)),
            ("Saved", if net.known { "Yes" } else { "No" }.to_string()),
        ];
        for (i, (key, value)) in rows.iter().enumerate() {
            let r = Rect::new(card.x, card.y + i as i32 * 96, card.w, 96);
            self.text(Text { text: key, x: (r.x + 32) as f32, y: (r.y + 32) as f32, w: 400.0, size: 32.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            self.text(Text { text: value, x: (r.x + 32) as f32, y: (r.y + 32) as f32, w: (r.w - 64) as f32, size: 32.0, bold: true, align: Align::End, lines: 1, gray: BLACK });
            if i + 1 < rows.len() {
                self.hairline(r.bottom() - 1, r.x + 32, r.right() - 32);
            }
        }
        let connect = if net.secured && !net.known { "Enter password" } else { "Connect" };
        self.button(l.net_connect(), connect, true);
        if net.known {
            self.button(l.net_forget(), "Forget this network", false);
        }
        if let Some(notice) = ui.notice.clone() {
            self.text(Text { text: &notice, x: MARGIN_X as f32, y: (l.height - 120) as f32, w: (l.width - 2 * MARGIN_X) as f32, size: 30.0, bold: false, align: Align::Start, lines: 2, gray: BLACK });
        }
    }

    /// The password screen: the network's name, the password field with its caret, Show password, the keyboard (its
    /// Go key joins), and the notice under it.
    fn password_screen(&mut self, ui: &Ui) {
        let l = ui.layout;
        let title = l.title();
        self.text(Text { text: "Password", x: 40.0, y: 36.0, w: 700.0, size: 56.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), "Back", true);
        self.fill(Rect::new(0, title.bottom() - 4, title.w, 4), BLACK);
        let name = ui.wifi_target.as_ref().map(|n| n.name.clone()).unwrap_or_default();
        self.text(Text { text: &name, x: MARGIN_X as f32, y: (l.title().bottom() + 24) as f32, w: (l.width - 2 * MARGIN_X) as f32, size: 30.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
        let field = l.password_field();
        self.rounded_frame(field, 20.0, 3, BLACK, WHITE);
        let typed = &ui.password.text;
        let shown = if ui.show_password { typed.iter().collect::<String>() } else { "•".repeat(typed.len()) };
        let (text, gray) = if typed.is_empty() { ("Type the password".to_string(), 120) } else { (shown, BLACK) };
        self.text(Text { text: &text, x: (field.x + 28) as f32, y: (field.y + 30) as f32, w: (field.w - 56) as f32, size: 40.0, bold: false, align: Align::Start, lines: 1, gray });
        // The caret: a bar at the caret's place in the text (on the typed characters).
        let caret_chars = if ui.show_password { ui.password.text[..ui.password.caret].iter().collect::<String>() } else { "•".repeat(ui.password.caret) };
        let caret_x = field.x + 28 + self.text_width(&caret_chars, 40.0, false) as i32;
        if !typed.is_empty() {
            self.fill(Rect::new(caret_x, field.y + 22, 3, 56), BLACK);
        }
        if ui.wifi_hidden {
            self.text(Text { text: "Leave it empty for an open network.", x: MARGIN_X as f32, y: (l.password_notice().y - 60) as f32, w: (l.width - 2 * MARGIN_X) as f32, size: 26.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
        }
        let show = l.password_show();
        self.text(Text { text: "Show password", x: show.x as f32 + 8.0, y: (show.y + 20) as f32, w: 600.0, size: 32.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.switch(switch_rect(show), ui.show_password);
        // The keyboard's area at the bottom, with the line above it, as in Primitive Browser.
        let area = l.keyboard_area();
        self.fill(Rect::new(area.x, area.y, area.w, primitive_keyboard::style::TOP_LINE as i32), primitive_keyboard::style::TOP_LINE_GRAY);
        for (key, r) in l.keyboard_keys(ui.password.layer) {
            let pressed = ui.pressed_key == Some(r);
            self.key(&key, r, pressed);
        }
        if let Some(notice) = ui.notice.clone() {
            let n = l.password_notice();
            self.text(Text { text: &notice, x: n.x as f32, y: n.y as f32, w: n.w as f32, size: 28.0, bold: false, align: Align::Center, lines: 2, gray: BLACK });
        }
    }

    /// The name screen for a network that is not in the list: the name field with its caret, a note, and the keyboard
    /// whose Go key says Next.
    fn network_name_screen(&mut self, ui: &Ui) {
        let l = ui.layout;
        let title = l.title();
        self.text(Text { text: "Other network", x: 40.0, y: 36.0, w: 800.0, size: 56.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), "Back", true);
        self.fill(Rect::new(0, title.bottom() - 4, title.w, 4), BLACK);
        self.text(Text { text: "Network name", x: MARGIN_X as f32, y: (title.bottom() + 24) as f32, w: 600.0, size: 30.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
        let field = l.password_field();
        self.rounded_frame(field, 20.0, 3, BLACK, WHITE);
        let typed: String = ui.network_name.text.iter().collect();
        let (text, gray) = if typed.is_empty() { ("Type the name, as it is written".to_string(), 120) } else { (typed.clone(), BLACK) };
        self.text(Text { text: &text, x: (field.x + 28) as f32, y: (field.y + 30) as f32, w: (field.w - 56) as f32, size: 40.0, bold: false, align: Align::Start, lines: 1, gray });
        if !typed.is_empty() {
            let before: String = ui.network_name.text[..ui.network_name.caret].iter().collect();
            let x = field.x + 28 + self.text_width(&before, 40.0, false) as i32;
            self.fill(Rect::new(x, field.y + 22, 3, 56), BLACK);
        }
        let note = "For a network that does not show in the list. Next asks for its password.";
        self.text(Text { text: note, x: MARGIN_X as f32, y: (field.bottom() + 24) as f32, w: (l.width - 2 * MARGIN_X) as f32, size: 26.0, bold: false, align: Align::Start, lines: 2, gray: 85 });
        let area = l.keyboard_area();
        self.fill(Rect::new(area.x, area.y, area.w, primitive_keyboard::style::TOP_LINE as i32), primitive_keyboard::style::TOP_LINE_GRAY);
        for (key, r) in l.keyboard_keys_with(ui.network_name.layer, "Next") {
            let pressed = ui.pressed_key == Some(r);
            self.key(&key, r, pressed);
        }
        if let Some(notice) = ui.notice.clone() {
            let n = l.password_notice();
            self.text(Text { text: &notice, x: n.x as f32, y: n.y as f32, w: n.w as f32, size: 28.0, bold: false, align: Align::Center, lines: 2, gray: BLACK });
        }
    }

    /// The SSH password screen: the password field with its caret, the rule, and the keyboard whose Go key says Set.
    fn ssh_password_screen(&mut self, ui: &Ui) {
        let l = ui.layout;
        let title = l.title();
        self.text(Text { text: "SSH password", x: 40.0, y: 36.0, w: 800.0, size: 56.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), "Back", true);
        self.fill(Rect::new(0, title.bottom() - 4, title.w, 4), BLACK);
        self.text(Text { text: "Root password for SSH", x: MARGIN_X as f32, y: (title.bottom() + 24) as f32, w: 600.0, size: 30.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
        let field = l.password_field();
        self.rounded_frame(field, 20.0, 3, BLACK, WHITE);
        let typed: String = ui.ssh_password.text.iter().collect();
        let (text, gray) = if typed.is_empty() { ("Type the password".to_string(), 120) } else { (typed.clone(), BLACK) };
        self.text(Text { text: &text, x: (field.x + 28) as f32, y: (field.y + 30) as f32, w: (field.w - 56) as f32, size: 40.0, bold: false, align: Align::Start, lines: 1, gray });
        if !typed.is_empty() {
            let before: String = ui.ssh_password.text[..ui.ssh_password.caret].iter().collect();
            let x = field.x + 28 + self.text_width(&before, 40.0, false) as i32;
            self.fill(Rect::new(x, field.y + 22, 3, 56), BLACK);
        }
        let note = "6 to 64 letters, digits or - _ . @ # % + = !, no spaces. Connect with the address on this page.";
        self.text(Text { text: note, x: MARGIN_X as f32, y: (field.bottom() + 24) as f32, w: (l.width - 2 * MARGIN_X) as f32, size: 26.0, bold: false, align: Align::Start, lines: 2, gray: 85 });
        let area = l.keyboard_area();
        self.fill(Rect::new(area.x, area.y, area.w, primitive_keyboard::style::TOP_LINE as i32), primitive_keyboard::style::TOP_LINE_GRAY);
        for (key, r) in l.keyboard_keys_with(ui.ssh_password.layer, "Set") {
            let pressed = ui.pressed_key == Some(r);
            self.key(&key, r, pressed);
        }
        if let Some(notice) = ui.notice.clone() {
            let n = l.password_notice();
            self.text(Text { text: &notice, x: n.x as f32, y: n.y as f32, w: n.w as f32, size: 28.0, bold: false, align: Align::Center, lines: 2, gray: BLACK });
        }
    }

    /// One key of the keyboard, in the shared look (`primitive-keyboard::style`, the same as Primitive Browser's
    /// keys): a light key with a thin grey outline, or an inked key with white text when it is on (Shift, Go).
    /// One key, in the keyboard's shared look: on, or pressed, takes the ink look (see `primitive_keyboard::look`).
    fn key(&mut self, key: &primitive_keyboard::KeyDef, r: Rect, pressed: bool) {
        use primitive_keyboard::style::*;
        let look = primitive_keyboard::look(key, pressed);
        self.rounded_frame(r, RADIUS as f64, BORDER as i32, look.line, look.fill);
        let size = label_size(&key.label) as f32;
        self.text(Text { text: &key.label, x: r.x as f32, y: r.y as f32 + r.h as f32 / 2.0 - size * 0.7, w: r.w as f32, size, bold: false, align: Align::Center, lines: 1, gray: look.ink });
    }

    /// Settings: the list of pages in groups. Each group is a header and one card; each page row shows its title, its
    /// current value under it, and a chevron. Done returns to the grid.
    fn settings_home(&mut self, ui: &Ui, s: &Status) {
        let l = ui.layout;
        self.text(Text { text: "Settings", x: 40.0, y: 36.0, w: 700.0, size: 64.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), "Done", true);
        for (header, card, rows) in l.home_sections() {
            if !header.is_empty() {
                self.section_header(card.x, card.y - 44, header);
            }
            self.card(card);
            let count = rows.len();
            for (i, (page, r)) in rows.into_iter().enumerate() {
                let value = page_value(page, ui, s);
                self.text(Text { text: page.title(), x: (r.x + 32) as f32, y: (r.y + 22) as f32, w: (r.w - 120) as f32, size: 38.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
                self.text(Text { text: &value, x: (r.x + 32) as f32, y: (r.y + 72) as f32, w: (r.w - 120) as f32, size: 28.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
                self.text(Text { text: "›", x: (r.right() - 80) as f32, y: (r.y + 30) as f32, w: 50.0, size: 52.0, bold: true, align: Align::End, lines: 1, gray: BLACK });
                if i + 1 < count {
                    self.hairline(r.bottom() - 1, card.x + 32, card.right() - 32);
                }
            }
        }
    }

    /// One page of Settings: its title and Back, then its sections (header, card of rows, footer). The Wi-Fi page
    /// adds the current network and the scan list; About is key and value text.
    fn settings_page(&mut self, ui: &Ui, s: &Status, page: kl_core::layout::Page) {
        let l = ui.layout;
        let title = l.title();
        self.text(Text { text: page.title(), x: 40.0, y: 36.0, w: 900.0, size: 56.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), "Back", true);
        self.fill(Rect::new(0, title.bottom() - 4, title.w, 4), BLACK);
        if page == kl_core::layout::Page::About {
            self.about_card(ui, s);
            return;
        }
        for section in l.page_sections(page) {
            self.section_header(section.card.x, section.header_y, section.header);
            self.card(section.card);
            let count = section.rows.len();
            for (i, row) in section.rows.iter().enumerate() {
                self.setting_row(ui, s, row);
                if i + 1 < count {
                    self.hairline(row.rect.bottom() - 1, row.rect.x + 32, row.rect.right() - 32);
                }
            }
            if !section.footer.is_empty() {
                self.text(Text { text: section.footer, x: section.card.x as f32 + 8.0, y: section.footer_y as f32, w: section.card.w as f32 - 16.0, size: 24.0, bold: false, align: Align::Start, lines: 2, gray: 85 });
            }
        }
        if page == kl_core::layout::Page::Wifi {
            self.wifi_list(ui, s);
        }
        // A result of an action on this page (a Wi-Fi join, an SSH check or password): shown at the bottom on every page.
        if let Some(notice) = ui.notice.clone() {
            let width = (l.width - 2 * MARGIN_X) as f32;
            self.text(Text { text: &notice, x: MARGIN_X as f32, y: (l.height - 84) as f32, w: width, size: 28.0, bold: true, align: Align::Start, lines: 2, gray: BLACK });
        }
    }

    /// One setting row: its label and its value under it, and its control on the right: a switch for on/off, round
    /// buttons for a level, a pill for "Show all", a chevron for something that opens.
    fn setting_row(&mut self, ui: &Ui, s: &Status, row: &kl_core::layout::SettingsRow) {
        let r = row.rect;
        let value = row_value(row.row, ui, s);
        let (label_w, text_x) = (r.w as f32 - 360.0, (r.x + 32) as f32);
        self.text(Text { text: row.row.label(), x: text_x, y: (r.y + 20) as f32, w: label_w, size: 36.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.text(Text { text: &value, x: text_x, y: (r.y + 66) as f32, w: label_w, size: 27.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
        for &(b, rect) in &row.buttons {
            match b {
                Button::LightDown | Button::WarmDown => self.round_button(rect, false),
                Button::LightUp | Button::WarmUp => self.round_button(rect, true),
                Button::RotationAuto => self.switch(rect, s.auto_rotation),
                Button::ShowHidden => self.button(rect, "Show all", false),
                Button::KindleUi => self.chevron(r),
                Button::Restart => {}
                Button::Rotation => {}
                Button::Wifi => self.switch(switch_rect(r), s.wifi_on),
                Button::DarkMode => self.switch(switch_rect(r), s.dark_mode),
                Button::Animation => self.switch(switch_rect(r), s.animation),
                Button::Ssh => self.switch(switch_rect(r), s.ssh),
                Button::Autostart => self.switch(switch_rect(r), s.autostart),
                Button::SleepScreen => self.switch(switch_rect(r), s.sleep_launcher),
                _ => {}
            }
        }
    }

    /// The Wi-Fi page's network sections: the current network, then the scan list with its Scan button.
    fn wifi_list(&mut self, ui: &Ui, s: &Status) {
        let l = ui.layout;
        let area = l.wifi_area();
        self.section_header(area.current.x, area.current_header, "Current network");
        self.card(area.current);
        match &ui.wifi.current {
            Some(net) => {
                let detail = format!("Connected · {} of {} signal{}", net.level, net.max, if net.secured { " · secured" } else { "" });
                self.text(Text { text: &net.name, x: (area.current.x + 32) as f32, y: (area.current.y + 20) as f32, w: (area.current.w - 220) as f32, size: 36.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
                self.text(Text { text: &detail, x: (area.current.x + 32) as f32, y: (area.current.y + 66) as f32, w: (area.current.w - 220) as f32, size: 27.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
                let lit = kl_core::wifi::arcs(net.level, net.max);
                self.wifi_icon((area.current.right() - 110) as f64, (area.current.y + 28) as f64, 56.0, Some(lit));
                self.text(Text { text: "›", x: (area.current.right() - 52) as f32, y: (area.current.y + 30) as f32, w: 40.0, size: 52.0, bold: true, align: Align::End, lines: 1, gray: BLACK });
            }
            None => {
                let text = if s.wifi_on { "Not connected" } else { "Wi-Fi is off" };
                self.text(Text { text, x: (area.current.x + 32) as f32, y: (area.current.y + 40) as f32, w: area.current.w as f32 - 64.0, size: 36.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
            }
        }
        let total = ui.wifi_networks().len();
        let heading = if total == 0 { "Networks".to_string() } else { format!("Networks · {total}") };
        self.section_header(area.current.x, area.networks_header, &heading);
        self.button(area.scan, if ui.wifi_scanning { "Scanning…" } else { "Scan" }, false);
        self.button(l.wifi_other(), "Other…", false);
        self.card(area.list);
        let networks = ui.wifi_networks();
        if networks.is_empty() {
            let text = if !s.wifi_on {
                "Wi-Fi is off. Turn it on to see the networks around."
            } else if ui.wifi_scanning {
                "Looking for networks…"
            } else {
                "No networks found. Tap Scan to look again."
            };
            self.text(Text { text, x: (area.list.x + 32) as f32, y: (area.list.y + 36) as f32, w: area.list.w as f32 - 64.0, size: 30.0, bold: false, align: Align::Start, lines: 2, gray: 85 });
            return;
        }
        for i in 0..area.visible {
            let Some(net) = networks.get(ui.wifi_scroll + i) else { break };
            let r = l.network_row(i);
            let name_w = (r.w - 260) as f32;
            self.text(Text { text: &net.name, x: (r.x + 32) as f32, y: (r.y + 18) as f32, w: name_w, size: 34.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            if net.known {
                self.text(Text { text: "Saved", x: (r.x + 32) as f32, y: (r.y + 60) as f32, w: name_w, size: 24.0, bold: false, align: Align::Start, lines: 1, gray: 85 });
            }
            if net.secured {
                self.lock_icon((r.right() - 150) as f64, (r.y + 34) as f64);
            }
            self.wifi_icon((r.right() - 100) as f64, (r.y + 26) as f64, 52.0, Some(kl_core::wifi::arcs(net.level, net.max)));
            if i + 1 < area.visible && ui.wifi_scroll + i + 1 < networks.len() {
                self.hairline(r.bottom() - 1, r.x + 32, r.right() - 32);
            }
        }
        // A scroll bar when the list is longer than the window: the thumb is the share of the list in view.
        if networks.len() > area.visible {
            let track = Rect::new(area.list.right() - 16, area.list.y + 16, 6, area.list.h - 32);
            self.fill(track, 200);
            let len = networks.len() as i32;
            let shown = area.visible as i32;
            let thumb_h = (track.h * shown / len).max(48);
            let max_scroll = (len - shown).max(1);
            let thumb_y = track.y + (track.h - thumb_h) * ui.wifi_scroll as i32 / max_scroll;
            self.rounded(Rect::new(track.x, thumb_y, track.w, thumb_h), 3.0, BLACK);
        }
    }

    /// The About page's card: the launcher's version, apps, memory, storage and Wi-Fi address, as key and value rows.
    fn about_card(&mut self, ui: &Ui, s: &Status) {
        let l = ui.layout;
        let top = l.title().bottom() + 24;
        self.section_header(MARGIN_X, top, "Launcher");
        let card = Rect::new(MARGIN_X, top + 56, self.width as i32 - 2 * MARGIN_X, 5 * 96);
        self.card(card);
        let rows = [
            ("Version", kl_core::VERSION.to_string()),
            ("Apps", ui.apps().len().to_string()),
            ("Memory", if s.memory.is_empty() { "unknown".into() } else { s.memory.clone() }),
            ("Storage", s.storage.clone()),
            ("Wi-Fi address", s.ip.clone()),
        ];
        for (i, (key, value)) in rows.iter().enumerate() {
            let r = Rect::new(card.x, card.y + i as i32 * 96, card.w, 96);
            self.text(Text { text: key, x: (r.x + 32) as f32, y: (r.y + 32) as f32, w: 400.0, size: 32.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            self.text(Text { text: value, x: (r.x + 32) as f32, y: (r.y + 32) as f32, w: (r.w - 64) as f32, size: 32.0, bold: true, align: Align::End, lines: 1, gray: BLACK });
            if i + 1 < rows.len() {
                self.hairline(r.bottom() - 1, r.x + 32, r.right() - 32);
            }
        }
        self.text(Text { text: "From the Kindle UI, open “Launcher” in the library to come back.", x: (card.x + 8) as f32, y: (card.bottom() + 16) as f32, w: (card.w - 16) as f32, size: 24.0, bold: false, align: Align::Start, lines: 2, gray: 85 });
    }

    /// A section header above a card: small, bold, at `x`, `y`.
    fn section_header(&mut self, x: i32, y: i32, text: &str) {
        self.text(Text { text, x: (x + 8) as f32, y: (y + 8) as f32, w: 900.0, size: 26.0, bold: true, align: Align::Start, lines: 1, gray: 60 });
    }

    /// A card: a rounded outline with white inside (the grouped list's container).
    fn card(&mut self, r: Rect) {
        self.rounded_frame(r, 22.0, 2, BLACK, WHITE);
    }

    /// A hairline between rows, from `x0` to `x1` at `y`.
    fn hairline(&mut self, y: i32, x0: i32, x1: i32) {
        self.fill(Rect::new(x0, y, x1 - x0, 2), 200);
    }

    /// A switch: a pill with a knob, black and knob-right when on; outlined and knob-left when off.
    fn switch(&mut self, r: Rect, on: bool) {
        let radius = r.h as f64 / 2.0;
        if on {
            self.rounded(r, radius, BLACK);
            self.rounded(Rect::new(r.right() - r.h + 6, r.y + 6, r.h - 12, r.h - 12), radius - 6.0, WHITE);
        } else {
            self.rounded_frame(r, radius, 3, BLACK, WHITE);
            self.rounded(Rect::new(r.x + 6, r.y + 6, r.h - 12, r.h - 12), radius - 6.0, BLACK);
        }
    }

    /// A disclosure chevron at the right of a row.
    fn chevron(&mut self, r: Rect) {
        self.text(Text { text: "›", x: (r.right() - 80) as f32, y: (r.y + 30) as f32, w: 50.0, size: 52.0, bold: true, align: Align::End, lines: 1, gray: BLACK });
    }

    /// A padlock (a secured network), at (`x`, `y`): a shackle over a body.
    fn lock_icon(&mut self, x: f64, y: f64) {
        self.rounded_frame(Rect::new(x as i32 + 5, y as i32, 14, 16), 7.0, 3, BLACK, WHITE);
        self.rounded(Rect::new(x as i32, y as i32 + 14, 24, 20), 4.0, BLACK);
    }

    /// The power menu: a box over the grid, with Cancel, Restart and Power off.
    fn power_menu_box(&mut self, l: &Layout) {
        let b = l.power_box();
        self.rounded_frame(b, POPUP_RADIUS as f64, 6, BLACK, WHITE);
        let inner = b.inset(48);
        self.text(Text { text: "Power", x: inner.x as f32, y: inner.y as f32, w: inner.w as f32, size: 50.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.text(Text { text: "Restart starts the Kindle again. Power off turns it off until you press its power button.", x: inner.x as f32, y: (inner.y + 80) as f32, w: inner.w as f32, size: 30.0, bold: false, align: Align::Start, lines: 3, gray: 85 });
        for (button, r) in l.power_buttons() {
            let (label, primary) = match button {
                Button::PowerCancel => ("Cancel", false),
                Button::PowerRestart => ("Restart", true),
                _ => ("Power off", true),
            };
            self.button(r, label, primary);
        }
    }

    fn confirm(&mut self, l: &Layout) {
        let b = l.confirm_box();
        self.rounded_frame(b, POPUP_RADIUS as f64, 6, BLACK, WHITE);
        let inner = b.inset(48);
        self.text(Text { text: "Open the Kindle UI?", x: inner.x as f32, y: inner.y as f32, w: inner.w as f32, size: 50.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.text(Text {
            text: "The launcher closes and the stock Kindle UI starts, which takes about a minute. To come back, open “Launcher” in the Kindle library.",
            x: inner.x as f32,
            y: inner.y as f32 + 100.0,
            w: inner.w as f32,
            size: 36.0,
            bold: false,
            align: Align::Start,
            lines: 5,
            gray: BLACK,
        });
        let [(_, cancel), (_, open)] = l.confirm_buttons();
        self.button(cancel, "Cancel", false);
        self.button(open, "Open Kindle UI", true);
    }

    /// A full-screen message (e.g. "Starting the Kindle UI"): a title and a body, centred.
    pub fn message(&mut self, title: &str, body: &str) -> Vec<u8> {
        self.ctx.reset();
        let (w, h) = (self.width as i32, self.ctx.height() as i32);
        self.fill(Rect::new(0, 0, w, h), WHITE);
        self.text(Text { text: title, x: 60.0, y: h as f32 / 2.0 - 140.0, w: w as f32 - 120.0, size: 64.0, bold: true, align: Align::Center, lines: 2, gray: BLACK });
        self.text(Text { text: body, x: 80.0, y: h as f32 / 2.0, w: w as f32 - 160.0, size: 38.0, bold: false, align: Align::Center, lines: 4, gray: BLACK });
        self.finish_gray()
    }

    /// The launcher's own library icon (600 × 920): a 3 × 3 grid of rounded-square tiles, as on the home screen,
    /// with `logo` (the Primitive logo) in the centre tile, and the name.
    pub fn launcher_icon(&mut self, logo: Option<&GrayImage>) -> Vec<u8> {
        self.ctx.reset();
        let (w, h) = (self.width as i32, self.ctx.height() as i32);
        self.fill(Rect::new(0, 0, w, h), WHITE);
        self.rounded_frame(Rect::new(30, 30, w - 60, h - 60), 40.0, 10, BLACK, WHITE);
        let (cell, gap) = (140, 26);
        let (gx, gy) = ((w - 3 * cell - 2 * gap) / 2, 110);
        let tile = |i: i32| Rect::new(gx + (i % 3) * (cell + gap), gy + (i / 3) * (cell + gap), cell, cell);
        for i in 0..9 {
            let radius = cell as f64 * image::ICON_RADIUS as f64;
            if i == 4 && logo.is_none() {
                self.rounded(tile(i), radius, BLACK);
            } else {
                self.rounded_frame(tile(i), radius, 8, BLACK, WHITE);
            }
        }
        self.text(Text { text: "Launcher", x: 0.0, y: h as f32 - 220.0, w: w as f32, size: 84.0, bold: true, align: Align::Center, lines: 1, gray: BLACK });
        let mut out = self.finish_gray();
        if let Some(logo) = logo {
            let inner = tile(4).inset(18);
            let mut fitted = image::fit(logo, inner.w as u32, inner.h as u32);
            image::dither16(&mut fitted);
            blit(&mut out, w as usize, inner, &fitted);
        }
        out
    }

    /// The sleep screen when there is no picture: only used for a battery warning (otherwise the driver draws
    /// nothing and the Kindle's screensaver, or the last screen, stays).
    fn sleep(&mut self, l: &Layout, warning: Option<String>) {
        let s = l.screen();
        self.frame(s.inset(40), 6, BLACK);
        let message = warning.unwrap_or_default();
        self.text(Text { text: "Connect the charger", x: 0.0, y: s.h as f32 / 2.0 - 120.0, w: s.w as f32, size: 80.0, bold: true, align: Align::Center, lines: 1, gray: BLACK });
        self.text(Text { text: &message, x: 60.0, y: s.h as f32 / 2.0 + 20.0, w: s.w as f32 - 120.0, size: 40.0, bold: false, align: Align::Center, lines: 2, gray: BLACK });
    }
}

/// The value a Settings list row shows for its page.
fn page_value(page: kl_core::layout::Page, ui: &Ui, s: &Status) -> String {
    use kl_core::layout::Page;
    let onoff = |on: bool| if on { "on" } else { "off" };
    match page {
        Page::Wifi => wifi_word(s),
        Page::Display => format!("Frontlight {} · Dark mode {} · Animation {}", s.light, onoff(s.dark_mode), onoff(s.animation)),
        Page::Sleep => if s.sleep_launcher { "The launcher's own".into() } else { "The Kindle's".into() },
        Page::Apps => format!("{} hidden", ui.hidden_count()),
        Page::Connection => format!("SSH {} · Public DNS {}", onoff(s.ssh), onoff(s.public_dns)),
        Page::Device => format!("Start with the Kindle {}", onoff(s.autostart)),
        Page::About => format!("Version {}", kl_core::VERSION),
    }
}

/// The Wi-Fi state as a word: Off, On, or the network state.
fn wifi_word(s: &Status) -> String {
    match (s.wifi_on, s.wifi_state.as_str()) {
        (false, _) => "Off".into(),
        (true, "") => "On".into(),
        (true, state) => capitalise(state),
    }
}

/// The value a setting row shows.
fn row_value(row: Row, ui: &Ui, s: &Status) -> String {
    match row {
        Row::Frontlight => format!("{} / 24", s.light),
        Row::Warmth => format!("{} / 24", s.warmth),
        Row::Wifi => wifi_word(s),
        Row::DarkMode => if s.dark_mode { "On: white on black".into() } else { "Off".into() },
        Row::Animation => if s.animation { "On: a zoom when an app opens".into() } else { "Off".into() },
        Row::Rotation => match (s.auto_rotation, s.rotation) {
            (true, Rotation::Upright) => "Automatic: upright".into(),
            (true, Rotation::UpsideDown) => "Automatic: upside down".into(),
            (false, Rotation::Upright) => "Upright (fixed)".into(),
            (false, Rotation::UpsideDown) => "Upside down (fixed)".into(),
        },
        Row::Ssh => if s.ssh { "Running".into() } else { "Off".into() },
        Row::SshPassword => if s.ssh_password_set { "Set".into() } else { "Not set".into() },
        Row::SshAddress => if s.ssh && !s.ssh_address.is_empty() { s.ssh_address.clone() } else { "Off".into() },
        Row::SshCheck => "Tap to check".into(),
        Row::PublicDns => if s.public_dns { "On: Cloudflare and Google".into() } else { "Off".into() },
        Row::Autostart => if s.autostart { "On".into() } else { "Off".into() },
        Row::SleepScreen => if s.sleep_launcher { "The launcher's own".into() } else { "The Kindle's".into() },
        Row::Hidden => match ui.hidden_count() {
            0 => "None. Long-press to hide".into(),
            n => format!("{n} hidden"),
        },
        Row::KindleUi => "Stock Kindle UI".into(),
        Row::Restart => "Tap to restart".into(),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Copy a tile into the screen buffer at `r` (clipped to the tile and screen).
fn blit(out: &mut [u8], width: usize, r: Rect, tile: &GrayImage) {
    let height = out.len() / width;
    for ty in 0..(tile.height as usize).min(r.h.max(0) as usize) {
        let y = r.y as usize + ty;
        if y >= height {
            break;
        }
        let n = (tile.width as usize).min(r.w.max(0) as usize).min(width.saturating_sub(r.x as usize));
        let src = &tile.pixels[ty * tile.width as usize..ty * tile.width as usize + n];
        out[y * width + r.x as usize..y * width + r.x as usize + n].copy_from_slice(src);
    }
}

/// Turn a screen buffer upside down (rotation by 180°), in place.
pub fn turn_half(buf: &mut [u8]) {
    buf.reverse();
}

/// The pixels to show for `rotation`, given the framebuffer's own rotation (`fb_turn`, 0 or 2).
pub fn for_panel(mut buf: Vec<u8>, rotation: Rotation, fb_turn: u32) -> Vec<u8> {
    if (rotation.turn() + fb_turn) % 4 == 2 {
        turn_half(&mut buf);
    }
    buf
}

/// The rows that differ between two frames: `(first, count)`, or `None` when equal.
pub fn changed_rows(old: &[u8], new: &[u8], width: usize) -> Option<(usize, usize)> {
    if old.len() != new.len() {
        return Some((0, new.len() / width));
    }
    let rows = new.len() / width;
    let first = (0..rows).find(|&y| old[y * width..(y + 1) * width] != new[y * width..(y + 1) * width])?;
    let last = (0..rows).rev().find(|&y| old[y * width..(y + 1) * width] != new[y * width..(y + 1) * width])?;
    Some((first, last - first + 1))
}

/// The box around every changed pixel: `(x, y, w, h)`, with `x` and `w` on 8-pixel columns (the e-ink
/// driver's update regions), or `None` when the frames are equal. A frame of another size changes everything.
pub fn changed_rect(old: &[u8], new: &[u8], width: usize) -> Option<(usize, usize, usize, usize)> {
    let rows = new.len() / width;
    if old.len() != new.len() {
        return Some((0, 0, width, rows));
    }
    let (y, h) = changed_rows(old, new, width)?;
    let (mut left, mut right) = (width, 0);
    for row in y..y + h {
        let (o, n) = (&old[row * width..(row + 1) * width], &new[row * width..(row + 1) * width]);
        if let Some(first) = (0..left).find(|&x| o[x] != n[x]) {
            left = first;
        }
        if let Some(last) = (right..width).rev().find(|&x| o[x] != n[x]) {
            right = right.max(last + 1);
        }
    }
    let x = left / 8 * 8;
    let end = right.div_ceil(8) * 8;
    Some((x, y, end.min(width) - x, h))
}

/// Whether a band of pixels is pure black and white (the non-flashing DU waveform can draw it).
pub fn is_black_and_white(pixels: &[u8]) -> bool {
    pixels.iter().all(|&p| p == 0 || p == 255)
}


/// A sleep screen picture from a PNG file: fitted to `w` × `h` (centred, white around it) and dithered to the
/// panel's 16 grays.
pub fn sleep_image(path: &std::path::Path, w: u32, h: u32) -> Result<GrayImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    image::picture_tile(&bytes, w, h)
}

/// The dithered rounded-square icon tile for `app`, of side `w` (`h` is the same), from its `# Icon:` data or file. With `cache`, tiles
/// are read from and written to that folder (keyed by the source bytes and size). `Ok(None)` when the app
/// has no icon.
pub fn app_icon(app: &App, w: u32, h: u32, cache: Option<&std::path::Path>) -> Result<Option<GrayImage>, String> {
    let bytes = match &app.icon {
        IconSource::None => return Ok(None),
        IconSource::Data(b64) => kl_core::base64::decode(b64).ok_or("icon: bad base64")?,
        IconSource::File(path) => std::fs::read(path).map_err(|e| format!("icon {path}: {e}"))?,
        IconSource::Bundled(bytes) => bytes.to_vec(),
    };
    let key = image::cache_key(&bytes, w, h);
    if let Some(dir) = cache {
        if let Some(tile) = std::fs::read(dir.join(&key)).ok().and_then(|b| image::from_cache(&b)) {
            if (tile.width, tile.height) == (w, h) {
                return Ok(Some(tile));
            }
        }
    }
    let tile = image::icon_tile(&bytes, w.min(h))?;
    if let Some(dir) = cache {
        let _ = std::fs::create_dir_all(dir);
        let tmp = dir.join(format!("{key}.tmp"));
        if std::fs::write(&tmp, image::to_cache(&tile)).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join(&key));
        }
    }
    Ok(Some(tile))
}

/// Load the icon tiles of every app into the renderer. Returns the problems (bad icons draw as monograms).
pub fn load_icons(renderer: &mut Renderer, apps: &[App], layout: &Layout, cache: Option<&std::path::Path>) -> Vec<String> {
    let icon = layout.icon(0);
    let mut problems = Vec::new();
    for app in apps {
        match app_icon(app, icon.w as u32, icon.h as u32, cache) {
            Ok(Some(tile)) => renderer.set_icon(&app.id, tile),
            Ok(None) => {}
            Err(e) => problems.push(format!("{}: {e}", app.name)),
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_rows_finds_the_band() {
        let a = vec![0u8; 4 * 5];
        let mut b = a.clone();
        assert_eq!(changed_rows(&a, &b, 4), None);
        b[4 + 2] = 9;
        b[4 * 3] = 9;
        assert_eq!(changed_rows(&a, &b, 4), Some((1, 3)));
    }

    #[test]
    fn for_panel_turns_only_when_needed() {
        let buf = vec![1, 2, 3];
        assert_eq!(for_panel(buf.clone(), Rotation::Upright, 0), vec![1, 2, 3]);
        assert_eq!(for_panel(buf.clone(), Rotation::UpsideDown, 0), vec![3, 2, 1]);
        assert_eq!(for_panel(buf.clone(), Rotation::UpsideDown, 2), vec![1, 2, 3]);
        assert_eq!(for_panel(buf, Rotation::Upright, 2), vec![3, 2, 1]);
    }

    #[test]
    fn the_bundled_kindle_ui_icon_makes_a_tile() {
        let kindle = App {
            id: "kindle".into(),
            name: "Kindle UI".into(),
            icon: IconSource::Bundled(kl_core::apps::KINDLE_UI_ICON),
            launch: Launch::KindleMode,
            dir: "/".into(),
            order: None,
            hidden: false,
            background: false,
        };
        let tile = app_icon(&kindle, 184, 184, None).unwrap().unwrap();
        assert_eq!((tile.width, tile.height), (184, 184));
        // The reader's silhouette is black; the sky above it is a mid gray.
        assert!(tile.pixels.iter().filter(|&&p| p == 0).count() > 184 * 184 / 5);
        assert!(tile.pixels.iter().any(|&p| p > 60 && p < 200));
    }

    #[test]
    fn blit_stays_inside() {
        let mut out = vec![255u8; 10 * 4];
        let tile = GrayImage { width: 3, height: 3, pixels: vec![0; 9] };
        blit(&mut out, 10, Rect::new(8, 2, 3, 3), &tile);
        assert_eq!(out.iter().filter(|&&p| p == 0).count(), 4);
    }

    #[test]
    fn changed_rect_is_the_box_around_the_changes_on_8_pixel_columns() {
        let a = vec![0u8; 32 * 6];
        let mut b = a.clone();
        assert_eq!(changed_rect(&a, &b, 32), None);
        b[32 + 9] = 9;
        b[32 * 4 + 20] = 9;
        assert_eq!(changed_rect(&a, &b, 32), Some((8, 1, 16, 4)));
        assert_eq!(changed_rect(&a, &[0u8; 4], 32), Some((0, 0, 32, 0)));
    }
}
