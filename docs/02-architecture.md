# 2. Architecture

Status date: 2026-11-17. Phase 1 code exists (unverified on the device); see 03-plan.md §3.3. Items that depend on unverified device
behaviour name the spike (S1–S9) in [03-plan.md](03-plan.md) that settles them.

## 2.1 Approach in one paragraph

The stock framework (`lab126_gui`) is stopped while the launcher is in use, so nothing else draws to
the screen or reads input. A shell **supervisor** stops the framework and runs the **launcher** (a small
native Rust program). When the user picks an app, the launcher exits and tells the supervisor which app
to run; the supervisor runs it, waits for it, and starts the launcher again. The supervisor restarts the
framework when the user picks Kindle mode, when the launcher keeps crashing, or (through a watchdog) when
the supervisor itself is killed. The launcher takes over the framework's few remaining duties: sleep
screen, Wi-Fi switch, rotation, USB mode, status. An optional boot job starts the supervisor after a
normal boot. Why this design: [00-research.md](00-research.md) §0.0 and §0.5.

## 2.2 What the framework did, and who does it now

| Duty | Stock Kindle | With the launcher | Check |
|---|---|---|---|
| Screen and UI | `cvm`, X, `awesome`, pillow | Launcher in front; the running app otherwise | S1 |
| Touch and buttons | X | Launcher reads evdev; apps read their own (no grab needed) | S1 |
| Sleep, wake, screensaver | `powerd` + the framework draws the screensaver | `powerd` runs; the launcher holds `com.lab126.kaf frameworkStarted = 1` (`deploy/holder.lua`) so the power button sleeps and wakes it (S2b); blanket still draws the stock sleep screen; apps draw their own (KOReader does) | S2 |
| Idle sleep timer | Framework reports activity | `powerd` alone, or the launcher resets it on input (`touchScreenSaverTimeout`) | S2 |
| Frontlight, warmth, battery | `powerd` | Same `lipc` properties (the browser uses them today) | S2 |
| Wi-Fi | `wifid` + framework UI | `wifid` through `lipc`: on/off, state, saved networks. New networks: Kindle mode. | S3 |
| Rotation | `com.lab126.winmgr accelerometer` (needs `awesome`) | Launcher reads `/dev/input/event7` with debouncing, or a manual setting; publishes `/var/tmp/kl/orientation` | S4 |
| USB drive mode | `volumd` + framework screen | Launcher: "Connect to computer" action and screen; `volumd` paused otherwise (as koreader.sh does) | S5 |
| Clock | Network time through the framework | Hardware clock; optional network time after Wi-Fi connects | S8 |
| Keyboard | Stock keyboard | Each app's own (the browser has one) | — |
| Books | Library + reader | KOReader | S7 |
| SSH | Started by apps | Also a launcher setting (development) | — |

## 2.3 Components

```
 boot (Phase 3) ─► /etc/upstart/primitive-launcher.conf ─► /mnt/us/launcher/boot.sh (guards) ─┐
 Kindle mode    ─► /mnt/us/documents/Launcher.sh (library book) ────────────────────────────────┤
                                                                                              ▼
                         /var/tmp/kl/supervisor.sh (copied from /mnt/us/launcher)  — SUPERVISOR (shell)
                                 • copy kl-launcher + assets to /var/tmp/kl/
                                 • stop lab126_gui, write marker "framework-stopped"
                                 • pause volumd (USB inhibit)
                                 • loop: run launcher → act on its exit code
                                 • watchdog process outlives it; restarts the framework if it dies
                                                │                       ▲
                                    runs        ▼                       │ exit code + /var/tmp/kl/next
                               /var/tmp/kl/kl-launcher  — LAUNCHER (Rust, static armv7 musl)
                                 grid UI · discovery · sleep screen · status · quick settings
                                 inputs: touch (event8), buttons (event3), accelerometer (event7),
                                         powerd/wifid events (lipc-wait-event), signals, clock
```

### Supervisor exit-code protocol

