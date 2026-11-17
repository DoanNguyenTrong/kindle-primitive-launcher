//! Auto-rotation: turns raw orientation readings into a stable screen rotation. A new orientation is
//! accepted only after it has been read continuously for `hold_ms`. Any reading that is not a clear
//! orientation (flat, unreadable) resets the timer, so a Kindle being tilted back and forth does not flip
//! the screen. The accelerometer decoding is not written yet: it waits for the S4 device spike.

use crate::settings::Rotation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debouncer {
    current: Rotation,
    candidate: Option<(Rotation, u64)>,
    hold_ms: u64,
}

impl Debouncer {
    pub fn new(current: Rotation, hold_ms: u64) -> Debouncer {
        Debouncer { current, candidate: None, hold_ms }
    }

    pub fn current(&self) -> Rotation {
        self.current
    }

    /// Feed one reading at `now_ms` (any monotonic clock, in milliseconds). Returns the new rotation when it
    /// changes.
    pub fn feed(&mut self, reading: Option<Rotation>, now_ms: u64) -> Option<Rotation> {
        let Some(seen) = reading else {
            self.candidate = None;
            return None;
        };
        if seen == self.current {
            self.candidate = None;
            return None;
        }
        match self.candidate {
            Some((wanted, since)) if wanted == seen => {
                if now_ms.saturating_sub(since) >= self.hold_ms {
                    self.current = seen;
                    self.candidate = None;
                    Some(seen)
                } else {
                    None
                }
            }
            _ => {
                self.candidate = Some((seen, now_ms));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Rotation::{UpsideDown, Upright};

    #[test]
    fn a_steady_turn_is_accepted_after_the_hold_time() {
        let mut d = Debouncer::new(Upright, 800);
        assert_eq!(d.feed(Some(UpsideDown), 0), None);
        assert_eq!(d.feed(Some(UpsideDown), 500), None);
        assert_eq!(d.feed(Some(UpsideDown), 800), Some(UpsideDown));
        assert_eq!(d.current(), UpsideDown);
    }

    #[test]
    fn a_flapping_reading_never_flips_the_screen() {
        let mut d = Debouncer::new(Upright, 800);
        for t in (0..5000).step_by(100) {
            let r = if (t / 100) % 2 == 0 { UpsideDown } else { Upright };
            assert_eq!(d.feed(Some(r), t), None, "flipped at {t} ms");
        }
        assert_eq!(d.current(), Upright);
    }

    #[test]
    fn an_unclear_reading_restarts_the_hold() {
        let mut d = Debouncer::new(Upright, 800);
        d.feed(Some(UpsideDown), 0);
        d.feed(Some(UpsideDown), 600);
        assert_eq!(d.feed(None, 700), None); // flat: the candidate is dropped
        assert_eq!(d.feed(Some(UpsideDown), 900), None); // a new hold starts here
        assert_eq!(d.feed(Some(UpsideDown), 1600), None);
        assert_eq!(d.feed(Some(UpsideDown), 1700), Some(UpsideDown));
    }

    #[test]
    fn the_current_orientation_cancels_a_pending_change() {
        let mut d = Debouncer::new(Upright, 800);
        d.feed(Some(UpsideDown), 0);
        d.feed(Some(Upright), 300);
        assert_eq!(d.feed(Some(UpsideDown), 400), None);
        assert_eq!(d.feed(Some(UpsideDown), 1100), None); // only 700 ms since the restart
        assert_eq!(d.feed(Some(UpsideDown), 1200), Some(UpsideDown));
    }

    #[test]
    fn a_clock_going_backwards_does_not_commit() {
        let mut d = Debouncer::new(Upright, 800);
        d.feed(Some(UpsideDown), 5000);
        assert_eq!(d.feed(Some(UpsideDown), 4000), None);
    }
}
