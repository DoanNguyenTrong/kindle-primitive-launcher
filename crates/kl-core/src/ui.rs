//! The launcher's UI state machine. Input comes in as gestures in screen coordinates (already turned
//! for the rotation); the result is an `Action` for the driver: redraw, launch an app, change a device
//! setting, or leave for the Kindle UI.

use crate::{
    apps::{sort_key, App, Launch},
    battery::{self, Warning},
    layout::{Button, Layout},
    settings::Rotation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Settings,
    /// "Open the Kindle UI?" over the previous screen.
    ConfirmKindle,
    Sleep,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    Tap(f32, f32),
    LongTap(f32, f32),
    Swipe { dx: f32, dy: f32 },
    PageForward,
    PageBack,
    /// `powerd` is going to the screensaver.
    Sleep,
    /// `powerd` left the screensaver.
    Wake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceCmd {
    Light(i32),
    Warmth(i32),
    ToggleWifi,
    ToggleRotation,
    ToggleSsh,
    ToggleAutostart,
    ToggleSleepScreen,
    /// Follow the device's turn again (Settings > Screen > Auto).
    AutoRotation,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    /// Draw the current screen. `full` asks for a flashing full refresh (screen changes that clear ghosting).
    Redraw { full: bool },
    /// Show the pressed tile, then hand the app to the supervisor.
    Launch(App),
    /// Leave: the supervisor starts the stock Kindle UI.
    KindleMode,
    /// Change a device setting, then redraw.
    Device(DeviceCmd),
    /// Hide (true) or show (false) apps by id: written to apps.toml, then redraw.
    SetHidden(Vec<(String, bool)>),
}

/// What the status strip and Settings show. Filled by the driver.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub clock: String,
    pub date: String,
    pub battery: Option<u32>,
    pub charging: bool,
    pub wifi_on: bool,
    /// "connected", "not connected", "off", or empty when unknown.
    pub wifi_state: String,
    pub light: u32,
    pub warmth: u32,
    pub ssh: bool,
    /// False while the launcher is off at boot (the DISABLE file exists).
    pub autostart: bool,
    /// The launcher's own sleep screen is on (the stock screensaver is unloaded).
    pub sleep_launcher: bool,
    /// The screen follows the device's turn.
    pub auto_rotation: bool,
    pub rotation: Rotation,
    /// Free memory for the About text, e.g. "MemAvailable 312 MB".
    pub memory: String,
}

/// A horizontal swipe longer than this (pixels) turns the page.
const SWIPE_MIN: f32 = 150.0;

pub struct Ui {
    apps: Vec<App>,
    pub layout: Layout,
    pub page: usize,
    pub screen: Screen,
    before_sleep: Screen,
    /// The pressed tile slot, drawn inverted until the app starts.
    pub pressed: Option<usize>,
    /// A message in the footer (problems in apps.toml, a failed action). Cleared by the next tap.
    pub notice: Option<String>,
    /// Apps hidden by the user (long press, or Settings > Hidden apps). Kept so they can be shown again.
    hidden_apps: Vec<App>,
    /// Low or critical battery, shown in the footer until the level recovers or the charger is connected.
    pub battery: Warning,
    battery_level: Option<u32>,
}

impl Ui {
    /// `apps` is the merged list; hidden apps are dropped here.
    pub fn new(apps: Vec<App>, layout: Layout) -> Ui {
        let (hidden_apps, apps): (Vec<App>, Vec<App>) = apps.into_iter().partition(|a| a.hidden);
        Ui {
            apps,
            hidden_apps,
            layout,
            page: 0,
            screen: Screen::Home,
            before_sleep: Screen::Home,
            pressed: None,
            notice: None,
            battery: Warning::None,
            battery_level: None,
        }
    }

