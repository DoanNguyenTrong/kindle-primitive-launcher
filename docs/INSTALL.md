# Install Primitive Launcher

Primitive Launcher runs on a **jailbroken** Kindle. It is verified on the Kindle Oasis 3 (firmware 5.18.2)
only. It does not jailbreak the Kindle and does not change any file of the Kindle's own software.

## What you need

- A jailbroken Kindle Oasis 3. Other models are not supported yet.
- A computer and a USB cable, or SSH access to the Kindle.
- Optional: [KOReader](https://koreader.rocks) and [Primitive Browser](https://github.com/DoanNguyenTrong/kindle-primitive-browser).
  The launcher finds them by their library entries (`KOReader.sh`, `Primitive Browser.sh`).

## Install with a USB cable

Install the launcher over USB for the first setup. Launcher updates through Primitive Store require the launcher
self-update flow and a published listing.

1. Download `primitive-launcher-<version>.zip` from the
   [latest release](https://github.com/DoanNguyenTrong/kindle-primitive-launcher/releases/latest) and unzip it.
   You can also use a `primitive-launcher-<version>-armhf.zip` package when one is available.
2. Connect the Kindle to the computer **while the stock Kindle UI is on the screen**. (While the launcher runs,
   the Kindle charges over USB but does not show up as a drive. Tap *Kindle UI* in the launcher first.)
3. Copy the `launcher` and `documents` folders onto the root of the Kindle drive. Merge `documents` with the
   folder that is already there; your books stay where they are.
4. Eject the Kindle and unplug it.
5. Open **Launcher** in the Kindle library. The stock UI stops and the launcher takes the screen. The first time, the
   launcher also sets up its boot start (a few seconds more): a cable in at boot then shows its picture, and SSH starts
   by itself. No computer is needed for this.

To check a download, compare its SHA-256 with the `.sha256` file next to it in the release:

```sh
shasum -a 256 primitive-launcher-<version>.zip      # macOS and Linux
certutil -hashfile primitive-launcher-<version>.zip SHA256    # Windows
```

## Install over SSH (developers)

With the SSH server running on the Kindle (start it in the launcher's Settings > SSH & DNS, or with the quick
settings' SSH switch), build and install from a checkout:

```sh
cargo zigbuild --release -p kl-launcher --target armv7-unknown-linux-musleabihf
KINDLE_HOST=<kindle-ip> sh deploy/install-ssh.sh
```

`scripts/build-device.sh` does the same and bumps the patch version first.

## Using it

- **Open:** *Launcher* in the Kindle library.
- **Leave:** tap *Kindle UI*. The stock Kindle UI starts in about a minute, and *Launcher* in its library brings
  you back.
- **Apps:** library entries in `documents/` (`*.sh`) appear by themselves. Others can be added in
  `launcher/apps.toml`; `launcher/apps.toml.example` shows how.
- **Edit:** long-press an app to move it, change its icon (PNG files in `launcher/icons/`) or hide it.
- **Settings:** frontlight, warmth, dark mode, Wi-Fi, SSH and public DNS (SSH & DNS), screen turn, the opening
  animation and the sleep screen.

Your settings are in `launcher/settings.txt` and `launcher/apps.toml`. The log is `launcher/log-launcher.txt`.

## Upgrade

Copy the new `launcher` and `documents` folders over the old ones. `apps.toml`, `settings.txt`, your icons and
your sleep pictures are not in the package, so they are kept.

## If something goes wrong

- The launcher starts the stock Kindle UI again by itself if it crashes repeatedly or stops answering.
- Restarting the Kindle starts the launcher again once Launcher has been opened once. To get the stock Kindle UI at
  the next restart, put an empty file named `BOOT_KINDLE` at the root of the Kindle drive (over USB). To stop the
  launcher starting at a restart, put an empty file named `DISABLE` in the `launcher` folder.

## Uninstall

Delete `documents/Launcher.sh` and the `launcher` folder from the Kindle (from the stock Kindle UI, over USB).
The boot job and its copy on the Kindle's system storage stay, but they do nothing once the `launcher` folder is gone.
