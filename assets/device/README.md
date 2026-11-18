# Device tools for the SSH server (Kindle Oasis 3, armv7)

The launcher runs its own SSH server, so it does not depend on Primitive Browser or KOReader being installed.
`deploy/install-ssh.sh` sends the two files to `/mnt/us/launcher/ssh/` when their SHA-256 differs, and
`scripts/package-release.sh` puts them in the release package.

| File | Version | Source | Used for | License |
|---|---|---|---|---|
| `armv7/dropbear` | Dropbear 2026.92 | https://github.com/mkj/dropbear/tree/DROPBEAR_2026.92 | SSH server (Settings > SSH & DNS) | MIT-style and others, text in `licenses/DROPBEAR-LICENSE.txt` |
| `armv7/libz.so.1` | zlib 1.3.2 | https://zlib.net (and https://github.com/madler/zlib) | Dropbear's dependency | zlib license, text in `licenses/ZLIB-LICENSE.txt` |

The Dropbear and zlib binaries are built from the upstream sources at the versions above. Dropbear's banner says
"(KOReader)", so the build was made by KOReader; the build recipe and any patches are KOReader's. Building these
from upstream for this device is still to do.

Checksums (SHA-256), copied from the browser's device tools (the same files):

| `armv7/dropbear` | Dropbear 2026.92 | https://github.com/mkj/dropbear/tree/DROPBEAR_2026.92 | SSH server (Settings > SSH & DNS) | MIT-style and others, text in `licenses/DROPBEAR-LICENSE.txt` |
     191576 bytes  0e4340bcc9f098f87f0a27bee3f9cb0ee34a40a53e76e7c6367a2644f7898297  armv7/dropbear
      63172 bytes  79a78432f05a2dff2db4e518a13827c7979aeed10b1fa7cdc9aa0e350e78416b  armv7/libz.so.1
