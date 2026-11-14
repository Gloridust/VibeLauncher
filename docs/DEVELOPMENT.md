# Development

## How it works

Both apps are Electron apps. Electron keeps one running instance **per data folder**, so starting
the app with `--user-data-dir=<folder>` gives an independent instance with its own storage.
VibeLauncher adds the app-specific settings that make the separation complete:

| | Data folder | Plus |
|---|---|---|
| Claude | `--user-data-dir=<profile>/electron` | `CLAUDE_CONFIG_DIR=<profile>/claude-config` |
| ChatGPT | `--user-data-dir=<profile>/electron` | `CODEX_ELECTRON_USER_DATA_PATH`, `CODEX_HOME=<profile>/codex-home` |

The app is found fresh every time (desktop entries including Flatpak and Snap on Linux; Spotlight
and `/Applications` on macOS; Store packages and per-user installs on Windows), so moving or
updating it doesn't break profiles.

To route sign-in links, VibeLauncher stands in for the app's link handler only while you have
isolated profiles of it, and hands it back when you turn routing off:

- **Linux:** a same-named desktop entry in `~/.local/share/applications` that runs VibeLauncher
  and falls back to the app if VibeLauncher is gone. The system file is never edited. Claude
  writes its own copy of that entry on every start; VibeLauncher replaces it (keeping its extra
  menu actions), Claude then leaves it alone, and the app's copy is put back on release.
- **macOS:** a tiny router app registered for the scheme, which falls back to the app if
  VibeLauncher is gone.
- **Windows:** the per-user (`HKCU`) handler, with the previous value backed up. The installer
  gives it back on uninstall and keeps it across upgrades.

Design notes and measurements (in Chinese): [ARCHITECTURE.md](ARCHITECTURE.md).

## Command line

Every package installs a small `vibelauncher` command next to the window. Shortcuts and link
handling run it, so the window never needs to be open.

```text
vibelauncher list [--json]           List profiles and whether they are running
vibelauncher launch <profile> [URL]  Open a profile (and hand it a link)
vibelauncher login <profile>         Open a profile to sign in
vibelauncher quit <profile>          Ask a profile's app to quit
vibelauncher create <name> --app claude|chatgpt [--shortcut]
vibelauncher delete <profile> [--delete-data]
vibelauncher doctor [--fix]          Find and fix common problems
vibelauncher scheme status|claim|release
vibelauncher log                     Recent link-routing decisions
vibelauncher cleanup                 Before uninstalling: give links back to the apps, remove shortcuts
```

Profile data lives in `~/.local/share/vibelauncher/profiles` (Linux),
`~/Library/Application Support/io.github.Gloridust.VibeLauncher/profiles` (macOS) or
`%USERPROFILE%\VibeLauncher Profiles` (Windows); any folder can be chosen per profile. Set
`VIBELAUNCHER_HOME` to keep all of VibeLauncher's own files in one place.

## Building

Requirements: Rust (stable), Node.js 20+ with pnpm, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your system
(on Debian/Ubuntu: `libwebkit2gtk-4.1-dev`, `librsvg2-dev`).

```bash
cargo build --release -p vibelauncher        # the command (target/release/vibelauncher)
cd app && pnpm install
pnpm tauri build                              # the window, plus packages
```

To ship the command inside the packages, copy it next to the window first and build with the
bundle config:

```bash
cp target/release/vibelauncher app/src-tauri/binaries/vibelauncher-$(rustc -vV | sed -n 's/host: //p')
cd app && pnpm tauri build --config src-tauri/tauri.bundle.conf.json
```

UI work without Tauri: `cd app && pnpm dev`, then open
`http://localhost:1420/?scenario=busy` (also `fresh`, `noapps`, `route`; add `&lang=zh`,
`&theme=dark`, `&platform=macos`). A mock backend is used; it isn't part of release builds.

## Tests

- `cargo test --workspace` — unit tests.
- End to end, against stand-in apps:
  `VL_SMOKE=sandbox cargo test -p vibelauncher --features smoke-test --test smoke`
  (sandbox mode keeps everything in temporary folders).
- CI runs the end-to-end test on Linux, macOS and Windows, opens the real window on macOS and
  Windows and clicks through it, and on Windows installs, upgrades and uninstalls the package.
- What CI can't cover — a real sign-in with the real apps — is the checklist in
  [TESTING.md](TESTING.md) (in Chinese).

Releasing and signing: [RELEASING.md](RELEASING.md) (in Chinese).
