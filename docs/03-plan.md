# 3. Plan

Status date: 2026-11-18. **Gate 1 passed on the Kindle with v0.1.11**: grid and paging, Primitive Browser and
KOReader open and return, Settings (frontlight, warmth, rotation, SSH), sleep and wake ×5, Kindle UI round trip
(about a minute, with an "Opening the Kindle UI" screen), supervisor kill returns the stock UI. Memory:
MemAvailable about 170–200 MB with the stock UI, about 310–340 MB with the launcher. Tests: kl-core 52, kl-render
8, kl-kindle 6 unit tests; supervisor stub checks 38; boot guard 15; library entry 4; KOReader wrapper 5; clippy
clean. Device findings and fixes are in device-notes.md.

| Milestone | State |
|---|---|
| Phase 1 steps 1–6 (launcher, supervisor, install, sleep, manual rotation) | Done on the Mac |
| Phase 1 step 7: Browser.sh patch (`PRIMITIVE_LAUNCHER=1`) and the orientation fallback | Done in `doannt-kindle-browser` (uncommitted, branch `feature/diagnostics-ssh`). Shell syntax checked; the Rust change is not compiled yet |
| Phase 2 status strip, quick settings, low battery | Done; Wi-Fi and battery icons ported from Primitive Browser (Mac-tested, not yet on the device) (footer warning with hysteresis; critical battery goes to the sleep screen with the warning) |
| Phase 2 autostart item in quick settings | Done (creates or removes `DISABLE`; clears the boot count when turned on) |
| Phase 2 auto-rotation | Debouncer done and tested (800 ms hold). **Not wired**: the accelerometer decoding waits for S4 on the Kindle |
| Phase 2 USB "Connect to computer" | **Not implemented.** Waits for S5. A wrong guess here could leave `/mnt/us` in use by the PC, so it is left out on purpose |
| Phase 2 network time (optional, S8) | **Not implemented.** The device clock is correct (S8); optional |
| Phase 3 boot job | Written: `deploy/primitive-launcher.conf`, `deploy/boot.sh`, `install-autostart.sh`, `remove-autostart.sh`; 15 guard checks. Not installed. The start event waits for S6 on the device |
| Phase 4 KUAL extensions | Done (Mac): an Extensions tile (shown when extensions are installed) opens a list with submenus, ordered by `priority` then name; the menu file comes from each extension's `config.xml` (else `menu.json`); items run `sh <extension>/<action> <params>`; `exitmenu: false` runs in the background. Not used: `status`, `checked`, `model`, `if` |
| FR-3 background mode | Done (Mac): `mode = "background"` in apps.toml or KUAL `exitmenu: false` runs the program while the launcher stays in front; its last output line is shown as a notice |
| FR-10 thresholds, FR-14 app list | Done (Mac): low at 10 %, critical at 5 % (with hysteresis); the discovered apps are logged at start |
| Phase 4 edit mode: hide | Done. Long-press an app to hide it (written to `apps.toml`); Settings > Hidden apps > Show all. The Kindle UI and Settings tiles cannot be hidden. The launcher rewrites `apps.toml` for this and drops comments |
| Phase 4 edit mode: move and choose icon | Done (Mac): long-press opens an app menu (Move earlier, Move later, Change icon from `/mnt/us/launcher/icons/*.png`, Hide), saved to `apps.toml` |
| Phase 4 screensaver choices | Done, default off. Pictures: PNG files in `/mnt/us/launcher/screensavers`, one per sleep, when the launcher's own sleep screen is on (Mac-tested, not yet on the device). Settings > Sleep screen: "The launcher's own" unloads the stock screensaver (`lipc-set-prop com.lab126.blanket unload screensaver`, as KOReader does). Switching back takes effect after Kindle mode or a reboot. **Device-unverified** |
| Phase 4 Wi-Fi network picker | **Not implemented.** Needs S3 (scan and profile properties) and a keyboard, which the launcher does not have |
| Phase 4 M6c (framework never starts at boot) | Research only, after Phase 3 is stable for weeks. Nothing to build |

Spikes: S1, S2 (button sleep and wake work with the `frameworkStarted` holder), S6, S8 answered; S7 read from
the scripts, not run; S3 partly seen (Wi-Fi states, `cmConnected`); S4, S5, S9 open.

Each phase ends with a gate. A gate passes only when the user confirms the visible behaviour on the
Kindle. Device builds bump the patch version every time.

## 3.1 Feasibility summary

