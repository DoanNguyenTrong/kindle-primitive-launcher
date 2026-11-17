//! The Kindle's controls through the property bus (`lipc-get-prop` / `lipc-set-prop`), the power
//! events, and a few files from /proc and /sys. Property names follow Primitive Browser 0.2.12
//! (`crates/kb-kindle/src/device.rs`), which uses the ones KOReader uses on this firmware. Nothing here
//! is fatal: a value that cannot be read shows as unknown.

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
    time::Duration,
};

use kl_core::{rotation::Debouncer, Input, Rotation, Settings, Status};

use crate::{log, profile, Ev};

const POWERD: &str = "com.lab126.powerd";
const WIFID: &str = "com.lab126.wifid";
const CMD: &str = "com.lab126.cmd";
pub const LEVEL_MAX: u32 = 24;

fn get_int(service: &str, prop: &str) -> Option<u32> {
    let out = Command::new("lipc-get-prop").args(["-i", service, prop]).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn get_text(service: &str, prop: &str) -> String {
    Command::new("lipc-get-prop")
        .args([service, prop])
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn set_int(service: &str, prop: &str, value: u32) -> bool {
    let ok = Command::new("lipc-set-prop")
        .args(["-i", service, prop, &value.to_string()])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    log(&format!("set {service} {prop} {value}: {}", if ok { "ok" } else { "failed" }));
    ok
}

fn sysfs_text(path: &str) -> String {
    std::fs::read_to_string(path).map(|t| t.trim().to_string()).unwrap_or_default()
}

/// The local time and date, e.g. "10:38" and "Mon 16 Nov".
pub fn local_time() -> (String, String, i32) {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    // SAFETY: `time`, `gmtime_r` and `localtime_r` only write into the values passed here.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        // The zone offset comes from /etc/localtime directly: musl reads the Kindle's file as UTC
        // (kl_core::tz). localtime_r is the fallback for a file that cannot be read.
        let offset = std::fs::read("/etc/localtime").ok().and_then(|d| kl_core::tz::offset_at(&d, now as i64));
        let done = match offset {
            Some(off) => {
                let shifted = now + off as libc::time_t;
                !libc::gmtime_r(&shifted, &mut tm).is_null()
            }
            None => !libc::localtime_r(&now, &mut tm).is_null(),
        };
        if !done {
            return ("--:--".into(), String::new(), -1);
        }
        let clock = format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        let day = DAYS.get(tm.tm_wday as usize).copied().unwrap_or("");
        let month = MONTHS.get(tm.tm_mon as usize).copied().unwrap_or("");
        (clock, format!("{day} {} {month}", tm.tm_mday), tm.tm_hour * 60 + tm.tm_min)
    }
}

fn battery() -> Option<u32> {
    get_int(POWERD, "battLevel").or_else(|| sysfs_text(profile::ACTIVE.battery_capacity).parse().ok())
}

fn charging() -> bool {
    get_int(POWERD, "isCharging") == Some(1)
        || sysfs_text(profile::ACTIVE.charger_charging) == "1"
        || sysfs_text(profile::ACTIVE.charger_status) == "Charging"
}

fn mem_available() -> String {
    let info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    info.lines()
        .find(|l| l.starts_with("MemAvailable:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|kb| kb.parse::<u64>().ok())
        .map(|kb| format!("MemAvailable {} MB", kb / 1024))
        .unwrap_or_default()
}

/// Whether the SSH server started by `ssh.sh` (Primitive Browser's dropbear) is running.
fn ssh_running() -> bool {
    std::fs::read_to_string("/tmp/dropbear_browser.pid")
        .ok()
        .and_then(|p| p.trim().parse::<i32>().ok())
        // SAFETY: signal 0 only checks that the process exists.
        .is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)
}

pub fn status(settings: &Settings) -> Status {
    let rotation = settings.rotation;
    let sleep_launcher = settings.sleep_screen_launcher;
    let (clock, date, _) = local_time();
    let wifi_on = get_int(WIFID, "enable").is_some_and(|v| v != 0);
    let state = get_text(WIFID, "cmState");
    let wifi_state = match (wifi_on, state.to_ascii_uppercase().as_str()) {
        (false, _) => "off".to_string(),
        (true, "CONNECTED") => "connected".to_string(),
        (true, "") => String::new(),
        (true, _) => "not connected".to_string(),
    };
    Status {
        clock,
        date,
        battery: battery(),
        charging: charging(),
        autostart: !Path::new(&launcher_dir()).join("DISABLE").exists(),
        sleep_launcher,
        auto_rotation: settings.auto_rotation,
        wifi_on,
        wifi_state,
        light: get_int(POWERD, "flIntensity").unwrap_or(0).min(LEVEL_MAX),
        warmth: get_int(POWERD, "currentAmberLevel").unwrap_or(0).min(LEVEL_MAX),
        ssh: ssh_running(),
        rotation,
        memory: mem_available(),
    }
}

pub fn set_light(level: u32) {
    set_int(POWERD, "flIntensity", level.min(LEVEL_MAX));
}

pub fn set_warmth(level: u32) {
    set_int(POWERD, "currentAmberLevel", level.min(LEVEL_MAX));
}

pub fn set_wifi(on: bool) {
    set_int(CMD, "wirelessEnable", u32::from(on));
}

/// Start or stop SSH with the launcher's `ssh.sh` (it uses Primitive Browser's dropbear).
/// The device's turn from the framework's accelerometer (`U` upright, `D` upside down). Landscape and
/// unreadable values give nothing. The browser reads the same property.
pub fn accelerometer_turn() -> Option<Rotation> {
    let out = Command::new("lipc-get-prop").args(["com.lab126.winmgr", "accelerometer"]).output().ok()?;
    match String::from_utf8_lossy(&out.stdout).trim() {
        "U" => Some(Rotation::Upright),
        "D" => Some(Rotation::UpsideDown),
        _ => None,
    }
}

/// Follow the device's turn once a second. A new turn is sent after it has been read for 800 ms in a row
/// (`kl_core::rotation`); a flat or unreadable reading starts the wait again.
pub fn orientation_loop(tx: Sender<Ev>, start: Rotation) {
    let mut debounce = Debouncer::new(start, 800);
    let began = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let now = began.elapsed().as_millis() as u64;
        if let Some(turn) = debounce.feed(accelerometer_turn(), now) {
            if tx.send(Ev::Turn(turn)).is_err() {
                return;
            }
        }
    }
}

/// Unload the stock screensaver (blanket's module), so the launcher's own sleep screen is what shows on
/// sleep. KOReader does the same in its framework-stopped mode. It stays unloaded until blanket restarts
/// (Kindle mode or a reboot).
pub fn unload_stock_screensaver() {
    let ok = Command::new("lipc-set-prop")
        .args(["com.lab126.blanket", "unload", "screensaver"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    log(&format!("stock screensaver unloaded: {ok}"));
}

/// The launcher's folder (`KL_DIR`, default /mnt/us/launcher).
pub fn launcher_dir() -> String {
    std::env::var("KL_DIR").unwrap_or_else(|_| "/mnt/us/launcher".into())
}

/// Autostart on: the boot guard may start the launcher. Off: the boot guard leaves the stock UI alone (the
/// DISABLE file). Turning it on also clears the failed-boot count, so the guard counts from zero.
pub fn set_autostart(dir: &str, on: bool) {
    let disable = Path::new(dir).join("DISABLE");
    if on {
        let _ = std::fs::remove_file(&disable);
        let _ = std::fs::remove_file(Path::new(dir).join("boot-attempts"));
    } else {
        let _ = std::fs::write(&disable, b"");
    }
    log(&format!("autostart {}", if on { "on" } else { "off" }));
}

pub fn set_ssh(dir: &str, on: bool) {
    let script = format!("{dir}/ssh.sh");
    let result = Command::new("/bin/sh")
        .args([script.as_str(), if on { "start" } else { "stop" }])
        .stdin(Stdio::null())
        .output();
    match result {
        Ok(out) => log(&format!("ssh.sh {}: {} {}", if on { "start" } else { "stop" }, out.status, String::from_utf8_lossy(&out.stdout).trim())),
        Err(e) => log(&format!("ssh.sh: {e}")),
    }
}

/// Map a `lipc-wait-event` line to an input. Lines look like `goingToScreenSaver 2`, or with the source
/// first (`com.lab126.powerd goingToScreenSaver 2`) in some modes, so any word may be the event.
pub fn power_event(line: &str) -> Option<Input> {
    line.split_whitespace().find_map(|word| match word {
        "goingToScreenSaver" => Some(Input::Sleep),
        "outOfScreenSaver" | "exitingScreenSaver" => Some(Input::Wake),
        _ => None,
    })
}

/// Follow the power events. `lipc-wait-event -m` runs as a child; if it ends, it is started again after
/// a pause. Off the Kindle it fails at once, and after a few tries the thread gives up.
pub fn power_loop(tx: Sender<Ev>) {
    let mut failures = 0;
    loop {
        let child = Command::new("lipc-wait-event")
            .args(["-m", POWERD, "goingToScreenSaver,outOfScreenSaver,exitingScreenSaver"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(mut child) => {
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        log(&format!("power: {line}"));
                        failures = 0;
                        if let Some(input) = power_event(&line) {
                            let _ = tx.send(Ev::Input(input));
                        }
                    }
                }
                let _ = child.wait();
            }
            Err(e) => log(&format!("lipc-wait-event: {e}")),
        }
        failures += 1;
        if failures >= 5 {
            return log("power events unavailable; giving up");
        }
        thread::sleep(Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_lines_map_to_inputs() {
        assert_eq!(power_event("goingToScreenSaver 2"), Some(Input::Sleep));
        assert_eq!(power_event("outOfScreenSaver 1"), Some(Input::Wake));
        assert_eq!(power_event("exitingScreenSaver"), Some(Input::Wake));
        assert_eq!(power_event("com.lab126.powerd goingToScreenSaver 2"), Some(Input::Sleep));
        assert_eq!(power_event("t1TimerReset"), None);
        assert_eq!(power_event(""), None);
    }
}
