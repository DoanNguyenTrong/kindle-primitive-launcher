//! The launcher as the system layer over an app (`docs/system-architecture.md` in the umbrella repo).
//!
//! When the supervisor offers it (`KL_SYSTEM_LAYER=1`), opening an app does not end the launcher: the launcher asks
//! the supervisor to run the app (`SIGUSR1`) and keeps running with its drawing paused. It takes the screen back for
//! the system's own screens (quick settings, the sleep picture): the app is frozen, the screen is saved, and both are
//! put back when the system screen closes, so the app does not need to redraw.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    thread,
    time::{Duration, Instant},
};

use kl_core::{Input, Rect, Rotation};
use kl_kindle::{log, Ev};

/// How long the supervisor gets to start the app before the request is sent again, and before the launcher gives up.
const RESEND_AFTER: Duration = Duration::from_secs(3);
const GIVE_UP_AFTER: Duration = Duration::from_secs(15);

/// An app the supervisor runs while the launcher stays in charge.
pub struct AppSession {
    /// What was on the screen when the system took it (panel orientation), while a system screen is up. `None`
    /// while the app is in front.
    pub saved: Option<Vec<u8>>,
    /// The theme when the system screen opened: a change asks the app to redraw when it comes back.
    pub dark_mode_at_open: bool,
}

impl AppSession {
    pub fn new() -> AppSession {
        AppSession { saved: None, dark_mode_at_open: false }
    }

    /// The app is in front: no system screen is up.
    pub fn in_front(&self) -> bool {
        self.saved.is_none()
    }
}

/// The supervisor offers the system layer.
pub fn enabled() -> bool {
    std::env::var("KL_SYSTEM_LAYER").as_deref() == Ok("1")
}

/// Ask the supervisor to run the app described in `run/next`. False when the supervisor cannot be reached.
pub fn request_launch(run: &Path) -> bool {
    let Some(pid) = read_pid(&run.join("supervisor.pid")) else {
        log("system layer: no supervisor.pid");
        return false;
    };
    // SAFETY: kill takes no pointers; SIGUSR1 is the supervisor's "run the app" request.
    let sent = unsafe { libc::kill(pid, libc::SIGUSR1) } == 0;
    if !sent {
        log(&format!("system layer: could not signal the supervisor (pid {pid})"));
    }
    sent
}

