# 0. Research: prior art and how to replace the stock home

Status date: 2026-11-17. Target: the user's jailbroken Kindle Oasis 3 (firmware 5.18.2, KOReader,
KPM, SSH, Primitive Browser installed). Facts marked **(device)** were measured on that Kindle in the
Primitive Browser project. Facts marked **(web)** come from the sources at the end. Anything marked
**unverified** must be checked on the device before we rely on it (see the spikes in
[03-plan.md](03-plan.md)).

## 0.0 Decision log

| Date | Decision | Reason |
|---|---|---|
| 2026-11-17 | **Retire the stock Kindle UI from daily use.** The launcher runs with the framework stopped (M6 below). The stock UI stays on the device, unmodified, as a slow "Kindle mode" escape hatch. | The user does not read Amazon-purchased books and does not use Send-to-Kindle or Whispersync. Without those, the framework's only value is as a fallback. Stopping it frees memory for the browser and removes every fight over the screen and input. |
| 2026-11-17 | First draft (superseded): an overlay over a running framework, with the library as one app (M2–M5). | Kept below for reference; it remains the design if Amazon content is ever needed again. |

## 0.1 The question

The user wants a home screen that is *the* interface of the Kindle: one place for every application
(Primitive Browser, KOReader, KUAL extensions, ...). This document answers:

1. Is there an existing project that already does this, or one we can build on?
2. Can the stock home be replaced *without* writing a new launcher?
3. What are the technical ways to take the "home" role from the stock UI, and which is safe?
4. What should we reuse?

## 0.2 How the stock UI is built (what we are retiring)

- The stock UI is the **framework**: upstart job `lab126_gui`, the Java `cvm` process, the X window
  manager `awesome`, the status-bar layer **pillow**, and services on the **lipc** bus (`com.lab126.*`).
  **(device, web)**
- The home/library is a *booklet* inside the framework, started with
  `lipc-set-prop com.lab126.appmgrd start app://com.lab126.booklet.home`. **(device)**
