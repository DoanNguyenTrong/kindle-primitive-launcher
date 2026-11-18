//! Draws the launcher's screens into an 8-bit gray buffer: shapes with `vello_cpu`, text with `parley`
//! (the same libraries and versions as Primitive Browser), icons as pre-dithered gray tiles.
//!
//! E-ink rules (see the `kindle-eink` skill): chrome and text are pure black on pure white after
//! `solidify`, so page changes do not flash; only icons carry gray, dithered to the panel's 16 steps.

pub mod image;

use std::collections::HashMap;

use kl_core::{
    layout::{Button, Rect, Row, LABEL_SIZE},
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
/// Pop-ups (the app menu, the Kindle UI dialog): corner radius, and the white margin around them.
const POPUP_RADIUS: f32 = 28.0;
const POPUP_HALO: i32 = 10;

// Icon paths copied from Primitive Browser (`crates/kb-app/src/ui.rs`, the `icon` set and `battery_icon`).
const WIFI_ARC_OUTER: &str = "M2.5 9 A14 14 0 0 1 21.5 9";
const WIFI_ARC_MIDDLE: &str = "M5.5 12.5 A9.5 9.5 0 0 1 18.5 12.5";
const WIFI_ARC_INNER: &str = "M8.5 16 A5 5 0 0 1 15.5 16";
const BOLT: &str = "M8 0 L1 12 H6 L4 22 L13 9 H8 Z";

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
}

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
    /// on a 24-unit grid, stroke 2, with a slash when Wi-Fi is off.
    fn wifi_icon(&mut self, x: f64, y: f64, size: f64, on: bool) {
        for arc in [WIFI_ARC_OUTER, WIFI_ARC_MIDDLE, WIFI_ARC_INNER] {
            self.stroke_icon(arc, x, y, size, 2.0, BLACK);
        }
        let k = size / 24.0;
        self.ctx.set_paint(gray(BLACK));
        let dot = kurbo::Circle::new((x + 12.0 * k, y + 19.5 * k), 1.6 * k).to_path(0.1);
        self.ctx.fill_path(&dot);
        if !on {
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
        self.ctx.reset();
        let l = ui.layout;
        self.fill(l.screen(), WHITE);
        let mut icon_blits: Vec<(Rect, String)> = Vec::new();
        match ui.screen {
            Screen::Home => self.home(ui, status, &mut icon_blits),
            Screen::Settings => self.settings(ui, status),
            // The dialog is a pop-up over the grid, drawn last (below).
            Screen::ConfirmKindle => self.home(ui, status, &mut icon_blits),
            // A sleep picture replaces the text, except for a battery warning, which matters more.
            Screen::Sleep => {
                if self.sleep_image.is_none() || ui.battery_message().is_some() {
                    self.sleep(&l, ui.battery_message());
                }
            }
            Screen::AppMenu => self.app_menu(ui, status, &mut icon_blits),
            Screen::Extensions => self.extensions(ui),
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
            Screen::AppMenu => Some(l.menu_box(ui.menu_anchor(), ui.menu_actions().len())),
            _ => None,
        };
        if let Some(b) = popup {
            self.ctx.reset();
            self.fill(l.screen(), WHITE);
            match ui.screen {
                Screen::ConfirmKindle => self.confirm(&l),
                _ => self.app_menu_box(ui),
            }
            let layer = self.finish_gray();
            image::copy_rounded(&mut out, &layer, width, (b.x - POPUP_HALO, b.y - POPUP_HALO, b.w + 2 * POPUP_HALO, b.h + 2 * POPUP_HALO), POPUP_RADIUS + POPUP_HALO as f32);
        }
        if status.dark_mode && ui.screen != Screen::Sleep {
            let halo = popup.map(|b| b.inset(-POPUP_HALO));
            image::invert_except(&mut out, width, &pictures, halo.map(|h| (h.x, h.y, h.w, h.h)));
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

    fn status_strip(&mut self, l: &Layout, s: &Status) {
        let r = l.status();
        let clock = if s.clock.is_empty() { "--:--" } else { s.clock.as_str() };
        self.text(Text { text: clock, x: 40.0, y: 26.0, w: 300.0, size: 46.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        if !s.date.is_empty() {
            self.text(Text { text: &s.date, x: 210.0, y: 38.0, w: 360.0, size: 34.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
        }
        // Battery and Wi-Fi icons as in Primitive Browser, at the right of the strip.
        let bx = r.right() - 40 - 128;
        if let Some(level) = s.battery {
            self.battery_icon(bx, (r.h - 56) / 2, level, s.charging);
        }
        let wifi_size = 48.0;
        let wx = bx as f64 - 28.0 - wifi_size;
        self.wifi_icon(wx, (r.h as f64 - wifi_size) / 2.0, wifi_size, s.wifi_on);
        // While Wi-Fi is on but not connected, say so beside the icon.
        let note = match (s.wifi_on, s.wifi_state.as_str()) {
            (true, "connected") | (false, _) => String::new(),
            (true, "") => "Wi-Fi …".to_string(),
            (true, other) => capitalise(other),
        };
        if !note.is_empty() {
            let nw = 320.0;
            self.text(Text { text: &note, x: (wx - 16.0) as f32 - nw, y: 38.0, w: nw, size: 30.0, bold: false, align: Align::End, lines: 1, gray: BLACK });
        }
        self.fill(Rect::new(0, r.bottom() - 4, r.w, 4), BLACK);
    }

    /// The home screen: status bar, the page's icons with their names, and the page dots.
    fn home(&mut self, ui: &Ui, s: &Status, icon_blits: &mut Vec<(Rect, String)>) {
        let l = ui.layout;
        self.status_strip(&l, s);
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
        self.ctx.set_paint(gray(WHITE));
        for i in 0..8 {
            let turn = Affine::translate((cx, cy)) * Affine::rotate(i as f64 * std::f64::consts::FRAC_PI_4);
            self.ctx.set_transform(turn);
            let tooth = kurbo::RoundedRect::new(-0.065 * s, -0.36 * s, 0.065 * s, -0.2 * s, 0.02 * s).to_path(0.1);
            self.ctx.fill_path(&tooth);
        }
        self.ctx.set_transform(Affine::IDENTITY);
        self.ctx.fill_path(&kurbo::Circle::new((cx, cy), 0.27 * s).to_path(0.1));
        self.ctx.set_paint(gray(BLACK));
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

    fn settings(&mut self, ui: &Ui, s: &Status) {
        let l = ui.layout;
        let title = l.title();
        self.text(Text { text: "Settings", x: 40.0, y: 32.0, w: 600.0, size: 56.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
        self.button(l.done_button(), "Done", true);
        self.fill(Rect::new(0, title.bottom() - 4, title.w, 4), BLACK);
        for row in l.settings_rows() {
            let value = match row.row {
                Row::Frontlight => format!("{} / 24", s.light),
                Row::Warmth => format!("{} / 24", s.warmth),
                Row::Wifi => match (s.wifi_on, s.wifi_state.as_str()) {
                    (false, _) => "Off".into(),
                    (true, "") => "On".into(),
                    (true, state) => capitalise(state),
                },
                Row::Rotation => match (s.auto_rotation, s.rotation) {
                    (true, Rotation::Upright) => "Automatic: upright".into(),
                    (true, Rotation::UpsideDown) => "Automatic: upside down".into(),
                    (false, Rotation::Upright) => "Upright (fixed)".into(),
                    (false, Rotation::UpsideDown) => "Upside down (fixed)".into(),
                },
                Row::Ssh => if s.ssh { "Running".into() } else { "Off".into() },
                Row::Autostart => if s.autostart { "On: the launcher opens after boot".into() } else { "Off: the stock Kindle opens after boot".into() },
                Row::SleepScreen => if s.sleep_launcher { "The launcher's own".into() } else { "The Kindle's".into() },
                Row::Hidden => match ui.hidden_count() {
                    0 => "None. Long-press to hide".into(),
                    n => format!("{n} hidden"),
                },
                Row::KindleUi => "Stock Kindle, about a minute".into(),
                Row::DarkMode => if s.dark_mode { "On: white on black".into() } else { "Off".into() },
                Row::Animation => if s.animation { "On: a zoom when an app opens".into() } else { "Off".into() },
            };
            let r = row.rect;
            self.text(Text { text: row.row.label(), x: r.x as f32, y: r.y as f32 + 14.0, w: 520.0, size: 38.0, bold: true, align: Align::Start, lines: 1, gray: BLACK });
            self.text(Text { text: &value, x: r.x as f32, y: r.y as f32 + 64.0, w: 560.0, size: 30.0, bold: false, align: Align::Start, lines: 1, gray: BLACK });
            for (b, rect) in row.buttons {
                // Minus and plus are drawn as bars: the bundled font has no minus sign.
                if matches!(b, Button::LightDown | Button::WarmDown | Button::LightUp | Button::WarmUp) {
                    self.button(rect, "", false);
                    let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
                    self.fill(Rect::new(cx - 22, cy - 3, 44, 7), BLACK);
                    if matches!(b, Button::LightUp | Button::WarmUp) {
                        self.fill(Rect::new(cx - 3, cy - 22, 7, 44), BLACK);
                    }
                    continue;
                }
                let label = match b {
                    Button::Wifi => if s.wifi_on { "Turn off" } else { "Turn on" },
                    Button::Rotation => "Turn",
                    Button::RotationAuto => "Auto",
                    Button::Ssh => if s.ssh { "Stop" } else { "Start" },
                    Button::Autostart => if s.autostart { "Turn off" } else { "Turn on" },
                    Button::ShowHidden => "Show all",
                    Button::SleepScreen => "Switch",
                    Button::KindleUi => "Open",
                    Button::DarkMode => if s.dark_mode { "Turn off" } else { "Turn on" },
                    Button::Animation => if s.animation { "Turn off" } else { "Turn on" },
                    _ => "",
                };
                self.button(rect, label, false);
            }
            self.fill(Rect::new(r.x, r.bottom() - 2, r.w, 2), BLACK);
        }
        let a = l.about();
        let about = format!(
            "Primitive Launcher {}\n{} apps · {}\nFrom the Kindle UI, open “Launcher” in the library to come back.",
            kl_core::VERSION,
            ui.apps().len(),
            if s.memory.is_empty() { "memory unknown" } else { &s.memory },
        );
        let mut y = a.y as f32;
        for line in about.lines() {
            y += self.text(Text { text: line, x: a.x as f32, y, w: a.w as f32, size: 32.0, bold: false, align: Align::Start, lines: 2, gray: BLACK }) + 6.0;
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
