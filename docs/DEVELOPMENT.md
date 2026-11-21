# Development

## How it works

Both apps are Electron apps. Electron keeps one running instance **per data folder**, so starting
the app with `--user-data-dir=<folder>` gives an independent instance with its own storage.
VibeLauncher adds the app-specific settings that make the separation complete:

| | Data folder | Plus |
|---|---|---|
| Claude | `--user-data-dir=<profile>/electron` | `CLAUDE_CONFIG_DIR=<profile>/claude-config` |
| ChatGPT | `--user-data-dir=<profile>/electron` | `CODEX_ELECTRON_USER_DATA_PATH`, `CODEX_HOME=<profile>/codex-home` |

No profile inherits another one's folder from VibeLauncher itself: if VibeLauncher was opened
from inside a profile (from a Claude Code session in it, say), an isolation variable pointing
into a profile's data is dropped at launch, and with `CLAUDECODE` set so are that session's
`CLAUDE*` variables; a folder the user set elsewhere is kept (`inherited_to_remove` in
`crates/vl-core/src/provider.rs`). Diagnostics reports a default profile already running with
one (`borrowed-env`).

Other apps can be added (**Other app…**, or `vibelauncher app add`). Only Electron and Chromium
apps are accepted, because only they keep everything in a folder VibeLauncher can choose; they are
recognised by the files every build ships (`resources/app.asar`, `v8_context_snapshot.bin`, the
Electron framework in a `.app`; an AppImage is mounted to look inside). Flatpak and Snap apps are
turned away, since their sandbox decides where data goes. An added app's profiles get only
`--user-data-dir`, and no link routing. Because an app can still pick its own folder in code,
Diagnostics warns when a profile's folder is still empty after it ran
(`crates/vl-core/src/detect.rs`, and `not-isolated` in `doctor.rs`). Added apps are stored as
`[[app]]` in `profiles.toml`, which is then written as schema 2 so older versions ask to be
updated instead of misreading it.

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

Updates: the window checks `latest.json` on the latest GitHub release when it opens (unless
turned off in Settings). With the beta program on (Settings; the default in a beta build), it
reads `releases.atom` instead and uses the manifest of the highest published version,
prereleases included; if that fails, it falls back to the latest release. Installing downloads
the package of the same kind as the running copy, checks its signature against the public key
(`app/src-tauri/src/update.rs`, also in `tauri.conf.json`), and installs it: the AppImage
and the `.app` replace themselves, Windows runs the installer in update mode (the hooks keep
links and shortcuts), and deb/rpm packages are installed with a single `pkexec` prompt. A build
that isn't a package (`cargo run`) only points to the download page.

Design notes and measurements (in Chinese): [ARCHITECTURE.md](ARCHITECTURE.md).

## Command line

Every package installs a small `vibelauncher` command next to the window. Shortcuts and link
handling run it, so the window never needs to be open.

```text
vibelauncher list [--json]           List profiles and whether they are running
vibelauncher launch <profile> [URL]  Open a profile (and hand it a link)
vibelauncher login <profile>         Open a profile to sign in
vibelauncher quit <profile>          Ask a profile's app to quit
vibelauncher create <name> --app claude|chatgpt|<added app> [--shortcut]
vibelauncher delete <profile> [--delete-data]
vibelauncher app add <path> [--name N]  Add an Electron or Chromium app
vibelauncher app list|remove <app>
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

The window is Rust + [Slint](https://slint.dev), drawn with Slint's software renderer: no GPU,
no WebKit, so graphics drivers can't garble it. Its interface is in `app/src-tauri/ui/*.slint`,
its logic in `app/src-tauri/src/`. (The folder keeps its old name because the Tauri bundler,
still used to make the packages, looks for it.)

Requirements: Rust (stable); on Debian/Ubuntu also `libfontconfig1-dev` and `libxkbcommon-dev`.
Making packages also needs Node.js 20+ with pnpm (for the Tauri bundler and signer only).

```bash
cargo build --release -p vibelauncher        # the command (target/release/vibelauncher)
cargo build --release -p vibelauncher-gui    # the window
```

To make packages, put the command next to the window and run the bundler:

```bash
cp target/release/vibelauncher app/src-tauri/binaries/vibelauncher-$(rustc -vV | sed -n 's/host: //p')
cd app && pnpm install && pnpm tauri bundle --config src-tauri/tauri.bundle.conf.json
```

On Linux the bundler needs `libgtk-3-dev` and `rpm`, and its packages are made for a WebKit
window: the deb and rpm depend on WebKitGTK and the AppImage carries GTK. Release builds then run
`.github/scripts/linux-slim.sh <target>` (it reads `target/<target>/release/bundle`), which makes
them again without either.

UI work without a screen: build with `--features selftest` and run
`VL_SHOT=<folder> target/release/vibelauncher-gui` to save every page and sheet as PNG files, in
light and dark (it only reads your configuration; set `VIBELAUNCHER_HOME` to use another one).

## Tests

- `cargo test --workspace` — unit tests.
- End to end, against stand-in apps:
  `VL_SMOKE=sandbox cargo test -p vibelauncher --features smoke-test --test smoke`
  (sandbox mode keeps everything in temporary folders).
- CI runs the end-to-end test on Linux, macOS and Windows, opens the real window on all three
  and clicks through it (`app/src-tauri/src/selftest.rs`, off-screen with the real window
  logic), and on Windows installs, upgrades and uninstalls the package.
- What CI can't cover — a real sign-in with the real apps — is the checklist in
  [TESTING.md](TESTING.md) (in Chinese).

Releasing and signing: [RELEASING.md](RELEASING.md) (in Chinese).