| Part | Feasibility | Why |
|---|---|---|
| Stopping and restarting the framework | **High** | Measured on this device in the browser's stop-framework mode (~9 s to restart); KOReader's documented mode. |
| Launcher UI, apps, e-ink | **High** | Same device layer, toolchain and crates as Primitive Browser. |
| Sleep/wake without the framework | **High-medium** | KOReader does it daily in framework-stop mode; our exact `powerd` behaviour must be checked (S2). |
| Wi-Fi with saved networks | **Medium-high** | KOReader handles Wi-Fi on Kindle without the framework; property names to confirm (S3). |
| Auto-rotation | **Medium** | The accelerometer codes alternate when tilted (device notes). Debouncing should work; a manual toggle is the fallback. |
| USB drive mode on demand | **Medium** | `volumd` pause is proven by koreader.sh; entering drive mode without the framework screen is unverified (S5). |
| Boot into the launcher | **High** with care | The framework boots normally first; we only stop it, so every failure falls back to stock. |
| Memory gain for the browser | **Likely** | Size unknown until S1 measures it. |

## 3.2 Phase 0: device spikes (about one session with the device)

Read-only or reversible checks. Each result goes into `docs/device-notes.md` with what was seen and
where. Most need only SSH; the user watches the screen.

| Spike | Question | Method | Decides |
|---|---|---|---|
| **S1** | What does `stop lab126_gui` stop and leave? How much memory is freed? Is the screen and input free afterwards? | `initctl list`, `ps -o pid,rss,args`, `/proc/meminfo` before and after the stop and after the start; time both; check that no process holds `event8`/`event3` (`ls -l /proc/*/fd`). | §2.2 table; AC-12 baseline |
| **S2** | With the framework stopped: do power button, sleep cover and idle timeout still produce `powerd` events, suspend and wake? Does anything need `touchScreenSaverTimeout`? Do frontlight/battery properties still work? | `lipc-wait-event -mt com.lab126.powerd '*'` to a file on `/mnt/us`; power button ×2, cover close/open, wait for idle sleep; `lipc-get-prop` the properties. Compare with KOReader's Kindle code. | FR-5 design |
| **S3** | Wi-Fi without the framework: on/off, connect to saved network, state and events. | `lipc-probe -v com.lab126.wifid` and `com.lab126.cmd`; toggle; `lipc-wait-event` on `wifid`; `ifconfig wlan0`; read KOReader's Kindle network code for property names. | Wi-Fi API |
| **S4** | Accelerometer: codes on `/dev/input/event7` for upright / upside down / flat / tilted; a debounce rule. | Small reader logging events with timestamps while the user turns and tilts the Kindle. | FR-8 rule, or manual only |
| **S5** | USB: with the framework stopped and `volumd` running, what happens on plug-in (drive mode, `/mnt/us` unmounted)? With `volumd` paused, does charging still work? How to enter and leave drive mode on demand? | `lipc-wait-event` on `com.lab126.volumd`, `/proc/mounts`, a test loop running from `/var/tmp`. | FR-9 design, exit code 11 |
| **S6** | Upstart: the framework job's events, `/mnt/us` mount timing, `mntroot`, Upstart version (override support for M6c later). | Read `/etc/upstart/*.conf`; `initctl version` (no changes). | Boot job wording |
| **S7** | KOReader under a stopped framework: which flags leave the framework stopped on exit? Does it handle sleep and rotation itself? | Stop the framework; run `koreader.sh` with `--asap`, with no flags, and read its exit path; watch `initctl list` after exit. | KOReader command in `apps.toml` |
| **S8** | Clock: does time survive reboot without the framework? Is a network time tool present (`ntpd`, `ntpdate`, `rdate`)? | `date`, `hwclock`, `which`. | FR-11 |
| **S9** | Kindle mode round trip: launcher → `start lab126_gui` → library → `Launcher.sh` → framework stopped again. Any leftovers (pillow state, stuck services)? | Shell version of the supervisor on the device. | AC-3 |

**Gate 0:** S1–S9 answered and recorded, and the architecture updated where a result changed it.
**Stop condition:** if S2 shows the device cannot sleep and wake reliably without the framework, stop
and revisit the overlay design (00-research.md §0.0, first draft).

## 3.3 Phase 1: launcher with the framework stopped (MVP) (v0.1.x)

1. Workspace: `Cargo.toml`, crates from §2.3, `scripts/build-device.sh` (zigbuild, offline, patch bump,
   install, undo the bump if the install fails), as in the browser.
2. Device layer (modules of `kl-kindle`): copy `eink.rs`, `device.rs`, input readers and the crash log from the browser with
   provenance; keep the struct-size tests (88 / 160 bytes); add the `powerd` event listener.
3. `kl-core`: scriptlet parser, `apps.toml` merge, grid/paging, state machine; unit tests for every
   parser (data-URI icons, missing headers, CRLF files).
4. `kl-render` + `kl-desktop`: tiles, monograms, PNG icons (quantise + dither), page dots, sleep screen,
   tap inversion; PNG screenshots for review.
5. `deploy/supervisor.sh`: supervisor with the exit-code protocol, framework stop/start, `volumd` pause,
   watchdog, heartbeat, crash limit, bounded log. `deploy/library-entry.sh` becomes
   `documents/Launcher.sh` (600 × 920 icon, `assets/launcher-icon.png`, drawn by `kl-desktop`). The two
   files have different names because macOS and the Kindle's FAT storage ignore letter case.
   `deploy/install-ssh.sh` (stop → checksum copy → verify).