| Launcher exit | Meaning | Supervisor action |
|---|---|---|
| 10 | Launch the app described in `/var/tmp/kl/next` (command, working directory, mode) | Run it with `PRIMITIVE_LAUNCHER=1`, `PRIMITIVE_FRAMEWORK=stopped`; wait; start the launcher again. An app crash does not count as a launcher crash. |
| 11 | USB drive mode | Resume `volumd`, wait until the drive is released and `/mnt/us` is back, pause `volumd`, rescan, start the launcher again. Spike S5 may move this into the launcher. |
| 0 | Kindle mode | Resume `volumd`, `start lab126_gui`, remove the marker, exit. |
| other | Crash | Count it. Fewer than 3 in 60 s: restart the launcher. Otherwise: Kindle mode, logging "too many crashes". |

Why the launcher exits while an app runs instead of staying resident:
- **Memory:** the app gets all of it.
- **Simpler launcher:** no child process management.
- **Same path every time:** every app runs from the same shell path as from the library.

The cost is a cold start on return, targeted ≤ 1 s, helped by running from tmpfs and a pre-dithered
icon cache. If that target is missed, a resident mode can be added later.

### Rust workspace (planned)

| Crate | Role | Source |
|---|---|---|
| `kl-core` | Platform-free: app model, discovery (scriptlet header parser, KUAL `config.xml`/`menu.json`, `apps.toml`), grid and paging, the UI state machine, rotation debouncer, settings. Unit-tested on the Mac. | New |
| `kl-render` | Screen → 8-bit gray buffer: `vello_cpu` shapes/images, `parley` text, PNG decode, icon scaling, 16-level quantise, Bayer dither, solid text. Same pinned versions as the browser (in the offline cache). | New; dither/quantise/solidify copied from the browser |
| `kl-kindle` | The device binary `kl-launcher`, with the device layer as modules: `eink.rs` (framebuffer + Zelda update ioctl), `input.rs` (touch and page buttons), `device.rs` (`lipc` properties, power events, SSH helper), `profile.rs`. | `eink.rs` and `profile.rs` copied from `doannt-kindle-browser/crates/kb-kindle` 0.2.12 with a provenance header; input and device code adapted from it. A shared crate only if both copies keep changing. |
| `kl-desktop` | Mac driver: renders screens to PNG and replays scripted input for tests and screenshots. | New |

## 2.4 Launcher states

```
   start ──► HOME ◄──────────────── wake ────────────── SLEEP
              │ ▲                                         ▲
   tap tile   │ │ back                goingToScreenSaver  │ (from any launcher state)
              ▼ │
           EXTENSIONS / SETTINGS / ABOUT  (sub-screens)
              │
   tap app ───┴─► write /var/tmp/kl/next, exit 10   (supervisor runs the app)
   USB        ───► exit 11
   Kindle mode ──► exit 0
```

- On start: read orientation, discover apps (from the cache, then a background rescan), draw with one
  full GC16 update, then partial no-flash updates.
- `SLEEP`: on `goingToScreenSaver`, draw the sleep screen (full update), stop reading touch. On
  `outOfScreenSaver`, redraw `HOME`. Sleep while an app runs is the app's job. Apps that do nothing
  leave their own screen, as now.
- Rotation changes redraw the whole screen and swap the page buttons, as in the browser.

## 2.5 Apps and the framework being stopped

Apps were written to start from the library with the framework running. Under the launcher they see
`PRIMITIVE_FRAMEWORK=stopped`:

- **Primitive Browser (`Browser.sh`).** In keep mode it disables pillow and, on exit, retries opening
  the library 40 times (10 s) when that fails. Change: with `PRIMITIVE_LAUNCHER=1`, skip
  `stop_ui`, `restore_ui` and `land_on_home`. Rotation: read `/var/tmp/kl/orientation` when the framework
  property gives nothing. This is a browser patch release.
- **KOReader (`koreader.sh`).** Without flags, it uses its keep-mode path: pillow calls fail harmlessly,
  but on exit it *starts* framework jobs it stopped and the home booklet. With `--framework_stop`, it
  starts `lab126_gui` on exit, which we must avoid. Spike S7 picks the invocation. Likely `--asap` plus a
  check that its exit leaves the framework stopped. If no flag combination works, a small wrapper
  restores the stopped state after KOReader exits.
- **KUAL actions and scriptlets.** Run with `/bin/sh` in their own folder. Scripts that call framework
  `lipc` services will fail on those calls; their output is shown so the user sees why.

