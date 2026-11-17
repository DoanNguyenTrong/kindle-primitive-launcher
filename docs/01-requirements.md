# 1. Requirements

Status date: 2026-11-17. Nothing is implemented yet. Working name: **Primitive Launcher** (folder
`doannt-kindle-launcher`); the name can change.

## 1.1 Purpose

The launcher is the interface of the user's jailbroken Kindle Oasis 3 (1264 × 1680 e-ink, firmware
5.18.2). It replaces the stock Kindle UI for daily use: one home screen with every application
(Primitive Browser, KOReader, KUAL extensions, ...). The stock framework is stopped while the launcher
runs. It stays installed and unmodified as **Kindle mode**, an escape hatch that the user can open
from the launcher and that every failure falls back to.

The user does not use Amazon-purchased books, Send-to-Kindle or Whispersync. Books are read in
KOReader. Background and the choice of approach: [00-research.md](00-research.md).

## 1.2 Users and devices

- One user (the owner), one device: Kindle Oasis 3, firmware 5.18.2, upright and upside-down portrait.
- Other kindlehf Kindles are not a goal; device facts stay in one profile.

## 1.3 Acceptance criteria

| ID | User outcome |
|---|---|
| AC-1 | The launcher shows every installed app as a tile (icon + name) on one or more pages. |
| AC-2 | Tapping a tile opens the app; leaving the app returns to the launcher. |
| AC-3 | **Kindle mode**: one tile opens the stock UI (≤ 15 s); the `Launcher` book in the stock library returns to the launcher (≤ 5 s). |
| AC-4 | Sleep and wake work as on a stock Kindle: power button, sleep cover, idle timeout. While the launcher is in front, it shows its sleep screen, and wake returns to the launcher. |
| AC-5 | Wi-Fi can be switched on and off from the launcher, and it connects to networks saved earlier. Its state is visible. |
| AC-6 | The launcher and its apps follow the Kindle's turn (upright / upside down) automatically, or by a manual toggle if automatic detection is unreliable. |
| AC-7 | The user can connect the Kindle to a computer as a USB drive from the launcher, without crashing a running program. |
| AC-8 | (Phase 3, opt-in) After a reboot the user lands on the launcher without touching anything. |
| AC-9 | No launcher failure leaves the Kindle unusable. A crash, hang, kill, missing file or bad configuration ends at the stock UI automatically. Autostart can be disabled from USB storage alone, without SSH. |
| AC-10 | Navigation does not flash. One full refresh is allowed when the launcher (re)takes the screen. |
| AC-11 | Apps added the existing ways (scriptlets in `documents/`, KUAL extensions in `extensions/`) appear without editing launcher config. |
| AC-12 | Memory available to apps with the launcher in place is measured and higher than with the stock UI running (the number is recorded, not promised). |

## 1.4 Functional requirements

