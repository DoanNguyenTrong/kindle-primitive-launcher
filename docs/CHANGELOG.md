# Changelog

Each device build gets a new patch version; a new phase starts a new minor version. Releases are the versions
with a `v*` tag.

## 0.3.6

- USB: plugging the cable in shows the drive on the computer and the launcher's drive-mode picture in place of the
  stock USB screen; ejecting returns to the grid. The launcher and its supervisor no longer hold `/mnt/us`, which the
  Kindle unmounts for the drive, so they keep running through it.

## 0.3.2

- Leaving the launcher always starts the Kindle UI again. A launcher that started while the framework was already
  stopped did not record that, so leaving left the screen blank.
- volumd keeps running while the launcher's grid is in front, so a USB cable gives the Kindle's drive mode, as the
  stock UI does. It is paused only while an app runs, so the app's files do not change under it.

## 0.2.24

- Settings > Device has a Launcher section with "Restart the launcher", which starts the launcher again as after an
  install.
- Wi-Fi: a network that did not connect says why: "check the password" when it was in range, "not in range now" when
  it was not. A connection that gets no address from the network is reported as such, and the join waits for one.

## 0.2.21

- The SSH server is the launcher's own: Dropbear and its zlib ship in the package (`ssh/bin`, `ssh/lib`), and the host key
  stays in `ssh/` across installs. The first install copies the key and the password flag from the browser's files, so
  nothing is lost. Primitive Browser no longer ships or runs an SSH server.
- Public DNS is the launcher's: `dns.sh`, with its backup in `dns/`; the first install keeps the browser's backup.
- Settings has a page for SSH & DNS, under Network. SSH: the switch (the quick settings' SSH switch does the same),
  the root password (set on the keyboard), the connect command while the server runs, and a check of the server.
  DNS: public DNS on or off (1.1.1.1 and 8.8.8.8), with the network's own settings kept for Restore.
- SSH opens the Kindle's firewall for port 2222 when it starts and closes it when it stops.
- Primitive Browser's Diagnostics page is gone; its SSH and DNS features are here.

## 0.2.20

- Quick settings has a System section: CPU, memory and storage, each with an icon, a bar and its value. The status
  bar no longer shows them. The icons and figures are shared with Primitive Browser (`primitive-quick`).
- Wi-Fi: "Other…" joins a network that does not show in the list (name, then password; an empty password joins an
  open network). With Wi-Fi off the list says so, and turning Wi-Fi on from the page starts a scan.
- The keyboard takes its keys, spacing and colours from the shared keyboard (`primitive-keyboard`), as in Primitive
  Browser.

## 0.2.7

- The first published release. The release workflow installs zig and cargo-zigbuild with setup actions (pinned
  to the versions the device builds use); the v0.2.6 release run failed while installing them, so 0.2.6 has a tag
  but no release package.
- The README says how to reach us if Amazon wants the Kindle icon removed.

## 0.2.6

- The library icon has the Primitive logo in its centre tile, and rounded-square tiles like the home screen.
- KOReader's library entry runs through the launcher's wrapper by default, so leaving KOReader returns to the
  launcher without starting the stock UI. `apps.toml` can still give KOReader another command.
- First version on GitHub, with CI and a release workflow.

## 0.2.0 – 0.2.5

- **Home screen in the style of a phone:** rounded-square icons of one size on a 4 × 5 grid, names under them,
  pages of 20 with dots at the bottom; swipe or the page buttons change pages.
- **Tap feedback:** only the touched icon darkens. The screen updates only the rectangle that changed.
- **Opening animation:** a card grows from the icon to the screen when an app opens; it can be turned off in
  Settings.
- **App menu:** a long press opens a box beside the icon: move earlier or later, change icon, hide.
- **Dark mode:** white on black for the home screen, Settings, menus and dialogs; picture icons keep their look.
- **Kindle UI:** the confirmation is a pop-up over the grid, and turns into "Opening the Kindle UI" with a
  progress bar until the library appears.
- **Icons:** the Kindle UI tile has the Kindle reader icon; built-in tiles have drawn icons.
- **Auto-rotation:** the screen follows the Kindle's turn (upright or upside down), read from the accelerometer
  with the stock UI stopped.
- **Sleep:** the stock screensaver shows straight away; the launcher no longer draws its own sleep screen first.
- **Status bar:** Wi-Fi and battery icons from Primitive Browser.
- **Extensions:** KUAL extensions behind one Extensions tile, with submenus; items with `exitmenu: false` run in
  the background.
- Sleep pictures from `launcher/screensavers/`; low-battery warnings at 10 % and 5 %.

## 0.1.11

- The first working launcher (Gate 1): the stock framework stops while the launcher runs; Primitive Browser and
  KOReader open and return; sleep and wake; the in-place Kindle UI return; a killed supervisor brings the stock
  UI back.
