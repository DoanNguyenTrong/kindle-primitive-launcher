# Changelog

Each device build gets a new patch version; a new phase starts a new minor version. Releases are the versions
with a `v*` tag.

## 0.2.6

- The library icon has the Primitive logo in its centre tile, and rounded-square tiles like the home screen.
- KOReader's library entry runs through the launcher's wrapper by default, so leaving KOReader returns to the
  launcher without starting the stock UI. `apps.toml` can still give KOReader another command.
- First release on GitHub, with CI and a release package.

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