## 2.6 Sleep and power

- One `lipc-wait-event -m com.lab126.powerd …` child process, restarted with backoff if it dies. It
  listens for `goingToScreenSaver`, `outOfScreenSaver`, `exitingScreenSaver`, `readyToSuspend`,
  `wakeupFromSuspend`, `battLevelChanged`, `charging`, `notCharging` and `battLevelCrit`.
- The Wallpaper page selects one sleep PNG from `/mnt/us/launcher/wallpapers/`, rotates PNGs in
  `/mnt/us/launcher/screensavers/`, or leaves the screen to the Kindle screensaver. Launcher prepares
  the selected image while awake and gives the low-battery warning priority over the picture.
- If S2 shows `powerd` will not sleep on its own without the framework, the launcher reports activity
  on input (rate-limited `touchScreenSaverTimeout`) and the sleep cover/power button handling follows
  KOReader's Kindle implementation.

## 2.7 Wi-Fi, USB, clock

- **Wi-Fi:** `lipc` on/off and state; `wifid` connects to saved profiles. The status strip shows
  off / connecting / connected. New networks are added once in Kindle mode. Exact property names come from
  S3 and KOReader's Kindle network code.
- **USB:** the supervisor pauses `volumd` (SIGSTOP) while the launcher or an app runs, so plugging in a
  cable cannot unmount `/mnt/us` under a running program. "Connect to computer" resumes it and shows a
  USB screen until the drive is released. Charging is unaffected. S5 confirms both halves.
- **Clock:** the hardware clock persists across sleep and reboot. After Wi-Fi connects, an optional
  network time sync runs if a tool is available on the device (S8).

## 2.8 App discovery and configuration

Order: built-ins → scriptlets → KUAL extensions → `apps.toml` overrides. Stable ids: the file name for
scriptlets, `kual:<extension id>` for extensions. Icons are cached as pre-dithered tiles in
`/mnt/us/launcher/cache/` (keyed by source checksum) and copied to `/var/tmp/kl/` at start.

| Source | Read | Launch |
|---|---|---|
| Built-in `kindle` | — | Exit 0 (Kindle mode) after a confirm sheet ("Starting the Kindle UI takes about 10 s") |
| Built-in `settings` | — | Quick settings screen |
| Scriptlet `/mnt/us/documents/*.sh` | `# Name:`, `# Author:`, `# Icon:` (absolute path or `data:image/png;base64,`), `# UseHooks` | `/bin/sh <file>`; with `# UseHooks`, source it and call `on_run`. Mode `screen`. |
| KUAL extension | `config.xml` → JSON menu: `items` (nested), `name`, `action`, `params`, `priority`, `exitmenu`, `if` (model checks) | `action params` in the extension's folder; `exitmenu: false` → mode `background` |
| `apps.toml` | Overrides and extra entries | As declared |

`/mnt/us/launcher/apps.toml` (example):

```toml
columns = 3
rows = 3
rotation = "auto"            # "auto" | "upright" | "upside-down"
ssh = false
screensaver = "rotate"       # "rotate" | "fixed:<file>" | "default"

[[app]]
id = "koreader"
name = "KOReader"
command = "/mnt/us/koreader/koreader.sh --asap"    # final flags from spike S7
icon = "/mnt/us/launcher/icons/koreader.png"
order = 1

[[app]]
id = "Browser.sh"            # discovered scriptlet; override only what differs
order = 2

[[app]]
id = "kindle"                # built-in Kindle mode
order = 99

[[app]]
id = "kual:kterm"
hidden = true
```

## 2.9 Drawing (from the `kindle-eink` skill)

- Screen 1264 × 1680, Y8: draw into a gray buffer, copy changed rows, send the Zelda update ioctl.
- Chrome uses only pure black and white. Text is anti-aliased, then solidified at the extremes. Icons
  are quantised to the 16 panel steps and Bayer-dithered.
- Tap feedback: invert the tile (DU partial), then exit to launch.
- Page changes: partial, no flash. Full GC16 only on start, wake and rotation.

Layout sketch (upright):

