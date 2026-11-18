//! Screen geometry, shared by drawing and hit testing so a tap lands on what is drawn. All values are
//! device pixels of the upright portrait screen (1264 × 1680 on the Oasis 3).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x as f32 && y >= self.y as f32 && x < (self.x + self.w) as f32 && y < (self.y + self.h) as f32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    /// The rectangle shrunk by `d` on every side.
    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2 * d).max(0), (self.h - 2 * d).max(0))
    }
}

/// The steps of the opening zoom: `n` rectangles growing from `from` (the tapped icon) to `to` (the screen),
/// eased out (big steps first), the last one `to`.
pub fn zoom_steps(from: Rect, to: Rect, n: usize) -> Vec<Rect> {
    let lerp = |a: i32, b: i32, t: f32| a + ((b - a) as f32 * t).round() as i32;
    (1..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let t = 1.0 - (1.0 - t) * (1.0 - t);
            Rect::new(lerp(from.x, to.x, t), lerp(from.y, to.y, t), lerp(from.w, to.w, t), lerp(from.h, to.h, t))
        })
        .collect()
}

/// Controls on the Settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Done,
    LightDown,
    LightUp,
    WarmDown,
    WarmUp,
    Wifi,
    Rotation,
    RotationAuto,
    Ssh,
    Autostart,
    ShowHidden,
    SleepScreen,
    KindleUi,
    MenuEarlier,
    MenuLater,
    MenuIcon,
    MenuHide,
    ConfirmCancel,
    ConfirmOpen,
    DarkMode,
    Animation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Frontlight,
    Warmth,
    DarkMode,
    Wifi,
    Rotation,
    Animation,
    Ssh,
    Autostart,
    Hidden,
    SleepScreen,
    KindleUi,
}

