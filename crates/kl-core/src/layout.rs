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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Frontlight,
    Warmth,
    Wifi,
    Rotation,
    Ssh,
    Autostart,
    Hidden,
    SleepScreen,
    KindleUi,
}

impl Row {
    pub const ALL: [Row; 9] = [
        Row::Frontlight,
        Row::Warmth,
        Row::Wifi,
        Row::Rotation,
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
pub const FOOTER_H: i32 = 100;
pub const MARGIN: i32 = 40;
/// Label text size and line height on tiles.
pub const LABEL_SIZE: f32 = 34.0;
pub const LABEL_LINE: i32 = 44;
const LABEL_H: i32 = LABEL_LINE * 2 + 4;
const TITLE_H: i32 = 130;
/// One row of the Extensions list.
const LIST_ROW_H: i32 = 120;
const ROW_H: i32 = 140;
const BUTTON_W: i32 = 150;
const WIDE_W: i32 = 300;
const BUTTON_H: i32 = 100;

impl Layout {
    pub fn new(width: u32, height: u32, cols: u32, rows: u32) -> Layout {
        Layout { width: width as i32, height: height as i32, cols: cols.clamp(2, 4) as i32, rows: rows.clamp(2, 4) as i32 }
    }

    pub fn screen(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    pub fn status(&self) -> Rect {
        Rect::new(0, 0, self.width, STATUS_H)
    }

    pub fn grid(&self) -> Rect {
        Rect::new(0, STATUS_H, self.width, self.height - STATUS_H - FOOTER_H)
    }

    pub fn footer(&self) -> Rect {
        Rect::new(0, self.height - FOOTER_H, self.width, FOOTER_H)
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

    /// The icon box in a cell: 2:3 portrait, like a book cover, centred.
    pub fn icon(&self, slot: usize) -> Rect {
        let cell = self.cell(slot);
        let mut h = cell.h - 18 - 12 - LABEL_H - 10;
        let mut w = h * 2 / 3;
        if w > cell.w - 2 * MARGIN {
            w = cell.w - 2 * MARGIN;
            h = w * 3 / 2;
        }
        Rect::new(cell.x + (cell.w - w) / 2, cell.y + 18, w, h)
    }

    pub fn label(&self, slot: usize) -> Rect {
        let cell = self.cell(slot);
        let icon = self.icon(slot);
        Rect::new(cell.x + 12, icon.bottom() + 12, cell.w - 24, LABEL_H)
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

    /// The app menu (long press on a tile): four wide buttons under the title.
    pub fn app_menu_buttons(&self) -> [(Button, Rect); 4] {
        let top = TITLE_H + 40;
        let w = self.width - 2 * MARGIN;
        let at = |i: i32| Rect::new(MARGIN, top + i * (BUTTON_H + 28), w, BUTTON_H);
        [(Button::MenuEarlier, at(0)), (Button::MenuLater, at(1)), (Button::MenuIcon, at(2)), (Button::MenuHide, at(3))]
    }

    /// Where the app menu shows the app's tile, under its buttons.
    pub fn app_menu_preview(&self) -> Rect {
        let icon = self.icon(0);
        let top = TITLE_H + 40 + 4 * (BUTTON_H + 28) + 40;
        Rect::new((self.width - icon.w) / 2, top, icon.w, icon.h)
    }

    pub fn app_menu_button_at(&self, x: f32, y: f32) -> Option<Button> {
        if self.done_button().inset(-20).contains(x, y) {
            return Some(Button::Done);
        }
        self.app_menu_buttons().into_iter().find(|(_, r)| r.inset(-8).contains(x, y)).map(|(b, _)| b)
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
        Layout::new(1264, 1680, 3, 3)
    }

    #[test]
    fn cells_tile_the_grid_and_icons_are_portrait() {
        let l = oasis();
        assert_eq!(l.per_page(), 9);
        assert_eq!(l.cell(0).y, STATUS_H);
        assert!(l.cell(8).bottom() <= l.footer().y);
        for s in 0..9 {
            let (icon, cell, label) = (l.icon(s), l.cell(s), l.label(s));
            assert!((icon.h * 2 - icon.w * 3).abs() <= 3, "2:3 icon {icon:?}");
            assert!(icon.x >= cell.x && icon.right() <= cell.right());
            assert!(label.bottom() <= cell.bottom(), "label inside cell {s}");
        }
    }

    #[test]
    fn hit_testing_finds_slots() {
        let l = oasis();
        let c = l.cell(4);
        assert_eq!(l.slot_at((c.x + 5) as f32, (c.y + 5) as f32), Some(4));
        assert_eq!(l.slot_at(10.0, 10.0), None);
        assert_eq!(l.pages(0), 1);
        assert_eq!(l.pages(9), 1);
        assert_eq!(l.pages(10), 2);
    }

    #[test]
    fn settings_fit_and_buttons_hit() {
        let l = oasis();
        let rows = l.settings_rows();
        assert!(rows.last().unwrap().rect.bottom() < l.about().y);
        assert!(l.about().h > 150);
        let (b, r) = rows[0].buttons[1];
        assert_eq!(l.settings_button_at((r.x + 10) as f32, (r.y + 10) as f32), Some(b));
        let d = l.done_button();
        assert_eq!(l.settings_button_at((d.x + 1) as f32, (d.y + 1) as f32), Some(Button::Done));
    }
}
