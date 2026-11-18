# 3. Plan

Status date: 2026-11-18. On the Kindle: **v0.2.5**. Gate 1 passed with v0.1.11.

How to read the checklists:

- `[x]` done and confirmed on the Kindle by the user.
- `[x] … (Mac)` done and tested on the Mac, not yet confirmed on the Kindle.
- `[ ]` not done.

A milestone is finished when every item is `[x]` without "(Mac)". A phase ends with a gate that the user
checks on the Kindle. Device builds bump the patch version; a new phase starts a new minor version
(`scripts/build-device.sh --minor`). Device findings are in [device-notes.md](device-notes.md).

## Overview

| Milestone | Phase / version | State |
|---|---|---|
| [M0 Device spikes](#m0-device-spikes) | 0 | 8 of 9 answered; S5 (USB) open |
| [M1 Launcher with the framework stopped](#m1-launcher-with-the-framework-stopped) | 1 / 0.1.x | Done, Gate 1 passed |
| [M2 Home screen, Apple style](#m2-home-screen-apple-style) | 2 / 0.2.x | **Done**, confirmed on the Kindle (v0.2.5) |
| [M3 Status bar](#m3-status-bar) | 2 / 0.2.x | Icons done (Mac) |
| [M4 Settings pages](#m4-settings-pages) | 2 / 0.2.x | Single page exists; pages not started |
| [M5 Wi-Fi networks](#m5-wi-fi-networks) | 2 / 0.2.x | API found (S3); not started |
| [M6 System duties](#m6-system-duties) | 2 / 0.2.x | Auto-rotation confirmed; battery warnings (Mac); USB waits for S5 |
| [M7 Boot into the launcher](#m7-boot-into-the-launcher) | 3 / 0.3.x | Written, not installed |
| [M8 Extensions and customising](#m8-extensions-and-customising) | 4 / 0.4.x | Mostly done (Mac) |

Order of work: M2 → M3 → M4 → M5, each shown in desktop screenshots first, then a device build for the
user to confirm. M6 items go in whenever the device is at hand. Gate 2 follows M6.

---

## M0 Device spikes

- [x] S1: what stopping the framework frees (about 140 MB) and that the screen and input are free.
- [x] S2: sleep and wake without the framework (needs the `frameworkStarted` holder).
- [x] S3: Wi-Fi API in `com.lab126.wifid`: `signalStrength` ("4/5"), `scan`, `scanList`, `createProfile`,
      `cmConnect`, `deleteProfile`, `cmState`. Found 2026-11-18; connecting is still untested.
- [x] S4: the accelerometer's sysfs `value` (x y z) works with the stock UI stopped; its events do not.
      Rule: y ≥ 0.6 g and 1.5 × |x| decides upright / upside down; 800 ms hold. Found 2026-11-18.
- [ ] S5: USB drive mode on demand with the framework stopped. First half done: with volumd paused the
      Kindle charges and `/mnt/us` stays mounted (no drive on the computer). Open: drive mode on demand.
- [x] S6: upstart jobs and events.
- [x] S7: KOReader flags (from the scripts; KOReader opens and returns on the device).
- [x] S8: the clock survives reboots; no network time tool needed.
- [x] S9: Kindle UI round trip (about a minute; no leftovers since v0.1.10).

## M1 Launcher with the framework stopped

Phase 1, v0.1.x. **Gate 1 passed with v0.1.11.**

- [x] Rust workspace, build script, SSH install with checksums.
- [x] Supervisor: stops and restarts the framework, runs apps, crash limit, watchdog, log.
- [x] `Launcher` book in the Kindle library opens the launcher.
- [x] App grid with paging; scriptlets from `documents/` and `apps.toml`.
- [x] Primitive Browser and KOReader open and return to the launcher.
- [x] Sleep and wake with the power button and the cover.
- [x] Kindle UI round trip in place, with a progress screen.
- [x] A killed supervisor brings the stock UI back by itself.
- [x] Manual rotation (upright / upside down), passed on to the apps.
- [x] Settings page: frontlight, warmth, rotation, SSH, Kindle UI.
- [x] Local time from the time zone file.

## M2 Home screen, Apple style

Phase 2. The app grid should feel like an iPhone home screen: simple, with everything lined up, and
feedback only where you touch. No dock: only the grid.

- [x] Tap feedback on the touched icon only: the icon darkens; nothing else changes (v0.2.1).
- [x] E-ink updates cover only the changed rectangle, not full-width rows (v0.2.1).
- [x] Rounded-square icons, all the same size, on an even 4 × 5 grid (v0.2.1).
- [x] Names centred under the icons, one line, cut with "…" (v0.2.1).
- [x] PNG icons fitted into the rounded square; apps without an icon get initials in the same shape (v0.2.1).
- [x] Page dots at the bottom; swipe and the page buttons change pages (v0.2.2).
- [x] App menu on a long press, in a box beside the icon (v0.2.1).
- [x] Sleep draws nothing: the Kindle's screensaver (or a sleep picture) shows straight away, with no
      "Sleeping" screen first. Only a critical battery shows text (v0.2.2).
- [x] Kindle UI icon: the Kindle app's reader under the arcs, from the user's `assets/kindle-icon.png`,
      cropped, made transparent outside the rounded square and mapped to the panel's grays (v0.2.3).
- [x] Primitive Browser icon: the browser's square logo without text (`icons/primitive-browser.png`, set in
      the Kindle's `apps.toml`).
- [x] Settings icon: the white gear on black stays (the user turned down three other designs).
- [x] Opening zoom: a white card grows from the tapped icon to the screen in 3 fast black-and-white steps
      (v0.2.4; the user saw it work), with an on/off switch in Settings (v0.2.5).
- [x] Many apps: pages of 20, swipe or page buttons, dots at the bottom (30 test apps, v0.2.5).
- [x] Dark mode: home, Settings, menus and dialogs white on black; picture icons keep their look (v0.2.5).
- [x] The Kindle UI confirmation is a pop-up over the grid; "Open" turns it into the "Opening the Kindle UI"
      dialog with the filling blocks, so the screen never goes blank (v0.2.5).
- [x] Pop-ups are drawn last, so picture icons no longer show through the app menu (v0.2.5).

## M3 Status bar

Phase 2. A bar like the stock Kindle's: several indicators, each with its own state, and a tap opens
quick settings.

- [x] Battery icon with level, number and charging bolt (from Primitive Browser) (Mac).
- [x] Wi-Fi icon with a slash when off, "Not connected" when on without a network (Mac).
- [ ] Wi-Fi signal strength: 0–5 bars from `signalStrength`.
- [ ] Indicator list: each item (Wi-Fi, airplane mode, SSH, background job, USB, battery warning) has its
      own state and is shown only when it matters.
- [ ] Layout: indicators on the left, clock in the centre, battery on the right.
- [ ] Tap the bar: quick settings drop down (airplane mode, Wi-Fi, rotation, frontlight and warmth
      sliders, "All settings").
- [ ] State follows events (Wi-Fi connects, charger in/out), not only the once-a-minute refresh.

## M4 Settings pages

Phase 2. Settings becomes a list of pages, like the stock Kindle and iOS: a back button and a title on
each page. Advanced things stay in the Kindle UI.

- [ ] Page stack: Settings → page → sub-page, with back.
- [ ] Settings home: one row per page, with its current value (for example the Wi-Fi network's name).
- [ ] Wi-Fi & network page (see M5).
- [ ] Display page: frontlight, warmth, rotation, auto-rotation. The controls exist on the single page today.
- [ ] Sleep page: sleep screen choice and sleep pictures. The choice exists today.
- [ ] Apps page: hidden apps, Extensions. Hidden apps exist today.
- [ ] Device page: start at boot, SSH, Kindle UI, restart the launcher.
- [ ] About page: version, memory, free storage, IP address, the app list.

## M5 Wi-Fi networks

Phase 2. Join networks from the launcher, as on the stock Kindle. Uses `com.lab126.wifid` (S3).

- [ ] Wi-Fi on and off.
- [ ] Network list from a scan: name, signal bars, a lock for secured networks, "Connected" and "Saved"
      marks; saved and stronger networks first.
- [ ] Join a saved network with one tap.
- [ ] On-screen keyboard: the Primitive Browser layout (letters, numbers, symbols, shift).
- [ ] Password page for WPA/WPA2 networks, with a show-password switch.
- [ ] Enterprise networks: user name and password, if `createProfile` accepts them (to be tested; no
      enterprise network nearby yet).
- [ ] Forget a saved network.
- [ ] Join a hidden network by name.
- [ ] Clear messages for a wrong password or no address.

## M6 System duties

Phase 2. **Gate 2** after this milestone: a day of normal use without the Kindle UI (reading in KOReader,
browsing, Wi-Fi on/off and joining a network, 10 sleep/wake cycles, a USB file copy, rotation both ways).

- [x] Low-battery notice at 10 % and 5 %; critical battery shows the sleep screen with a warning (Mac).
- [x] Auto-rotation from the accelerometer: the S4 rule, 4 readings a second, 800 ms hold (v0.2.4; confirmed by
      the user).
- [ ] USB "Connect to computer" (needs S5).
- [ ] Network time (optional; the clock is right today).

## M7 Boot into the launcher

Phase 3, v0.3.x, opt-in. **Gate 3**: every reboot test ends on the expected screen without SSH.

- [x] Boot job, guard script, install and remove scripts, 15 guard checks (Mac).
- [x] "Start at boot" switch in Settings (creates or removes `DISABLE`) (Mac).
- [ ] A way to start SSH without the framework, checked before the first reboot.
- [ ] Reboot tests: normal boot ×5, the `DISABLE` and `BOOT_KINDLE` files, a broken launcher (autostart
      turns itself off on the third boot), USB cable in at boot, the removal script.

## M8 Extensions and customising

Phase 4, v0.4.x.

- [x] Extensions tile with KUAL menus and submenus; menu file from `config.xml` (Mac).
- [x] Background mode for apps and KUAL `exitmenu: false` items (Mac).
- [x] Hide apps; Settings shows them again (on the device since v0.1.x, not confirmed by the user).
- [x] Move apps and choose an icon from `icons/` (Mac).
- [x] Sleep pictures from `screensavers/` (Mac).
- [x] The app list is written to the log at start (Mac).
- [ ] KUAL `status`, `checked` and model `if` checks.
- [ ] Research only: never start the framework at boot (M6c), after Phase 3 is stable for weeks.

---

## Feasibility

| Part | Feasibility | Why |
|---|---|---|
| Stopping and restarting the framework | High | Done; the return takes about a minute. |
| Launcher UI, apps, e-ink | High | Same device layer and crates as Primitive Browser. |
| Sleep and wake without the framework | High | Works with the holder (S2). |
| Wi-Fi networks and passwords | Medium-high | `wifid` has scan, profile and connect calls (S3); KOReader uses the same ones. Hash properties go through `lipc-hash-prop`. |
| Apple-style grid on e-ink | High for layout; medium for animation | Ghosting limits animation to a few fast frames. |
| Auto-rotation | Medium | Codes alternate when tilted; debouncer ready, manual fallback. |
| USB drive mode on demand | Medium | `volumd` pause proven; entering drive mode unverified (S5). |
| Boot into the launcher | High with care | The framework boots first; every failure falls back to stock. |

## Test strategy

| Level | What | Where |
|---|---|---|
| Unit | Parsers, layout, state machine, debouncer, Wi-Fi list parsing, keyboard | Mac, `cargo test` |
| Screenshots | The desktop driver draws each screen to PNG for review | Mac |
| Shell | Supervisor and boot scripts against stubs | Mac |
| Device | Milestone checklists; log lines `[launcher …]`, `[kl …]` | Kindle, with the user watching |

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| An app restarts the framework on exit | Medium | Stock UI behind the launcher | Supervisor checks after each app |
| A wrong Wi-Fi profile call breaks saved networks | Low | Networks to re-enter in the Kindle UI | Read-only checks first; test on one network; never delete without the user |
| Animations leave ghosts | Medium | Smudged screen | Few fast frames, a clean frame after; drop if bad |
| Accelerometer too noisy | Medium | Rotation flips | Debounce; manual setting |
| USB inhibit blocks charging or drive mode | Low | No computer access from the launcher | S5; the Kindle UI always offers USB |
| Boot job mistake | Low | Stock UI; worst case SSH | Framework boots first; USB marker files; boot counter |
| SIGBUS from files changing under a program | Known | Crash | Run from `/var/tmp`; `volumd` paused; stop before install |
