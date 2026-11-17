# Device notes (Phase 0 spikes)

Kindle Oasis 3, firmware 5.18.2, kernel 4.1.15-lab126, upstart 0.6.6. Checked over SSH on 2026-11-17.

## Baseline with the stock UI running (S1, first half)

- Memory: 493,696 kB total, ~14 MB free, **~199 MB available**, 131 MB swap (86 MB used).
- `/var` is a 64 MB tmpfs (8.6 MB used). `/mnt/us` 6.1 GB, 5.2 GB free.
- Largest processes (RSS): cvm 61 MB, KPPMainApp 49 MB, pillowd 26 MB, mesquite 16 MB, kb 8 MB,
  Xorg 7.6 MB, blanket 6 MB, awesome 6 MB, statusbar 5 MB, fastmetrics 5 MB, powerd 4 MB.
- Input and screen holders: Xorg holds `event3`, `event7`, `event8` and `fb0`; awesome and blanket hold `fb0`.
  Primitive Browser reads the same devices alongside Xorg, so Xorg does not grab them exclusively.

## Upstart (S6)

- `lab126_gui` starts on `n_ready and langpicker_ready and ekart_ready` and stops on `stopping x or ota-update`.
- Jobs that stop with `lab126_gui`: framework (cvm), pillow, kb, statusbar, splash, webreader,
  contentpackd, kfxreader, kfxview, kppmainapp, whisperstore, progressivedownloads, chk_enc_errors.
  Jobs that stop with `framework`: framework_setup, fastmetrics, juno-acs-helper, kmc, poll_daemons,
  scanner, stored.
- **X does not stop with `lab126_gui`**: `x` stops only on `stopping dbus or stop_x`. Xorg, and probably
  awesome and blanket, keep running after the framework stops (to confirm in S1).
- `framework.conf` pre-start: if **`/mnt/us/DONT_START_FRAMEWORK`** exists, the framework job does not
  start. This is a stock switch, a candidate for M6c later.
- `lab126_gui` pre-start counts failed boots (`MAX_REBOOTS=2`) and, past the limit, sets airplane mode and
  CVM debugging. We never make the framework fail, we only stop it, so this counter is not touched.

## Clock (S8)

- `date` and `hwclock` agree (RTC kept in UTC; local zone GMT+7).
- Network time tools present: `/usr/bin/ntpd`, `/usr/bin/ntpdate`, `/usr/sbin/rdate`, `/usr/bin/sntp`.

## KOReader (S7, from reading the scripts)

- The library entry `documents/KOReader.sh` runs `/var/local/kmc/bin/kpm launch koreader --asap`.
- `koreader.sh` with no flag or `--asap` assumes the framework is running. It tries to disable
  pillow, SIGSTOPs awesome and stops `stored webreader kfxreader kfxview todo tmd rcm archive scanner otav3
  otaupd`. On exit it **starts those services again**, restores a framebuffer dump and asks appmgrd to open
  the library. Under a stopped framework that would start framework services behind the launcher.
- `koreader.sh --framework_stop` runs `stop lab126_gui` first and `start lab126_gui` on exit.
- KOReader's Lua (`frontend/device/kindle/device.lua`, `frameworkStopped()`), when `STOP_FRAMEWORK=yes`:
  registers the lipc property `com.lab126.kaf frameworkStarted = 1` and unloads blanket's `splash` and
  `screensaver` modules. Blanket keeps running when the framework stops and otherwise draws the stock
  screensaver on sleep.
- `koreader.sh` also SIGSTOPs volumd itself and resumes it on exit, removes the system passcode file while
  running, and opens UDP 8134 in iptables.
- **Plan:** run `koreader.sh --framework_stop` with a PATH shim that makes `start lab126_gui` and
  `stop lab126_gui` do nothing (see 02-architecture.md §2.5). KOReader then behaves as in its own
  framework-stopped mode, and the framework stays stopped when it exits.

## Primitive Browser

- Library entry: `documents/Primitive Browser.sh` (the name has a space). Default mode `keep`: it disables
  pillow on start; on exit it enables pillow and retries `appmgrd start app://com.lab126.booklet.home` for up
  to 10 s. Mode `stop` (flag file `stop-framework`): `stop lab126_gui` / `start lab126_gui`.
- The SSH server (dropbear, `/tmp/dropbear_browser.pid`) runs under init (ppid 1) and is stopped only from
  the browser's Diagnostics page, so it outlives the browser.

## Framework stopped (S1 second half, 2026-11-17, Primitive Browser still running in front)