    /// Record a battery reading. Returns true when the warning changed (the screen needs a redraw). A critical
    /// battery on the home screen goes to the sleep screen, which shows the warning; the power button wakes it.
    pub fn set_battery(&mut self, level: Option<u32>, charging: bool) -> bool {
        let next = battery::assess(level, charging, self.battery);
        self.battery_level = level;
        let changed = next != self.battery;
        self.battery = next;
        if changed && next == Warning::Critical && self.screen == Screen::Home {
            self.before_sleep = Screen::Home;
            self.screen = Screen::Sleep;
            self.pressed = None;
        }
        changed
    }

    /// The battery text for the footer, if a warning is active.
    pub fn battery_message(&self) -> Option<String> {
        battery::message(self.battery, self.battery_level)
    }

    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden_apps.len()
    }

    /// Long press on a tile: hide that app. The Kindle UI and Settings stay in the grid.
    fn home_long_tap(&mut self, x: f32, y: f32) -> Action {
        let l = self.layout;
        let Some(slot) = l.slot_at(x, y) else { return Action::None };
        let index = self.page * l.per_page() + slot;
        let Some(app) = self.apps.get(index) else { return Action::None };
        if matches!(app.launch, Launch::Settings | Launch::KindleMode) {
            self.notice = Some(format!("{} stays in the grid", app.name));
            return Action::Redraw { full: false };
        }
        let app = self.apps.remove(index);
        let id = app.id.clone();
        self.notice = Some(format!("Hidden: {}. Show it again in Settings > Hidden apps.", app.name));
        self.hidden_apps.push(App { hidden: true, ..app });
        self.page = self.page.min(self.pages().saturating_sub(1));
        Action::SetHidden(vec![(id, true)])
    }

    /// Settings > Hidden apps > Show all.
    pub fn show_all(&mut self) -> Action {
        if self.hidden_apps.is_empty() {
            return Action::None;
        }
        let back: Vec<App> = self.hidden_apps.drain(..).map(|a| App { hidden: false, ..a }).collect();
        let changes = back.iter().map(|a| (a.id.clone(), false)).collect();
        self.notice = Some(format!("Showing {} app(s) again", back.len()));
        self.apps.extend(back);
        self.apps.sort_by_cached_key(sort_key);
        Action::SetHidden(changes)
    }

    pub fn pages(&self) -> usize {
        self.layout.pages(self.apps.len())
    }

    /// The apps on the current page, with their slots.
    pub fn page_apps(&self) -> impl Iterator<Item = (usize, &App)> {
        let per = self.layout.per_page();
        self.apps.iter().skip(self.page * per).take(per).enumerate()
    }

    fn turn_page(&mut self, step: i32) -> Action {
        let next = self.page as i32 + step;
        if next < 0 || next >= self.pages() as i32 {
            return Action::None;
        }
        self.page = next as usize;
        Action::Redraw { full: false }
    }

    fn go(&mut self, screen: Screen) -> Action {
        self.screen = screen;
        self.pressed = None;
        Action::Redraw { full: false }
    }

    pub fn handle(&mut self, input: Input) -> Action {
        match input {
            Input::Sleep => {
                if self.screen != Screen::Sleep {
                    self.before_sleep = self.screen;
                }
                self.screen = Screen::Sleep;
                self.pressed = None;
                return Action::Redraw { full: true };
            }
            Input::Wake => {
                // `outOfScreenSaver` and `exitingScreenSaver` both arrive: redraw once.
                if self.screen != Screen::Sleep {
                    return Action::None;
                }
                self.screen = self.before_sleep;
                return Action::Redraw { full: true };
            }
            _ if self.screen == Screen::Sleep => return Action::None,
            _ => {}
        }
        let had_notice = self.notice.take().is_some();
        let action = match self.screen {
            Screen::Home => self.home(input),
            Screen::Settings => self.settings(input),
            Screen::ConfirmKindle => self.confirm(input),
            Screen::Sleep => Action::None,
        };
        if had_notice && action == Action::None {
            Action::Redraw { full: false }
        } else {
            action
        }
    }

    fn home(&mut self, input: Input) -> Action {
        let l = self.layout;
        match input {
            Input::PageForward => self.turn_page(1),
            Input::PageBack => self.turn_page(-1),
            Input::Swipe { dx, dy } if dx.abs() > SWIPE_MIN && dx.abs() > dy.abs() * 1.5 => {
                self.turn_page(if dx < 0.0 { 1 } else { -1 })
            }
            Input::LongTap(x, y) => self.home_long_tap(x, y),
            Input::Tap(x, y) => {
                if l.status().contains(x, y) {
                    return self.go(Screen::Settings);
                }
                if l.footer().contains(x, y) {
                    return self.turn_page(if x < (l.width / 2) as f32 { -1 } else { 1 });
                }
                let Some(slot) = l.slot_at(x, y) else { return Action::None };
                let Some(app) = self.apps.get(self.page * l.per_page() + slot).cloned() else {
                    return Action::None;
                };
                match app.launch {
                    Launch::Settings => self.go(Screen::Settings),
                    Launch::KindleMode => self.go(Screen::ConfirmKindle),
                    _ => {
                        self.pressed = Some(slot);
                        Action::Launch(app)
                    }
                }
            }
            _ => Action::None,
        }
    }

    fn settings(&mut self, input: Input) -> Action {
        match input {
            Input::PageBack => self.go(Screen::Home),
            Input::Tap(x, y) => match self.layout.settings_button_at(x, y) {
                Some(Button::Done) => self.go(Screen::Home),
                Some(Button::LightDown) => Action::Device(DeviceCmd::Light(-1)),
                Some(Button::LightUp) => Action::Device(DeviceCmd::Light(1)),
                Some(Button::WarmDown) => Action::Device(DeviceCmd::Warmth(-1)),
                Some(Button::WarmUp) => Action::Device(DeviceCmd::Warmth(1)),
                Some(Button::Wifi) => Action::Device(DeviceCmd::ToggleWifi),
                Some(Button::Rotation) => Action::Device(DeviceCmd::ToggleRotation),
                Some(Button::Ssh) => Action::Device(DeviceCmd::ToggleSsh),
                Some(Button::Autostart) => Action::Device(DeviceCmd::ToggleAutostart),
                Some(Button::ShowHidden) => self.show_all(),
                Some(Button::SleepScreen) => Action::Device(DeviceCmd::ToggleSleepScreen),
                Some(Button::RotationAuto) => Action::Device(DeviceCmd::AutoRotation),
                Some(Button::KindleUi) => self.go(Screen::ConfirmKindle),
                _ => Action::None,
            },
            _ => Action::None,
        }
    }

    fn confirm(&mut self, input: Input) -> Action {
        let Input::Tap(x, y) = input else { return Action::None };
        let [(_, cancel), (_, open)] = self.layout.confirm_buttons();
        if open.inset(-12).contains(x, y) {
            Action::KindleMode
        } else if cancel.inset(-12).contains(x, y) || !self.layout.confirm_box().contains(x, y) {
            self.go(Screen::Home)
        } else {
            Action::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::IconSource;

    fn app(id: &str, launch: Launch) -> App {
        App { id: id.into(), name: id.into(), icon: IconSource::None, launch, dir: "/".into(), order: None, hidden: false }
    }

    fn script(id: &str) -> App {
        app(id, Launch::Script { path: format!("/d/{id}"), hooks: false })
    }

    fn ui(n: usize) -> Ui {
        let mut apps: Vec<App> = (0..n).map(|i| script(&format!("app{i}.sh"))).collect();
        apps.push(app("settings", Launch::Settings));
        apps.push(app("kindle", Launch::KindleMode));
        Ui::new(apps, Layout::new(1264, 1680, 3, 3))
    }

    fn centre(r: crate::Rect) -> Input {
        Input::Tap((r.x + r.w / 2) as f32, (r.y + r.h / 2) as f32)
    }

    #[test]
    fn tapping_a_tile_launches_it_and_marks_it_pressed() {
        let mut u = ui(3);
        let c = u.layout.cell(1);
        match u.handle(centre(c)) {
            Action::Launch(a) => assert_eq!(a.id, "app1.sh"),
            other => panic!("{other:?}"),
        }
        assert_eq!(u.pressed, Some(1));
        assert_eq!(u.handle(centre(u.layout.cell(8))), Action::None, "empty slot");
    }

    #[test]
    fn paging_with_buttons_swipes_and_bounds() {
        let mut u = ui(10); // 12 tiles: 2 pages
        assert_eq!(u.pages(), 2);
        assert_eq!(u.handle(Input::PageBack), Action::None);
        assert_eq!(u.handle(Input::PageForward), Action::Redraw { full: false });
        assert_eq!(u.page, 1);
        assert_eq!(u.handle(Input::PageForward), Action::None);
        assert_eq!(u.handle(Input::Swipe { dx: 300.0, dy: 20.0 }), Action::Redraw { full: false });
        assert_eq!(u.page, 0);
        assert_eq!(u.handle(Input::Swipe { dx: -100.0, dy: 0.0 }), Action::None, "too short");
        assert_eq!(u.page_apps().count(), 9);
    }

    #[test]
    fn hidden_apps_are_not_shown() {
        let mut a = script("x.sh");
        a.hidden = true;
        let u = Ui::new(vec![a, script("y.sh")], Layout::new(1264, 1680, 3, 3));
        assert_eq!(u.apps().len(), 1);
    }

    #[test]
    fn builtins_open_screens_and_confirm_leads_to_kindle_mode() {
        let mut u = ui(2); // settings at slot 2, kindle at slot 3
        assert_eq!(u.handle(centre(u.layout.cell(2))), Action::Redraw { full: false });
        assert_eq!(u.screen, Screen::Settings);
        let done = u.layout.done_button();
        u.handle(centre(done));
        assert_eq!(u.screen, Screen::Home);
        u.handle(centre(u.layout.cell(3)));
        assert_eq!(u.screen, Screen::ConfirmKindle);
        let [(_, cancel), (_, open)] = u.layout.confirm_buttons();
        u.handle(centre(cancel));
        assert_eq!(u.screen, Screen::Home);
        u.handle(centre(u.layout.cell(3)));
        assert_eq!(u.handle(centre(open)), Action::KindleMode);
    }

    #[test]
    fn status_strip_opens_settings_and_buttons_give_device_commands() {
        let mut u = ui(1);
        u.handle(Input::Tap(600.0, 40.0));
        assert_eq!(u.screen, Screen::Settings);
        let rows = u.layout.settings_rows();
        assert_eq!(u.handle(centre(rows[0].buttons[1].1)), Action::Device(DeviceCmd::Light(1)));
        assert_eq!(u.handle(centre(rows[2].buttons[0].1)), Action::Device(DeviceCmd::ToggleWifi));
        assert_eq!(u.handle(centre(rows[3].buttons[0].1)), Action::Device(DeviceCmd::ToggleRotation));
    }

    #[test]
    fn sleep_ignores_input_and_wake_restores_the_screen() {
        let mut u = ui(1);
        u.handle(Input::Tap(600.0, 40.0));
        assert_eq!(u.handle(Input::Sleep), Action::Redraw { full: true });
        assert_eq!(u.handle(Input::PageForward), Action::None);
        assert_eq!(u.handle(Input::Sleep), Action::Redraw { full: true });
        assert_eq!(u.handle(Input::Wake), Action::Redraw { full: true });
        assert_eq!(u.screen, Screen::Settings);
        assert_eq!(u.handle(Input::Wake), Action::None, "a second wake event does nothing");
    }

    #[test]
    fn a_low_battery_warning_is_reported_once_and_persists_across_input() {
        let mut u = ui(1);
        assert!(!u.set_battery(Some(90), false));
        assert_eq!(u.battery_message(), None);
        assert!(u.set_battery(Some(12), false));
        assert_eq!(u.battery_message().as_deref(), Some("Battery low 12%. Connect the charger soon."));
        assert!(!u.set_battery(Some(11), false)); // still low: no change to redraw for
        u.handle(Input::Tap(5.0, 1400.0));
        assert!(u.battery_message().is_some()); // not a notice: the next input does not clear it
        assert!(u.set_battery(Some(12), true)); // charger connected
        assert_eq!(u.battery_message(), None);
    }

    #[test]
    fn a_long_press_hides_an_app_and_show_all_brings_it_back() {
        let mut u = ui(3);
        let cell = u.layout.cell(0);
        let (x, y) = (cell.x as f32 + 5.0, cell.y as f32 + 5.0);
        let before = u.apps().len();
        let name = u.apps()[0].name.clone();
        let id = u.apps()[0].id.clone();
        let action = u.handle(Input::LongTap(x, y));
        assert_eq!(action, Action::SetHidden(vec![(id, true)]));
        assert_eq!(u.apps().len(), before - 1);
        assert_eq!(u.hidden_count(), 1);
        assert!(u.notice.as_deref().unwrap().contains(&name));
        u.screen = Screen::Settings;
        let Action::SetHidden(changes) = u.show_all() else { panic!() };
        assert_eq!(changes.len(), 1);
        assert!(!changes[0].1);
        assert_eq!(u.apps().len(), before);
        assert_eq!(u.hidden_count(), 0);
        assert_eq!(u.show_all(), Action::None);
    }

    #[test]
    fn the_kindle_ui_and_settings_cannot_be_hidden() {
        let mut u = Ui::new(
            vec![app("kindle", Launch::KindleMode), app("a.sh", Launch::Script { path: "/d/a.sh".into(), hooks: false })],
            Layout::new(1264, 1680, 3, 3),
        );
        let slot = u.layout.cell(0);
        assert_eq!(u.handle(Input::LongTap(slot.x as f32 + 5.0, slot.y as f32 + 5.0)), Action::Redraw { full: false });
        assert_eq!(u.apps().len(), 2);
        assert_eq!(u.hidden_count(), 0);
    }

    #[test]
    fn a_critical_battery_goes_to_the_sleep_screen_and_wakes_to_home() {
        let mut u = ui(1);
        assert!(u.set_battery(Some(4), false));
        assert_eq!(u.screen, Screen::Sleep);
        assert_eq!(u.battery_message().as_deref(), Some("Battery critical 4%. Connect the charger now."));
        assert_eq!(u.handle(Input::Wake), Action::Redraw { full: true });
        assert_eq!(u.screen, Screen::Home);
    }

    #[test]
    fn a_critical_battery_does_not_interrupt_settings() {
        let mut u = ui(1);
        u.screen = Screen::Settings;
        u.set_battery(Some(4), false);
        assert_eq!(u.screen, Screen::Settings);
    }

    #[test]
    fn the_autostart_row_toggles_the_boot_switch() {
        let mut u = ui(1);
        u.screen = Screen::Settings;
        let row = u.layout.settings_rows().into_iter().find(|r| r.row == crate::layout::Row::Autostart).unwrap();
        let (_, rect) = row.buttons[0];
        assert_eq!(u.handle(Input::Tap(rect.x as f32 + 10.0, rect.y as f32 + 10.0)), Action::Device(DeviceCmd::ToggleAutostart));
    }

    #[test]
    fn a_notice_is_cleared_by_the_next_input() {
        let mut u = ui(1);
        u.notice = Some("apps.toml is not valid".into());
        assert_eq!(u.handle(Input::Tap(5.0, 1400.0)), Action::Redraw { full: false });
        assert_eq!(u.notice, None);
    }
}