impl Row {
    pub const ALL: [Row; 11] = [
        Row::Frontlight,
        Row::Warmth,
        Row::DarkMode,
        Row::Wifi,
        Row::Rotation,
        Row::Animation,
        Row::Ssh,
        Row::Autostart,
        Row::Hidden,
        Row::SleepScreen,
        Row::KindleUi,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Row::Frontlight => "Frontlight",
            Row::Warmth => "Warmth",
            Row::DarkMode => "Dark mode",
            Row::Animation => "Opening animation",
            Row::Wifi => "Wi-Fi",
            Row::Rotation => "Screen",
            Row::Ssh => "SSH (port 2222)",
            Row::Autostart => "Start with the Kindle",
            Row::Hidden => "Hidden apps",
            Row::SleepScreen => "Sleep screen",
            Row::KindleUi => "Kindle UI",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsRow {
    pub row: Row,
    pub rect: Rect,
    pub buttons: Vec<(Button, Rect)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub cols: i32,
    pub rows: i32,
}

pub const STATUS_H: i32 = 112;
/// The row of page dots (and notices) at the bottom of the home screen.
pub const FOOTER_H: i32 = 90;
pub const MARGIN: i32 = 40;
/// The home grid's side margin.
const GRID_SIDE: i32 = 24;
/// Icons are rounded squares of one size: at most this many pixels.
const ICON_MAX: i32 = 184;
/// Gap between an icon and its name.
const LABEL_GAP: i32 = 12;
/// Label text size and line height under icons (one line).
pub const LABEL_SIZE: f32 = 30.0;
pub const LABEL_LINE: i32 = 40;
/// The app menu (long press): a box beside the icon, one row per action under the app's name.
const MENU_W: i32 = 600;
const MENU_ROW_H: i32 = 100;
const TITLE_H: i32 = 130;
/// One row of the Extensions list.
const LIST_ROW_H: i32 = 120;
const ROW_H: i32 = 116;
const BUTTON_W: i32 = 150;
const WIDE_W: i32 = 300;
const BUTTON_H: i32 = 100;

impl Layout {
    pub fn new(width: u32, height: u32, cols: u32, rows: u32) -> Layout {
        Layout { width: width as i32, height: height as i32, cols: cols.clamp(3, 5) as i32, rows: rows.clamp(3, 6) as i32 }
    }

    pub fn screen(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn status(&self) -> Rect {
        Rect::new(0, 0, self.width, STATUS_H)
    }

    /// The app grid, between the status bar and the page dots.
    pub fn grid(&self) -> Rect {
        let top = STATUS_H + 20;
        Rect::new(GRID_SIDE, top, self.width - 2 * GRID_SIDE, self.home_footer().y - top)
    }

    /// The bottom bar of list screens (Extensions): page controls.
    pub fn footer(&self) -> Rect {
        Rect::new(0, self.height - 100, self.width, 100)
    }

    /// The row of page dots at the bottom of the home screen. Notices are shown here too.
    pub fn home_footer(&self) -> Rect {
        Rect::new(0, self.height - 20 - FOOTER_H, self.width, FOOTER_H)
    }

    /// The side of every icon: as large as the cell allows, at most `ICON_MAX`.
    pub fn icon_size(&self) -> i32 {
        let grid = self.grid();
        let (cw, ch) = (grid.w / self.cols, grid.h / self.rows);
        ICON_MAX.min(cw - 2 * MARGIN).min(ch - LABEL_GAP - LABEL_LINE - 16)
    }


    pub fn per_page(&self) -> usize {
        (self.cols * self.rows) as usize
    }

    pub fn pages(&self, count: usize) -> usize {
        count.div_ceil(self.per_page()).max(1)
    }

    /// The cell of slot `slot` (0-based on the page, row by row).
    pub fn cell(&self, slot: usize) -> Rect {
        let grid = self.grid();
        let (col, row) = (slot as i32 % self.cols, slot as i32 / self.cols);
        let (cw, ch) = (grid.w / self.cols, grid.h / self.rows);
        Rect::new(grid.x + col * cw, grid.y + row * ch, cw, ch)
    }

    /// The icon in a cell: a square of `icon_size`, centred with its name under it.
    pub fn icon(&self, slot: usize) -> Rect {
        let cell = self.cell(slot);
        let s = self.icon_size();
        let top = cell.y + (cell.h - s - LABEL_GAP - LABEL_LINE) / 2;
        Rect::new(cell.x + (cell.w - s) / 2, top, s, s)
    }

    /// The app's name under its icon: one line.
    pub fn label(&self, slot: usize) -> Rect {
        let cell = self.cell(slot);
        let icon = self.icon(slot);
        Rect::new(cell.x + 6, icon.bottom() + LABEL_GAP, cell.w - 12, LABEL_LINE)
    }

    /// The slot under a point on the home screen.
    pub fn slot_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.per_page()).find(|&s| self.cell(s).contains(x, y))
    }

    pub fn title(&self) -> Rect {
        Rect::new(0, 0, self.width, TITLE_H)
    }

    pub fn done_button(&self) -> Rect {
        Rect::new(self.width - MARGIN - 200, (TITLE_H - BUTTON_H) / 2, 200, BUTTON_H)
    }

    pub fn settings_rows(&self) -> Vec<SettingsRow> {
        let right = self.width - MARGIN;
        let top = TITLE_H + 30;
        Row::ALL
            .iter()
            .enumerate()
            .map(|(i, &row)| {
                let rect = Rect::new(MARGIN, top + i as i32 * ROW_H, self.width - 2 * MARGIN, ROW_H);
                let by = rect.y + (ROW_H - BUTTON_H) / 2;
                let pair = |a, b| {
                    vec![
                        (a, Rect::new(right - 2 * BUTTON_W - 24, by, BUTTON_W, BUTTON_H)),
                        (b, Rect::new(right - BUTTON_W, by, BUTTON_W, BUTTON_H)),
                    ]
                };
                let wide = |b| vec![(b, Rect::new(right - WIDE_W, by, WIDE_W, BUTTON_H))];
                let buttons = match row {
                    Row::Frontlight => pair(Button::LightDown, Button::LightUp),
                    Row::Warmth => pair(Button::WarmDown, Button::WarmUp),
                    Row::Wifi => wide(Button::Wifi),
                    Row::DarkMode => wide(Button::DarkMode),
                    Row::Animation => wide(Button::Animation),
                    Row::Rotation => pair(Button::Rotation, Button::RotationAuto),
                    Row::Ssh => wide(Button::Ssh),
                    Row::Autostart => wide(Button::Autostart),
                    Row::Hidden => wide(Button::ShowHidden),
                    Row::SleepScreen => wide(Button::SleepScreen),
                    Row::KindleUi => wide(Button::KindleUi),
                };
                SettingsRow { row, rect, buttons }
            })
            .collect()
    }

    /// The area under the settings rows, for the About text.
    pub fn about(&self) -> Rect {
        let top = TITLE_H + 30 + Row::ALL.len() as i32 * ROW_H + 30;
        Rect::new(MARGIN, top, self.width - 2 * MARGIN, self.height - top - MARGIN)
    }

    /// The app menu's box for an icon at `anchor` with `actions` rows under the name row: below the icon
    /// when it fits, else above it; inside the screen.
    pub fn menu_box(&self, anchor: Rect, actions: usize) -> Rect {
        let h = (actions as i32 + 1) * MENU_ROW_H;
        let x = (anchor.x + anchor.w / 2 - MENU_W / 2).clamp(MARGIN, self.width - MARGIN - MENU_W);
        // Below the icon's name, so the name stays readable.
        let below = anchor.bottom() + LABEL_GAP + LABEL_LINE + 16;
        let y = if below + h <= self.height - 20 { below } else { (anchor.y - 24 - h).max(STATUS_H) };
        Rect::new(x, y, MENU_W, h)
    }

    /// Row `i` of the app menu's actions (row 0 of the box is the app's name).
    pub fn menu_row(&self, anchor: Rect, actions: usize, i: usize) -> Rect {
        let b = self.menu_box(anchor, actions);
        Rect::new(b.x, b.y + (i as i32 + 1) * MENU_ROW_H, b.w, MENU_ROW_H)
    }

    /// Rows of the Extensions list on one page (between the title and the footer).
    pub fn list_rows_per_page(&self) -> usize {
        ((self.footer().y - TITLE_H - 20) / LIST_ROW_H).max(1) as usize
    }

    /// Row `i` of the Extensions list on the current page.
    pub fn list_row(&self, i: usize) -> Rect {
        Rect::new(MARGIN, TITLE_H + 20 + i as i32 * LIST_ROW_H, self.width - 2 * MARGIN, LIST_ROW_H)
    }

    pub fn list_row_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.list_rows_per_page()).find(|&i| self.list_row(i).contains(x, y))
    }

    /// Pages of the Extensions list for `n` items (at least one).
    pub fn list_pages(&self, n: usize) -> usize {
        n.div_ceil(self.list_rows_per_page()).max(1)
    }

    pub fn settings_button_at(&self, x: f32, y: f32) -> Option<Button> {
        if self.done_button().inset(-20).contains(x, y) {
            return Some(Button::Done);
        }
        self.settings_rows()
            .into_iter()
            .flat_map(|r| r.buttons)
            .find(|(_, rect)| rect.inset(-12).contains(x, y))
            .map(|(b, _)| b)
    }

    pub fn confirm_box(&self) -> Rect {
        Rect::new(MARGIN * 2, self.height / 2 - 300, self.width - 4 * MARGIN, 600)
    }

    pub fn confirm_buttons(&self) -> [(Button, Rect); 2] {
        let b = self.confirm_box();
        let y = b.bottom() - 40 - BUTTON_H;
        [
            (Button::ConfirmCancel, Rect::new(b.x + 40, y, 300, BUTTON_H)),
            (Button::ConfirmOpen, Rect::new(b.right() - 40 - 400, y, 400, BUTTON_H)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oasis() -> Layout {
        Layout::new(1264, 1680, 4, 5)
    }

    #[test]
    fn icons_are_equal_squares_on_an_even_grid() {
        let l = oasis();
        assert_eq!(l.per_page(), 20);
        assert!(l.cell(0).y > STATUS_H);
        assert!(l.cell(19).bottom() <= l.home_footer().y);
        assert!(l.home_footer().bottom() <= l.height);
        let s = l.icon_size();
        assert!(s >= 160, "icons {s} px");
        for slot in 0..20 {
            let (icon, cell, label) = (l.icon(slot), l.cell(slot), l.label(slot));
            assert_eq!((icon.w, icon.h), (s, s));
            assert!(icon.x >= cell.x && icon.right() <= cell.right());
            assert!(label.bottom() <= cell.bottom(), "label inside cell {slot}");
            // Columns and rows line up: the same x in a column, the same y in a row.
            assert_eq!(icon.x, l.icon(slot % 4).x);
            assert_eq!(icon.y, l.icon(slot / 4 * 4).y);
        }
    }

    #[test]
    fn the_menu_box_opens_below_or_above_the_icon_inside_the_screen() {
        let l = oasis();
        let top = l.icon(0);
        let b = l.menu_box(top, 5);
        assert!(b.y > top.bottom() && b.x >= MARGIN && b.right() <= l.width - MARGIN);
        let last = l.icon(19);
        let b = l.menu_box(last, 5);
        assert!(b.bottom() < last.y, "above an icon in the last row: {b:?}");
        assert_eq!(l.menu_row(last, 5, 0).y, b.y + MENU_ROW_H);
    }

    #[test]
    fn the_zoom_grows_from_the_icon_to_the_screen() {
        let l = oasis();
        let icon = l.icon(5);
        let steps = zoom_steps(icon, l.screen(), 3);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[2], l.screen());
        assert!(steps[0].w > icon.w && steps[0].w < steps[1].w);
        // Eased out: the first step covers more than a third of the way.
        assert!(steps[0].w - icon.w > (l.width - icon.w) / 3);
    }

    #[test]
    fn hit_testing_finds_slots() {
        let l = oasis();
        let c = l.cell(5);
        assert_eq!(l.slot_at((c.x + 5) as f32, (c.y + 5) as f32), Some(5));
        assert_eq!(l.slot_at(10.0, 10.0), None);
        assert_eq!(l.pages(0), 1);
        assert_eq!(l.pages(20), 1);
        assert_eq!(l.pages(21), 2);
    }

    #[test]
    fn settings_fit_and_buttons_hit() {
        let l = oasis();
        let rows = l.settings_rows();
        assert!(rows.last().unwrap().rect.bottom() < l.about().y);
        assert!(l.about().h >= 140, "about {:?}", l.about());
        let (b, r) = rows[0].buttons[1];
        assert_eq!(l.settings_button_at((r.x + 10) as f32, (r.y + 10) as f32), Some(b));
        let d = l.done_button();
        assert_eq!(l.settings_button_at((d.x + 1) as f32, (d.y + 1) as f32), Some(Button::Done));
    }
}