- `stop lab126_gui` plus the 1.25 s pause took 5 s.
- **MemAvailable 198 MB -> 336 MB (+138 MB)**; SwapFree 45 MB -> 108 MB.
- Still running: Xorg (holds event3/7/8 and fb0), awesome, blanket (fb0), powerd, wifid and
  wpa_supplicant, appmgrd, volumd, lipc-daemon, dbus, fastmetrics, todo, tmd, archive, rcm.
- powerd stays `Active` with its idle timer counting; Wi-Fi stays `CONNECTED`; blanket still has
  `screensaver langpicker blankwindow usb` loaded.

## Sleep with the framework stopped (S2, first attempt)

What the user saw (browser in front, framework stopped):
- **Power button: nothing happened** (pressed several times; the device did not sleep).
- **Cover closed: the stock sleep screen appeared** (blanket's screensaver module), so cover-driven
  suspend still works. **Cover opened: blank screen** (blanket removed its screensaver; nothing redrew).
- Closing the browser left a blank screen: Browser.sh in `keep` mode tries pillow and appmgrd, which are
  gone, and nothing else draws.
- **Wi-Fi did not come back after waking**: no ping for over a minute; the device had to be rebooted.

Open questions for the log (`/mnt/us/kl-s2.txt`, read after the reboot):
- Did powerd emit `goingToScreenSaver` on the power button? If yes, powerd waits for someone (the
  framework) to answer before it suspends; KOReader's `frameworkStarted` registration may be that answer.
- Did the device actually suspend after the cover, and did it resume, or was it still suspended with a
  blank screen?
- Is Wi-Fi reconnect after resume done by the framework (cvm), so the launcher must ask wifid to
  reconnect?

Consequence: Gate 0's stop condition is not hit yet, but the launcher must do what KOReader does in its
framework-stopped mode before any further sleep test (register `com.lab126.kaf frameworkStarted`, unload
blanket's screensaver, draw its own sleep screen, redraw on wake, and check Wi-Fi after resume).

## To do on the device

S1 second half (stop the framework: what is left, memory, timing), S2 sleep, S3 Wi-Fi, S4 accelerometer,
S5 USB, S7 KOReader run, S9 Kindle mode round trip.

## Sleep test with the frameworkStarted holder (S2b, 2026-11-17 22:28–22:40)

Stock UI stopped; a stock-Lua process held `com.lab126.kaf frameworkStarted = 1` (the same property
KOReader sets with `STOP_FRAMEWORK=yes`). Log: `/mnt/us/kl-s2b.txt` on the Kindle.

- **Power button sleep works with the holder:** `powerButtonPressed` at 22:28:50 -> `goingToScreenSaver 2`
  (BUTTON_SUSPEND). Without the holder (first attempt) the button produced `powerButtonPressed` but no
  `goingToScreenSaver`. The holder is the difference.
- Idle countdown `readyToSuspend 10..1` then `suspending "mem"` at 22:30:45 (~2 min after the button).
- **Wi-Fi drops while suspended:** `cmDisconnected`, `cmStateChange NA`, `cmIntfNotAvailable` at 22:30:25.
- **Power button wake works:** `powerButtonPressed` 22:37:26 -> `wakeupFromSuspend 400`,
  `outOfScreenSaver 1` (BUTTON_WAKEUP). Wi-Fi reconnected by 22:37:32 without any action from us.
- The 4-minute restore (22:39:27) ran `start lab126_gui`: the framework came up, but the screen stayed
  blank until the library was opened. Browser.sh opens it with
  `lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home`; a bare `start` does not.
  Library was up afterwards (opened by the user / that call).
- The holder ran as stock Lua: `/usr/bin/lua` plus `/usr/lib/lua/liblipclua.so` (lipc `init`,
  `register_int_property`). No extra binary needed on the device.

Conclusion: the holder makes the power button sleep and wake work with the framework stopped. Next:
run it from the supervisor for the whole session, and redraw the launcher's own screen on wake.

## Automated device test, v0.1.1 (2026-11-17, user away; log on the Kindle at /mnt/us/kl-autotest.txt)

Deployed with `scripts/build-device.sh` (checksums matched). Results:

- **Start through `Launcher.sh`: PASS.** Framework stopped in 3 s; `volumd` paused (state T); the launcher
  drew its first screen in 773 ms; heartbeat advancing.
- **Holder: FAIL at first, fixed.** The supervisor logged "holder not started: lua or holder.lua missing":
  `install-ssh.sh` did not upload `holder.lua`. Fixed in the source (uploaded by hand for this test). The next
  build installs it.
- **Power button sleep with the holder: PASS.** `goingToScreenSaver 2` (BUTTON_SUSPEND) and powerd state
  `screenSaver`; the launcher's sleep screen was in front.
- **RTC wake timer: not available.** `com.lab126.powerd rtcWakeup` gives `lipcErrNoSuchProperty` on 5.18.2,
  so an automatic wake is not possible. Wake by button (suspend and wake, Wi-Fi reconnect) is still untested;
  it needs a press.
- **Supervisor killed (SIGTERM): PASS.** The stock framework came back by itself (`start/running`), the
  launcher exited, the holder was released (property 0), `volumd` resumed (state S). This is the S9 recovery
  path, not a full round trip through the library book.
- **S4 at rest: no events on `/dev/input/event7`** in 5 s. The accelerometer needs a tilt to report; open.

Not tested automatically: tapping through the grid, opening an app and returning, Kindle mode (exit 0 needs a
tap), USB (S5), KOReader with `--framework_stop` (S7), the critical-battery sleep, and edit mode.

## Kindle UI return: "application error" and stuck screen (2026-11-18, v0.1.4–v0.1.6)

Symptom: after Settings > Kindle UI, the stock UI came back but the library showed "application error",
then a stuck or blank screen. A plain `stop lab126_gui; start lab126_gui` over SSH did not show it.

What the logs showed (`/var/log/messages`):
- **Root cause: appmgrd.** The library opens "Launcher" through KPM's script runner
  (`tech.hackerdude.shell_integration.launcher`, command `/var/local/kmc/bin/sh_integration_launcher`, an app
  of appmgrd). `Launcher.sh` used to start the supervisor and return at once. appmgrd then switched back to the
  library (`go` to KPPMainApp) at the moment the supervisor stopped the framework. KPPMainApp never answered:
  `Timed out waiting for sync prop access ... property=go`, `Sent SIGQUIT to CVM`, `default_alert_displayed`,
  `QueueUnload:reason=handleFailure`. After the framework restarted, appmgrd kept the home app as `PAUSED` and
  ignored loading it (`applicationAlreadyLoadedOrExecuted:state=PAUSED,ignoring=LOAD`): stuck screen. The alert
  queued earlier is the "application error".
- **Fix (v0.1.7):** `Launcher.sh` stays running until the supervisor ends (the supervisor is still detached), as
  KOReader's library entry does. appmgrd only goes back to the library after the framework is restarted.
- **Side effect seen once: a wedged `lipc-daemon`.** After repeated failures, every script `lipc-daemon` forked
  for `com.lab126.system` (e.g. `usid`) deadlocked, so the framework never became ready. `restart lipcd` fixed
  it. The supervisor now checks `usid` before starting the framework and restarts `lipcd` if it does not
  answer (v0.1.6).
- **Not the cause:** the frameworkStarted holder (the error happened with it disabled; its lipc name is released
  when it exits) and pausing `volumd` (the framework became ready after a 10 s pause).
- **Timing:** `start lab126_gui` returns after ~9 s, but the framework emits `framework_ready` ~40 s after the
  start; only then does the library app (`kppmainapp` job) start.
- Each library crash leaves a ~5 MB `KPPMainAppV2_*_crash_*.tgz` in `/mnt/us/documents`.

## Clock showed UTC (fixed in v0.1.7)

`/etc/localtime` links to `/var/local/system/tz`, a TZif v2 file with an empty POSIX footer. musl (the launcher's
libc) treats times after the last transition as UTC when the footer is empty; glibc keeps the last transition's
offset. The launcher now reads the offset from the file itself (`kl_core::tz`), +7 h on this device.

## Kindle UI return works (v0.1.10, 2026-11-18 01:20)

Two more causes were found and fixed after the appmgrd race:
- **v0.1.7 regression:** `Launcher.sh` set `trap '' TERM` before starting the supervisor. A signal ignored when a
  shell starts cannot be trapped again, so the supervisor and the holder inherited TERM as ignored, the holder
  survived its stop and kept `com.lab126.kaf`; the restarted framework failed with "Duplicate service name
  (com.lab126.kaf)" / "Cannot register framework with Lipc". Fixed in v0.1.8 (trap set after the start; the
  holder stop falls back to KILL).
- **Hand-back too early:** appmgrd sends `load` to the library app as soon as the script runner ends. Ending when
  the `kppmainapp` job ran was too early (`ServiceUnknown`), and ending when `com.lab126.KPPMainApp` was listed
  was still too early (status 8, no such property). v0.1.10 waits until the library app offers its `load`
  property, then 3 s.

Result (log 01:19:44–01:20:51): framework start 9 s, library ready 49 s later, appmgrd `LOAD_SENT → LOADED →
GO_SENT → STARTED`, no alert; the library appears about 65 s after tapping Kindle UI. The launcher's last screen
stays up during the wait.