6. Sleep screen and wake (FR-5). Rotation: manual setting only in this phase.
7. Cross-repo: Primitive Browser patch release. `Browser.sh` skips its UI handling under
   `PRIMITIVE_LAUNCHER=1`; the browser reads `/var/tmp/kl/orientation` as a fallback.

**Gate 1** (user on device): `Launcher` book → framework stops → grid; one flash only; page through;
Browser opens and returns; KOReader opens and returns (framework still stopped); sleep/wake ×5 from
the launcher; Kindle mode → stock library → `Launcher` book → launcher; kill the supervisor → stock UI
returns by itself; memory figures recorded.

## 3.4 Phase 2: system duties (v0.2.x)

1. Status strip: clock, battery, charging, Wi-Fi.
2. Quick settings: frontlight, warmth, Wi-Fi, rotation, SSH, Kindle mode, autostart off.
3. Auto-rotation from the accelerometer (S4 rule), published to `/var/tmp/kl/orientation`.
4. USB "Connect to computer" (S5).
5. Low-battery notices and critical-battery sleep screen; optional network time (S8).

**Gate 2:** a full day of normal use without Kindle mode: reading in KOReader, browsing, Wi-Fi on/off,
10 sleep/wake cycles in the launcher and in each app, USB copy of a file, rotation both ways.

## 3.5 Phase 3: boot into the launcher (opt-in) (v0.3.x)

1. Before enabling autostart, make sure SSH can be started without the framework (launcher setting), and
   that the user knows the USB escape files.
2. `boot.sh`, the job file per §2.10 (wording from S6), install and remove scripts, the quick-settings
   item.
3. Dry runs before the first real reboot: run the job script by hand with each marker present and absent;
   `sh -n` on every script; test the boot counter.
4. Reboot tests with the user present: normal boot ×5; `DISABLE` file; `BOOT_KINDLE` file; a launcher
   binary that exits 1 at once (autostart must disable itself on the third boot, stock UI each time);
   USB cable in at boot; removal script.

**Gate 3:** every reboot test ends on the expected screen, and none needs SSH to recover.

## 3.6 Phase 4: ecosystem and polish (v0.4.x)

- KUAL extensions as an "Extensions" tile with a KUAL-style list (`items`, nesting, `params`,
  `priority`, `exitmenu`, `status`, model `if` checks).
- Edit mode: long press → move, hide, choose icon.
- Screensaver choices; Wi-Fi network picker with the browser's keyboard (if S3 shows it is possible).
- Research only: M6c (never start the framework at boot) after Phase 3 has been stable for weeks.

## 3.7 Test strategy

| Level | What | Where |
|---|---|---|
| Unit | Parsers, merge order, layout, state machine, rotation debouncer, dither/quantise, ioctl struct sizes | Mac, `cargo test` |
| Snapshot | Desktop driver renders each screen to PNG; compared with reviewed images | Mac |
| Shell | Supervisor and boot scripts against stubs for `start`, `stop`, `initctl`, `lipc-*`, `killall` (like the browser's `launcher-stub-test.sh`): exit-code protocol, crash limit, watchdog, markers | Mac |
| Device | Gate checklists; log lines `[launcher …]`, `[kl …]`; memory and timing from the log | Kindle, with the user watching |

## 3.8 Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Sleep or wake misbehaves without the framework | Low-medium | Battery drain or a stuck screen | S2 first; KOReader's implementation as reference; stop condition at Gate 0 |
| An app restarts the framework on exit (KOReader flags, old scripts) | Medium | Stock UI appears behind or over the launcher | Supervisor checks and stops it again after each app; S7 picks KOReader's flags |
| Accelerometer too noisy | Medium | Rotation flips | Debounce; manual setting |
| USB inhibit blocks charging or drive mode | Low | No computer access from the launcher | S5; Kindle mode always gives stock USB |
| No SSH after boot autostart | Medium | Harder debugging | SSH setting before Phase 3; USB escape files |
| Boot job mistake | Low (with Phase 3 tests) | Stock UI; worst case SSH to remove one file | Framework always boots first; no respawn; USB-editable markers; boot counter |
| SIGBUS from files changing under a running program | Known | Crash | Run from `/var/tmp`; `volumd` paused; stop before install |
| Firmware OTA | Low (5.18.2 is reported as the last Oasis 3 firmware) | Jailbreak or job removed | Keep OTA blocked |

## 3.9 Immediate next steps

1. Phase 0 with the Kindle on Wi-Fi and SSH started. S1, S2, S3, S6, S7, S8 and S9 are shell checks.
   S4 and S5 need a small reader binary or the user's hands.
2. Record the results in `docs/device-notes.md`; adjust §2.2 and §2.5–§2.7.
3. Start Phase 1 steps 1–4 on the Mac in parallel; they do not depend on the spikes.