/// Follow the app the supervisor runs: wait for `run/app.pid` to appear (asking again once if it is slow), then for it
/// to go, and send `Ev::AppEnded`.
pub fn watch(run: PathBuf, tx: Sender<Ev>) {
    thread::spawn(move || {
        let pid_file = run.join("app.pid");
        let started = Instant::now();
        let mut asked_again = false;
        while !pid_file.exists() {
            if !asked_again && started.elapsed() > RESEND_AFTER && run.join("next").exists() {
                log("system layer: the app has not started; asking the supervisor again");
                request_launch(&run);
                asked_again = true;
            }
            if started.elapsed() > GIVE_UP_AFTER {
                log("system layer: the supervisor did not start the app");
                let _ = tx.send(Ev::AppEnded);
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
        while pid_file.exists() {
            thread::sleep(Duration::from_millis(300));
        }
        let _ = tx.send(Ev::AppEnded);
    });
}

/// Freeze the app (`true`) while a system screen is up, or let it run again (`false`).
pub fn freeze(run: &Path, on: bool) {
    signal_app(run, if on { libc::SIGSTOP } else { libc::SIGCONT });
}

/// Ask the app to draw its whole screen again (the theme changed while it was frozen). Only an app that declares
/// `# Wake: redraw` gets the signal (the supervisor marks it with `run/app-redraws`): SIGUSR1 ends any other process.
/// Returns whether the app was asked.
pub fn ask_redraw(run: &Path) -> bool {
    if !run.join("app-redraws").exists() {
        return false;
    }
    signal_app(run, libc::SIGUSR1);
    true
}

/// Signal the app's process group (the supervisor starts apps in their own group), or the app alone without one.
fn signal_app(run: &Path, signal: libc::c_int) {
    let Some(pid) = read_pid(&run.join("app.pid")) else { return };
    // SAFETY: kill takes no pointers. A negative pid addresses the process group the app leads.
    if unsafe { libc::kill(-pid, signal) } != 0 {
        // SAFETY: as above, for the app's own process.
        unsafe { libc::kill(pid, signal) };
    }
}

fn read_pid(path: &Path) -> Option<i32> {
    fs::read_to_string(path).ok()?.trim().parse().ok().filter(|pid: &i32| *pid > 1)
}

/// The swipe that opens quick settings: down from the top edge, as on the home screen (`Ui::home`).
pub fn opens_quick_settings(input: Input, status_bottom: i32) -> bool {
    matches!(input, Input::Swipe { dx, dy, start_y }
        if dy > 150.0 && dy.abs() > dx.abs() * 1.5 && start_y <= status_bottom as f32 + 40.0)
}

/// Draw the app under a system panel: every pixel of `frame` (upright) outside `panel` comes from `saved` (the app's
/// screen in panel orientation), so the panel floats over the app instead of over the home screen.
pub fn app_under(frame: &mut [u8], saved: &[u8], width: usize, panel: Rect, rotation: Rotation, fb_turn: u32) {
    if frame.len() != saved.len() || width == 0 {
        return;
    }
    // The panel orientation is upright or a half turn; a half turn is its own inverse.
    let upright: Vec<u8> = if (rotation.turn() + fb_turn) % 4 == 2 {
        saved.iter().rev().copied().collect()
    } else {
        saved.to_vec()
    };
    let rows = frame.len() / width;
    let (x0, x1) = (panel.x.clamp(0, width as i32) as usize, panel.right().clamp(0, width as i32) as usize);
    let (y0, y1) = (panel.y.clamp(0, rows as i32) as usize, panel.bottom().clamp(0, rows as i32) as usize);
    for y in 0..rows {
        let row = y * width..(y + 1) * width;
        if y < y0 || y >= y1 {
            frame[row.clone()].copy_from_slice(&upright[row]);
        } else {
            frame[y * width..y * width + x0].copy_from_slice(&upright[y * width..y * width + x0]);
            frame[y * width + x1..(y + 1) * width].copy_from_slice(&upright[y * width + x1..(y + 1) * width]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_shows_outside_the_panel_and_the_panel_stays() {
        let width = 4;
        let mut frame = vec![1u8; 16];
        let saved = vec![9u8; 16];
        app_under(&mut frame, &saved, width, Rect { x: 0, y: 0, w: 4, h: 2 }, Rotation::Upright, 0);
        assert_eq!(&frame[..8], &[1u8; 8], "the panel (top two rows) is the launcher's");
        assert_eq!(&frame[8..], &[9u8; 8], "below the panel is the app");
    }

    #[test]
    fn a_half_turn_puts_the_app_back_upright() {
        let width = 2;
        let mut frame = vec![0u8; 4];
        let saved = vec![1, 2, 3, 4];
        app_under(&mut frame, &saved, width, Rect { x: 0, y: 0, w: 0, h: 0 }, Rotation::UpsideDown, 0);
        assert_eq!(frame, vec![4, 3, 2, 1]);
    }

    #[test]
    fn only_a_swipe_down_from_the_top_opens_quick_settings() {
        assert!(opens_quick_settings(Input::Swipe { dx: 10.0, dy: 400.0, start_y: 20.0 }, 120));
        assert!(!opens_quick_settings(Input::Swipe { dx: 10.0, dy: 400.0, start_y: 600.0 }, 120), "mid-screen is the app's");
        assert!(!opens_quick_settings(Input::Swipe { dx: 400.0, dy: 10.0, start_y: 20.0 }, 120), "sideways is the app's");
        assert!(!opens_quick_settings(Input::Tap(10.0, 10.0), 120));
    }
}
