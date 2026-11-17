# Primitive Launcher (working name)

A simple home screen that becomes the interface of a jailbroken Kindle Oasis 3. All apps, including
Primitive Browser, KOReader and KUAL extensions, are tiles on one grid. The stock Kindle UI is retired
from daily use: its framework is stopped while the launcher runs. It stays installed, unmodified, as a
"Kindle mode" escape hatch and as the fallback for any failure.

Status (2026-11-18): Phase 1 done; Gate 1 passed on the Kindle with v0.1.11. Phases 2–4 partly written (see docs/03-plan.md).

## Documents

| Doc | Contents |
|---|---|
| [docs/00-research.md](docs/00-research.md) | Decision log, prior art (KUAL, KUAL Next, OpenReader, KOReader), ways to replace the stock home, what we reuse |
| [docs/01-requirements.md](docs/01-requirements.md) | Acceptance criteria, functional and non-functional requirements, scope |
| [docs/02-architecture.md](docs/02-architecture.md) | Framework duties and who takes them over, supervisor protocol, states, sleep/Wi-Fi/USB/rotation, boot job, failure modes |
| [docs/03-plan.md](docs/03-plan.md) | Feasibility, device spikes, phases with gates, tests, risks |

## Agent skills

`.agents/skills/kindle-device` and `.agents/skills/kindle-eink` were copied from
`../doannt-kindle-browser`. They describe SSH access, safe installs, storage limits and e-ink drawing on
this Kindle. Read them before any device work. Primitive Browser is the reference implementation for the
device layer this project reuses.

## Build and test (Mac)

```sh
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/opt/homebrew/bin:$PATH"
cargo test --offline -j 4 --workspace          # core, renderer, device code
sh tests/supervisor-test.sh                    # supervisor against stub Kindle commands
cargo run --offline --release -p kl-desktop -- --documents DIR --out out/   # screens as PNG
cargo zigbuild --offline --release -p kl-kindle --target armv7-unknown-linux-musleabihf
scripts/build-device.sh                        # bump patch version, test, build, install over SSH
```

## Layout

| Path | What |
|---|---|
| `crates/kl-core` | App discovery (scriptlets, `apps.toml`), settings, layout, UI state machine |
| `crates/kl-render` | Drawing to an 8-bit gray buffer (`vello_cpu`, `parley`), icons, dithering |
| `crates/kl-kindle` | The device binary `kl-launcher`: e-ink, touch, buttons, power events, `lipc` |
| `crates/kl-desktop` | PNG screenshots on the Mac; draws `assets/launcher-icon.png` |
| `deploy/supervisor.sh` | Stops/starts the framework, runs the launcher and apps, watchdog |
| `deploy/library-entry.sh` | Installed as `documents/Launcher.sh`: opens the launcher from the Kindle library |
| `deploy/install-ssh.sh` | Installs over SSH |