- `stop lab126_gui` / `start lab126_gui` stop and restart the whole stock UI. A restart takes about 9 s
  **(device**, the browser's stop-framework mode). KOReader's `--framework_stop` mode does the same, and
  it is KOReader's documented way to run. **(web)**
- Power, sleep and the screensaver are driven by `powerd` (`com.lab126.powerd` events
  `goingToScreenSaver`, `outOfScreenSaver`, `exitingScreenSaver`, `readyToSuspend`,
  `wakeupFromSuspend`; properties include `touchScreenSaverTimeout`, `preventScreenSaver`). **(web)**
  KOReader's framework-stop mode relies on these events, which suggests `powerd` keeps running when
  `lab126_gui` stops. **Unverified on this device.**
- The orientation property `com.lab126.winmgr accelerometer` answers only while `awesome` runs
  **(device)**, so with the framework stopped there is no rotation property. The accelerometer's own
  input device (`/dev/input/event7`, `bma_interrupt`) gives codes that alternate when the Kindle is tilted.
  **(device)**
- Frontlight, warmth and battery are `powerd` properties (`flIntensity`, `currentAmberLevel`,
  `battLevel`, `isCharging`). The browser already uses them. **(device)**

## 0.3 Prior art

| Project | What it is | Replaces home? | Tech | Fit for us |
|---|---|---|---|---|
| **KUAL** (classic, coplate booklet) | The standard menu of extensions (`/mnt/us/extensions/*/config.xml` + `menu.json`). | No. A book in the library that opens a menu. | Java booklet API (needs the framework) | Its **extension format** is the ecosystem standard. It cannot run with the framework stopped. |
| **KUAL Next** (thiagokokada) | Native reimplementation of the KUAL menu for kindlehf firmware 5.16.3+. Parses `config.xml`/`menu.json` incl. nested items, `params`, priorities, RPN `if`, `exitmenu`, `refresh`, `status`. | No. A scriptlet over the framework; restores it for lock/handoff/exit. | C then Zig, FBInk, evdev | Best existing **menu** and the reference for KUAL semantics. A text list, not an icon grid; built around a running framework; untested on Oasis 3; licence not checked. |
| **OpenReader** (terpinedream) | Touch launcher that replaces the stock UI. Optional "Amazon-Free Boot" upstart job (`openreader-boot.conf`); `boot-replacement.sh` SIGSTOPs framework processes. Boot-loop guard (more than 3 boots in 60 s falls back to stock). Escape hatch: an empty `BOOT_KINDLEOS` file on USB storage. | **Yes** | Shell + small C touch reader + FBInk | Same goal as ours, which confirms the idea. Not a base: 4 commits, fixed button coordinates with manual calibration, hard-coded `/dev/input/event1` (Oasis touch is `event8`). It freezes the framework with SIGSTOP instead of stopping it. Good **safety ideas** to copy. |
| **lachlanchen/Kindle** | Guarded KOReader autostart for PW5 on 5.15.1. Native / framework-up / framework-stop modes, 30 s recovery window, single launch with no respawn, regular-file markers; warns that USB drive mode makes `/mnt/us` disappear during boot. | Yes (KOReader) | Upstart + shell | Good **boot-guard design** to copy. |
| **KOReader autostart** (MobileRead, "KOReader Autolaunch" KUAL extension) | Upstart job that execs `koreader.sh --kual [--framework_stop]`, guarded by `test -x`. | Yes (KOReader as home) | Upstart | Maintainers warn a crash at boot can loop. The risk is in the job, not the app. |
| **koreader.sh** (KOReader's launcher) | `--framework_stop`: `stop lab126_gui` (with `trap "" TERM` and a 1.25 s pause) and `start lab126_gui` on exit. Keep mode: framebuffer dump and restore, pillow disable/enable, home booklet start. Pauses `volumd` (SIGSTOP) to stop USB mass storage taking `/mnt/us` while it runs. | n/a | Shell | Reference for **stopping and restarting the framework** and for **USB inhibition**. |
| **KOReader** itself | A complete framework-free Kindle app: its own sleep screen, Wi-Fi handling, frontlight, power events. | n/a | Lua + C | Proof that a Kindle works day to day with the framework stopped; reference for which services survive. |
| **Primitive Browser** (ours): `deploy/Browser.sh`, `crates/kb-kindle` | Supervisor with watchdog, crash-restart limit and restore marker; a stop-framework mode; Rust e-ink driver (Zelda ioctl, no-flash rules), touch and buttons, `lipc` helpers. | n/a | Shell + Rust | The **device layer and supervisor** we reuse directly. |
| **Mesquito / Illusion** (WinterBreak) | HTML/JS apps in the framework's web-app runner. | No | Mesquite WAF | Needs the framework; no longer relevant. |

**Conclusion.** No existing project gives an icon grid, Oasis 3 support, no-flash e-ink drawing, KUAL and
scriptlet apps, and safe framework-free operation. OpenReader has the same goal but is not a usable
base. We build a small native launcher on our own Rust device layer. It reads the KUAL and scriptlet
formats, takes over the framework's few remaining duties (§2.2 of the architecture), and copies the
boot guards from OpenReader and lachlanchen/Kindle and the framework stop/start from koreader.sh.

## 0.4 Replacing the stock home *without* writing a launcher

| Setup | What the user gets | Cost / risk | Verdict |
|---|---|---|---|
| **KOReader as home**: guarded autostart with `--framework_stop`. | Fast start into KOReader's file browser; framework-free. | KOReader is a reader, not an app launcher: Primitive Browser and others need KOReader plugins or a terminal to start. | Viable stop-gap. Does not meet "one place for all applications". |
| **KUAL Next (or KUAL) as the app list** | One text list with every app, no code. | Needs the framework running; not home; untested on Oasis 3. | Only useful while the framework is still the home. |
| **OpenReader** as-is | Real replacement at boot. | Wrong touch device and button coordinates for Oasis 3; frozen (not stopped) framework; early-stage. | No. |
| **Re-point the stock "home"** in the framework's app registry (`/var/local/appreg.db`; **unverified on 5.18.2**) | — | The registry starts booklets inside the framework, not native programs. It changes undocumented state, needs SSH to recover from a bad entry, and can be undone by firmware. | Rejected (M7). |

## 0.5 Ways to take the "home" role

| # | Mechanism | Decision |
|---|---|---|
| M1 | Launcher as a scriptlet in the library | **Kept** as the way back from Kindle mode (`Launcher.sh` stops the framework and starts the launcher). |
| M2 | Resident launcher drawing over a running framework | Superseded by M6 (the draft design). |
| M3 | Intercept the stock Home button through lipc events | Superseded: with the framework stopped there is no stock Home. |
| M4 | Page-button chord / library book to return | Superseded (the book is kept as M1). |
| M5 | Boot job that starts the launcher over a running framework | Superseded by M6's boot path. |
| **M6** | **Launcher with the framework stopped** (`stop lab126_gui`, as KOReader `--framework_stop`); the framework is restarted only for Kindle mode or as a fallback | **Chosen.** |
| M6b | Freeze the framework with SIGSTOP (OpenReader) | Rejected: frozen services hold memory and can misbehave when resumed; a clean stop is proven on this device. |
| M6c | Keep the framework from starting at boot (an Upstart `lab126_gui.override` with `manual`) | Later research only. If our boot job also failed, nothing would draw a UI and SSH would not be running. Consider only after the stop-after-boot path has been stable for weeks; check that this Upstart version supports override files. |
| M7 | Re-point the home booklet in the app registry | Rejected (§0.4). |
| M8 | Launcher as a Mesquite/Illusion web app | Rejected: needs the framework. |

**Safety property of M6.** The framework is never deleted or edited, only stopped. The supervisor (a
shell script, like `Browser.sh`) restarts it whenever the launcher gives up, is killed, or the user
chooses Kindle mode. Every failure ends at the stock UI after about 9 s, never at a dead screen.

## 0.6 What we reuse

| From | What |
|---|---|
| Primitive Browser (`crates/kb-kindle`) | Framebuffer + Zelda update ioctl, waveform and no-flash rules, touch/button readers, input discovery by name, `lipc` helpers (battery, frontlight, warmth), crash log handler, dither/quantise/solidify. |
| Primitive Browser (`deploy/Browser.sh`, `install-ssh.sh`) | Supervisor: framework stop/start, restore marker, watchdog that outlives the launcher, crash-restart limit, bounded log; safe install (stop, then copy by checksum). |
| Primitive Browser toolchain | `cargo zigbuild --offline` to `armv7-unknown-linux-musleabihf`; `vello_cpu`, `parley`, `png`, `toml` already in the offline crate cache. |
| koreader.sh / KOReader | Framework stop sequence; `volumd` pause for USB inhibition; which `powerd` events to handle and how a framework-free app sleeps. |
| KUAL / KUAL Next | `config.xml` + `menu.json` format and semantics. We read it ourselves; KUAL need not be installed. |
| KindleModding scriptlets | `documents/*.sh` with `# Name:`, `# Author:`, `# Icon:` (path or data URI), `# UseHooks` (`on_run`), `# DontUseFBInk`. The `documents/` folder stays our app registry even though the library is retired. |
| OpenReader, lachlanchen/Kindle, KOReader autostart | Boot guards: an escape-hatch file on USB storage, a boot counter that disables autostart, a single launch with no upstart respawn, `test -x` before exec, wait for `/mnt/us`. |

## 0.7 Sources

- KUAL Next: https://github.com/thiagokokada/kual-next/ and https://www.mobileread.com/forums/showthread.php?t=374955
- KUAL: https://www.mobileread.com/forums/showthread.php?t=203326, https://wiki.mobileread.com/wiki/KUAL_What's_New
- OpenReader: https://github.com/terpinedream/OpenReader
- Guarded KOReader autostart: https://github.com/lachlanchen/Kindle
- KOReader autostart discussion: https://www.mobileread.com/forums/showthread.php?t=365866,
  https://www.answeroverflow.com/m/1359655622877909063, https://kindlemodshelf.me/bootkoreader
- koreader.sh: https://github.com/koreader/koreader/blob/master/platform/kindle/koreader.sh
- Scriptlets: https://kindlemodding.org/kindle-dev/scriptlets.html
- lipc events and properties: https://wiki.mobileread.com/wiki/Lipc, https://www.mobileread.com/forums/showthread.php?t=367800,
  https://blog.davidv.dev/posts/integrating-a-kindle-into-house-automation/
- WinterBreak / Mesquito: https://hackerdude.tech/projects/2025-01-01-winterbreak/
- Jailbreak overview (firmware ranges, OTA warning): https://www.androidauthority.com/amazon-old-kindle-jailbreak-how-3669128/