| ID | Requirement | Phase |
|---|---|---|
| FR-1 | **App grid.** Portrait grid of tiles (default 3 × 3), paged with swipe and the page buttons. Tile = icon (2:3 book-cover ratio, or a generated monogram) + name. | 1 |
| FR-2 | **Discovery.** Sources, merged in this order: (a) built-ins (Kindle mode, Settings); (b) scriptlets `/mnt/us/documents/*.sh` (`# Name`, `# Icon`, `# Author`, `# UseHooks`); (c) KUAL extensions `/mnt/us/extensions/*/config.xml` + JSON menu, as one "Extensions" tile with a KUAL-style list; (d) `apps.toml` entries and overrides (name, icon, order, hidden, command, mode). `Launcher.sh` itself is never listed. | 1 (a, b, d), 4 (c) |
| FR-3 | **Launch.** The supervisor runs the app with `PRIMITIVE_LAUNCHER=1` and `PRIMITIVE_FRAMEWORK=stopped` in its environment, then starts the launcher again when the app exits. Modes: `screen` (default) and `background` (KUAL items with `exitmenu: false`; the launcher stays in front and shows the last output line). | 1 |
| FR-4 | **Framework control.** On entry (from `Launcher.sh` or boot), the supervisor stops `lab126_gui` and records that in a marker. Kindle mode, giving up after crashes, and the watchdog all start it again. | 1 |
| FR-5 | **Sleep.** The launcher handles `powerd` events. While in front, `goingToScreenSaver` draws the sleep screen (an image from `screensavers/`, or the default); `outOfScreenSaver` redraws the launcher. User input resets the idle timer if `powerd` needs that without the framework (S2). Apps handle sleep themselves while they run. | 1 |
| FR-6 | **Status strip.** Clock, battery (with charging), Wi-Fi state. | 2 |
| FR-7 | **Quick settings.** Frontlight, warmth, Wi-Fi on/off, rotation (auto / upright / upside down), USB drive mode, Kindle mode, SSH on/off (for development; off by default). | 2 |
| FR-8 | **Rotation service.** Detect the turn from the accelerometer device with debouncing (S4), or use the manual setting. Publish it in `/var/tmp/kl/orientation` (`U` or `D`) so apps can use it when the framework property is absent. | 2 |
| FR-9 | **USB.** Keep `/mnt/us` from being taken while programs run from it. The launcher offers "Connect to computer": it stops the running app, enters USB drive mode, shows a "USB drive" screen, and returns when the cable is removed (S5). | 2 |
| FR-10 | **Low battery.** A notice at 10 % and 5 %; a sleep screen telling the user to charge when `powerd` reports critical. | 2 |
| FR-11 | **Clock.** Use the hardware clock; optionally set the time from the network after Wi-Fi connects (S8). | 2 |
| FR-12 | **Boot start (opt-in).** A guarded upstart job starts the supervisor after the framework has started, and the supervisor stops the framework. It can be disabled by a file on USB storage, and it disables itself after repeated failed starts. | 3 |
| FR-13 | **Edit mode.** Long-press a tile to move it, hide it or set its icon; saved to `apps.toml`. | 4 |
| FR-14 | **Logs and diagnostics.** Bounded log on `/mnt/us/launcher/`, crash-signal logging, an "About" page with version, memory figures and the discovered app list. | 1 |

## 1.5 Non-functional requirements

| ID | Requirement | Target |
|---|---|---|
| NF-1 | First full screen after start, cold, from `Launcher.sh` (framework stop excluded). | ≤ 1.5 s |
| NF-2 | Back to the launcher after an app exits. | ≤ 1 s to first paint |
| NF-3 | Launcher memory (RSS) while in front. | ≤ 25 MB. While an app runs, only the supervisor shell remains. |
| NF-4 | No-flash drawing per the `kindle-eink` skill: pure black text on white, icons quantised to 16 grays and dithered, partial updates for navigation. | Verified by the user on device |
| NF-5 | The framework's files are never changed. Writes go only to `/mnt/us/launcher/`, `/mnt/us/documents/Launcher.sh` and `/var/tmp/kl/`. The only root-filesystem file is the optional boot job (Phase 3). | Install script audit |
| NF-6 | The launcher binary and its assets do not depend on `/mnt/us` after start (they run from `/var/tmp/kl/`). | Spike S5 |
| NF-7 | Build offline: `cargo zigbuild --offline --target armv7-unknown-linux-musleabihf`, static binary ≤ 8 MB. | Build script |
| NF-8 | Each device build gets a new patch version (the user's rule). Tags `vX.Y.Z` are never moved. | Build script |
| NF-9 | Stop the framework only with `stop lab126_gui` (never SIGSTOP); never edit or remove framework jobs. | Code review |

## 1.6 Out of scope

- Amazon content: store, purchased books, Whispersync, Send-to-Kindle, Audible, Goodreads. Available
  only in Kindle mode.
- Adding a new Wi-Fi network (password entry) in the launcher: use Kindle mode once; saved networks then
  work from the launcher. May be added later.
- Book management; KOReader does it.
- Landscape; older Kindles; preventing the framework from starting at boot (M6c, later research only).

## 1.7 Constraints from the device (see the `kindle-device` and `kindle-eink` skills)

- `/var` is a 64 MB tmpfs; `/` has about 24 MB free and is read-only in normal use; `/mnt/us` is FAT.
- A program read from FAT and overwritten (or unmounted) while running can crash with SIGBUS.
- Touch `/dev/input/event8`, page buttons `/dev/input/event3`, accelerometer `/dev/input/event7`; find
  them by name, the numbers can change.
- Busybox shell (integer `sleep`, no `scp`).
- With the framework stopped, SSH runs only if something starts it (KOReader, the browser, or the
  launcher's setting). Keep a way to start SSH before boot autostart is enabled.
- The user watches the device; visual results count only after the user confirms them.
