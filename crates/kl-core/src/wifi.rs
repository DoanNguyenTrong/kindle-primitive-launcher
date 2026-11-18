//! Wi-Fi signal strength for the status bar. `com.lab126.wifid signalStrength` reads like `4/5` (S3,
//! device-notes.md); the Wi-Fi icon has three arcs above its dot.

/// `signalStrength` ("4/5") as (level, out of).
pub fn parse_signal(text: &str) -> Option<(u32, u32)> {
    let (level, max) = text.trim().split_once('/')?;
    let (level, max) = (level.trim().parse().ok()?, max.trim().parse().ok()?);
    (max > 0 && level <= max).then_some((level, max))
}

/// How many of the icon's three arcs are lit for a signal: none for no signal, at least one for any.
pub fn arcs(level: u32, max: u32) -> usize {
    if level == 0 || max == 0 {
        return 0;
    }
    ((level * 3).div_ceil(max) as usize).clamp(1, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_wifid_value() {
        assert_eq!(parse_signal("4/5\n"), Some((4, 5)));
        assert_eq!(parse_signal("0/5"), Some((0, 5)));
        assert_eq!(parse_signal("6/5"), None);
        assert_eq!(parse_signal(""), None);
        assert_eq!(parse_signal("x/5"), None);
    }

    #[test]
    fn arcs_follow_the_level() {
        let lit: Vec<usize> = (0..=5).map(|l| arcs(l, 5)).collect();
        assert_eq!(lit, [0, 1, 2, 2, 3, 3]);
    }
}