```
┌────────────────────────────────────────┐
│ 10:42                   Wi-Fi  ▮▮▮▯ 78% │  status strip (tap: quick settings)
├────────────────────────────────────────┤
│   ┌────────┐   ┌────────┐   ┌────────┐  │
│   │  icon  │   │  icon  │   │  icon  │  │  tiles 2:3, 3 × 3 per page
│   └────────┘   └────────┘   └────────┘  │
│    KOReader    Primitive    Extensions  │
│                Browser                  │
│                    ...                  │
│                            Kindle mode  │
├────────────────────────────────────────┤
│                  ● ○                    │  page dots; swipe or page buttons
└────────────────────────────────────────┘
```

## 2.10 Boot start (Phase 3)

Stop-after-boot: the Kindle boots exactly as stock, then the framework is stopped.

- **Job file** `/etc/upstart/primitive-launcher.conf` (the only root-filesystem file; written with
  `mntroot rw` / `mntroot ro`). Start condition: after the framework job has started (exact wording from
  S6). Script:
  1. wait up to 60 s for `/mnt/us/launcher/boot.sh` (storage mounted, not in USB mode); otherwise exit 0;
  2. exit 0 if `/mnt/us/launcher/DISABLE` or `/mnt/us/BOOT_KINDLE` exists (the USB escape hatch);
  3. start `boot.sh` detached and exit 0 at once. No `respawn`.
- **`boot.sh`** (on `/mnt/us`, editable over USB): increment `boot-attempts`. At 3 or more, create
  `DISABLE` and exit. Otherwise wait until the framework is idle (home booklet up, polled up to 60 s),
  then start the supervisor. The launcher resets `boot-attempts` after 60 s of healthy running.
- **Failure cases:** every failure leaves or returns the stock UI. A broken `boot.sh` can be deleted
  over USB. A broken job file makes upstart skip it; it cannot stop `lab126_gui` by itself.
- **Later (M6c):** an Upstart override that keeps the framework from starting at all would save the
  framework's boot time. It is deferred because a double failure would leave no UI and no SSH.
- **Install / remove:** `deploy/install-autostart.sh`, `deploy/remove-autostart.sh` (SSH), and an
  "Autostart off" item in quick settings.

## 2.11 Files on the device

```
/mnt/us/documents/Launcher.sh          entry from Kindle mode (repo: deploy/library-entry.sh)
/mnt/us/launcher/
    kl-launcher                        binary (copied to /var/tmp/kl/ at start)
    supervisor.sh                      supervisor + watchdog (copied to /var/tmp/kl/ and run from there)
    ssh.sh                             SSH on/off for Settings (uses Primitive Browser's dropbear)
    settings.txt                       written by the launcher (rotation)
    boot.sh                            Phase 3 boot guard
    apps.toml                          user configuration
    fonts/ icons/ screensavers/ cache/ assets
    log-launcher.txt                   bounded log (about 200 KB)
    DISABLE, boot-attempts             Phase 3 markers
/var/tmp/kl/                           runtime: binary copy, assets, next, orientation, pids, markers
/etc/upstart/primitive-launcher.conf   Phase 3 only, optional
```

## 2.12 Failure modes

| Failure | Result | Recovery |
|---|---|---|
| Launcher crashes | Supervisor restarts it; 3 crashes in 60 s → Kindle mode | Automatic |
| Launcher hangs | Watchdog sees no heartbeat (`/var/tmp/kl/heartbeat`, 20 s), kills it; counts as a crash | Automatic |
| Supervisor killed | Watchdog (outlives it, as in `Browser.sh`) sees the marker, resumes `volumd`, starts `lab126_gui` | Automatic, ~9 s |
| App hangs | As today; the power button still sleeps the device (`powerd`) | App's own recovery, or SSH, or a long power-button reboot |
| App leaves the framework started (e.g. a KOReader flag) | Supervisor checks after each app; if the framework is running, it stops it again and logs it | Automatic |
| USB cable plugged while running | `volumd` paused: charging only, `/mnt/us` stays mounted | "Connect to computer" |
| Bad `apps.toml` | Ignored with a notice; discovered apps still shown | Edit over USB |
| Boot problem | Stock UI after boot | `DISABLE` / `BOOT_KINDLE` over USB, or the boot counter |
| Firmware update | Job removed; files on `/mnt/us` stay | Re-run the autostart install (OTA stays blocked) |
