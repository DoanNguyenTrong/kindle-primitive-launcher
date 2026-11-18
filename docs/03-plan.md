# 3. Plan

Status date: 2026-11-18. On the Kindle: **v0.2.14**. Gate 1 passed with v0.1.11.

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
| [M3 Status bar](#m3-status-bar) | 2 / 0.2.x | **Done**, awaiting the user's device check (v0.2.13) |
| [M4 Settings pages](#m4-settings-pages) | 2 / 0.2.x | Grouped pages built (v0.2.15, Mac); device check next |
| [M5 Wi-Fi networks](#m5-wi-fi-networks) | 2 / 0.2.x | List, join with password, forget done (Mac, v0.2.17); device check next |
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

Phase 2. A bar like the stock Kindle's: several indicators, each with its own state. Tapping the bar does not
open quick settings; a swipe down from the top edge does.

- [x] Date and time on the left; indicators, Wi-Fi and battery on the right, the same places as in quick settings (v0.2.12).
- [x] Wi-Fi icon with signal strength: three arcs filled from `signalStrength` (4/5 shows three); a slash when off;
      "No network" beside it when on without a network (v0.2.8).
- [x] Indicators shown only when they matter: SSH, a running background job; the slashed Wi-Fi shows airplane mode (v0.2.8).
- [x] Battery icon with level, number and charging bolt (from Primitive Browser).
- [x] Quick settings: a swipe down from the top edge opens a sheet like Primitive Browser's: clock and date,
      Wi-Fi and battery, round icon buttons (Airplane, Auto-rotate, Dark mode, Animation, SSH, Settings), two
      25-step level rows with − and +, and a swipe up to close (v0.2.8 to v0.2.11).
- [x] System section in quick settings: CPU, memory and storage, each with an icon, a bar and its value (v0.2.20).
      The status bar does not show them. Primitive Browser's quick settings show the same figures.
- [x] Live state: battery, charging, Wi-Fi and clock are re-read every 5 s; the bar is redrawn only when it
      would show something new (v0.2.13).

Not in M3, later: Bluetooth (see "Later" below).

## M4 Settings pages

Phase 2. Settings is a list of pages in groups; each page is sections with a header, a card, and a footer that
explains it. Design rules, with the sources and their limits: [settings-design.md](settings-design.md).

- [x] Page stack: Settings → page → Back (v0.2.14).
- [x] Settings list grouped into Network, Display, System and About, each page with its current value (v0.2.15).
- [x] Switches for on and off (Wi-Fi, dark mode, opening animation, SSH, start with the Kindle, sleep screen, auto-rotate).
- [x] Wi-Fi & network page: the Wi-Fi switch; the current network; the networks in range with signal, lock and
      "Saved"; Scan; scrolling (v0.2.15). The scan list is read from the device; joining is M5.
- [x] Display page: frontlight and warmth (round buttons), dark mode, opening animation, screen rotation with a tap-to-turn value (v0.2.15).
- [x] Sleep page: the sleep screen choice (v0.2.15).
- [x] Apps page: hidden apps (Show all) (v0.2.15).
- [x] Device page: start with the Kindle, Kindle UI (v0.2.15; SSH moved to its own page in v0.2.21).
- [x] SSH & DNS page (v0.2.21): the SSH switch (the quick settings' SSH switch does the same), the root password,
      the connect command while it runs, a check of the server, and public DNS (1.1.1.1, 8.8.8.8) with Restore.
      Moved from the browser's Diagnostics page, which is gone. Device checks pending.
- [x] Device page: restart the launcher, as after an install (v0.2.24, device check pending).
- [x] About page: version, apps, memory, free storage, Wi-Fi address (v0.2.15).

## M5 Wi-Fi networks

Phase 2. Join networks from the launcher, as on the stock Kindle. Uses `com.lab126.wifid` (S3, and the stock
Kindle's own calls as KOReader makes them).

- [x] Wi-Fi on and off (v0.2.15).
- [x] Network list from a scan: name, signal arcs, a lock for secured networks, "Connected" and "Saved" marks;
      strongest first (v0.2.15). Parsed against the Kindle's real scan output (Mac test).
- [x] Join a saved network: Connect on the network's screen, no password (v0.2.17, device check pending).
- [x] On-screen keyboard: the shared keyboard (`primitive-keyboard`, used by Primitive Browser too): the four
      layers, the bottom row with ‹ and ›, space, Go (v0.2.18).
- [x] Password for a new WPA2 network, with Show password; at least 8 characters (v0.2.17, device check pending).
      The password goes to the connection tool on its input only, and is not logged or kept after joining.
- [x] Forget a saved network, from its screen or by tapping the connected network (v0.2.19, device check pending).
- [x] Long lists: a scroll bar and the count; a swipe moves by the rows it covers (v0.2.19, checked with 30 networks). Tested on the Kindle: a test profile was created
      and deleted, and the saved list was back to its earlier count.
- [ ] Enterprise networks: user name and password, if `createProfile` accepts them (to be tested; no
      enterprise network nearby yet).
- [x] Join a hidden network by name: "Other…" opens a name field and the keyboard (Next asks for the password;
      an empty password joins an open network) (v0.2.20, device check pending).
- [x] Wi-Fi off: the list says so; turning Wi-Fi on from the page starts a scan (v0.2.20, device check pending).
- [x] Clear messages for a wrong password ("check the password", or "not in range now") and for no address (the
      network gave none; the join waits for DHCP before it says connected) (v0.2.24, device check pending).

Note for the device: `profileData` holds the saved passwords in plain text. The launcher never prints or logs it.

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

## Later

Not scheduled. Each item is picked up when the user asks for it.

- [ ] **Bluetooth.** The Oasis 3 has the stock Bluetooth service (`com.lab126.btfd`), which reports Bluetooth
      off (`BTstate` 0), and no Bluetooth radio shows in the kernel, so the hardware is unconfirmed. To do: check
      the radio from the stock UI, then decide on a status bar indicator and whether to pair from the launcher.

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
