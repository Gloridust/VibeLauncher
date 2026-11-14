<p align="center">
  <img src="app/public/logo.svg" width="88" alt="">
</p>

<h1 align="center">VibeLauncher</h1>

<p align="center">
  Separate profiles for the official Claude and ChatGPT desktop apps:<br>
  work and personal, or one per client — each with its own sign-in, chats and settings.
</p>

<p align="center"><a href="#中文说明">中文说明</a></p>

---

VibeLauncher is a small launcher. It opens the desktop app **you already installed** with a
different data folder for each profile. It doesn't modify, copy, repackage or redistribute the
apps, and it never reads or copies sign-in data: every profile signs in on its own. Official
updates keep working exactly as before.

> VibeLauncher is an independent project. It is not affiliated with, endorsed by or supported by
> Anthropic or OpenAI. "Claude" and "ChatGPT" are trademarks of their owners.

## What it does

- **Profiles.** Each profile runs the app with its own data folder, so sign-ins, chats, settings
  and caches stay apart. The app's normal install is kept as the "default" profile, untouched.
- **Shortcuts.** Each profile can get its own entry in the app menu (Linux), `~/Applications`
  (macOS) or the Start menu (Windows). Shortcuts open the profile directly — the VibeLauncher
  window doesn't need to be running.
- **Sign-in that comes back to the right profile.** Signing in finishes in the browser, which
  sends a `claude://` / `codex://` link back to the app. With several profiles the system would
  hand it to whichever one it likes. VibeLauncher marks the profile you're signing in to (for five
  minutes) and passes the link there; if it can't tell, it asks. If the link never arrives you
  can paste it.
- **Diagnostics.** Finds the things that quietly mix profiles up — a profile that an update
  restarted without its settings, profiles that start (and so update) at the same moment, link
  handlers that were taken over — and fixes them in one click.
- **Light.** The launcher itself is a 1.7 MB command. The manager window is only open while you
  use it; nothing stays in the background.

## Platforms

| | Claude | ChatGPT | Sign-in routing |
|---|---|---|---|
| **Linux** | ✅ tested | ✅ lightly tested | ✅ |
| **macOS** | passes CI end to end; not yet tried with a real account | same | experimental |
| **Windows** | passes CI end to end; not yet tried with a real account | same | experimental |

On every change, CI runs the whole command-line flow on all three systems, for both apps, with
stand-ins for them: it creates profiles, launches, checks each profile is recognised, quits every
copy, catches an instance that lost its settings, and has the system itself deliver a sign-in link
(`claude://`, `codex://`) through VibeLauncher. What CI can't check on macOS and Windows is a real
sign-in with the real apps, and the manager window (it is built and packaged there, but only
tried by hand on Linux). Reports are very welcome — please
[open an issue](https://github.com/Gloridust/VibeLauncher/issues).

## Install

Download a package from [Releases](https://github.com/Gloridust/VibeLauncher/releases):

- **Linux:** `.deb` or `.rpm` (recommended), or the `.AppImage`.
- **macOS:** `.dmg` (Apple silicon and Intel).
- **Windows:** the `-setup.exe` installer (per-user, no admin rights).

The macOS and Windows packages aren't code-signed yet, so the system asks once before opening them:

- **macOS:** drag VibeLauncher to Applications, then right-click it and choose **Open** (or allow it
  under System Settings → Privacy & Security). If macOS says the app is damaged, run
  `xattr -dr com.apple.quarantine /Applications/VibeLauncher.app`.
- **Windows:** if SmartScreen appears, choose **More info → Run anyway**.

Or build it yourself — see [Building](#building).

## Using it

Open VibeLauncher, choose **New profile**, give it a name, and click **Sign in…**. The app opens
with the new profile; sign in there, allow the browser to open the app when it asks, and you're
done. From then on use the profile's shortcut, or the window.

Everything is also available from the `vibelauncher` command:

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
```

Profile data lives in `~/.local/share/vibelauncher/profiles` (Linux),
`~/Library/Application Support/io.github.Gloridust.VibeLauncher/profiles` (macOS) or
`%USERPROFILE%\VibeLauncher Profiles` (Windows); any folder can be chosen per profile. Set
`VIBELAUNCHER_HOME` to keep all of VibeLauncher's own files in one place.

## How it works

Both apps are Electron apps. Electron keeps one running instance **per data folder**, so starting
the app with `--user-data-dir=<folder>` gives an independent instance with its own storage.
VibeLauncher adds the app-specific settings that make the separation complete:

| | Data folder | Plus |
|---|---|---|
| Claude | `--user-data-dir=<profile>/electron` | `CLAUDE_CONFIG_DIR=<profile>/claude-config` |
| ChatGPT | `--user-data-dir=<profile>/electron` | `CODEX_ELECTRON_USER_DATA_PATH`, `CODEX_HOME=<profile>/codex-home` |

The app is found fresh every time (desktop entries including Flatpak and Snap on Linux; Spotlight and
`/Applications` on macOS; Store packages and per-user installs on Windows), so moving or updating it
doesn't break profiles. You can also point
VibeLauncher at an unusual install in Settings.

To route sign-in links, VibeLauncher stands in for the app's link handler only while you have more
than one profile of it, and hands it back when you turn routing off:

- **Linux:** a same-named desktop entry in `~/.local/share/applications` that runs VibeLauncher and
  falls back to the app if VibeLauncher is gone. The system file is never edited.
- **macOS:** a tiny router app registered for the scheme.
- **Windows:** the per-user (`HKCU`) handler, with the previous value backed up.

More detail, including measurements and design notes, is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Known limitations

- **Claude's Cowork is shared.** Cowork's virtual machine belongs to your system account, not to
  the app's data folder, so only one profile at a time can use it.
- **Some app-wide switches are shared**, such as Claude's "open with Claude" file-manager entries.
- **Updates can restart an app without its profile.** When an app updates itself and relaunches,
  it may come back with the shared settings. VibeLauncher detects this ("running without its
  profile") and restarts it correctly. Restarting profiles a few seconds apart (Settings →
  Restarts) keeps them from updating at the same moment.
- **The terminal `codex` command** still uses `~/.codex`, i.e. the default ChatGPT account.
- **It relies on how the apps behave today** (per-folder single instance, the environment
  variables above). These aren't public promises; if an update changes them, Diagnostics will say
  so and a VibeLauncher update will follow.
- **AppImage:** shortcuts point at the AppImage file; if you move it, open VibeLauncher once from
  the new place and use Diagnostics → Repair. The `.deb`/`.rpm` don't have this issue.

## Privacy

VibeLauncher makes no network requests of its own (the Website button just opens your browser).
The routing log records where links went and why; the codes inside sign-in links are never
written down.

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

Tests: `cargo test --workspace`.

## License

[MIT](LICENSE)

---

## 中文说明

VibeLauncher 让官方 Claude 和 ChatGPT 桌面版拥有多个**互相独立的配置**：工作和个人分开，或每个客户一个。每个配置有自己的登录、对话和设置。

- 使用你已经安装的官方应用，不修改、不复制、不重新打包、不分发；不读取、不复制任何登录数据，每个配置各自登录。官方更新照常进行。
- 每个配置可以有自己的快捷方式（应用菜单 / 启动台 / 开始菜单），直接打开，不需要 VibeLauncher 窗口在运行。
- 登录时浏览器会把 `claude://` / `codex://` 链接发回应用；有多个配置时，VibeLauncher 会把它交给正在登录的那个配置（5 分钟内有效），分不清时会询问，链接没回来时可以手动粘贴。
- “诊断”能发现并一键修复容易让配置混在一起的问题：更新后应用自己重启而丢了配置、多个配置同时启动（会同时更新）、链接处理程序被抢走等。
- 很轻：启动器本身是一个 1.7 MB 的命令，管理窗口只在你打开时运行，不常驻后台。

**已知限制**：Claude 的 Cowork 虚拟机属于系统账户而不是配置，同一时间只有一个配置能用；部分全局开关（如文件管理器里的“用 Claude 打开”）是共享的；终端里的 `codex` 命令仍使用默认账号（`~/.codex`）；它依赖官方应用目前的行为，若更新改变了这些行为，“诊断”会提示。

macOS 和 Windows 上，CI 每次改动都会用替身应用为 Claude 和 ChatGPT 跑通命令行的完整流程（创建、启动、识别、退出、检测丢失设置、由系统转交 `claude://` / `codex://` 登录链接），但还没有用真实账号登录过；管理窗口在这两个平台上能构建打包，只在 Linux 上手动用过。欢迎反馈。这两个平台的安装包暂未签名：macOS 上右键应用选“打开”（提示“已损坏”时运行 `xattr -dr com.apple.quarantine /Applications/VibeLauncher.app`），Windows 上在 SmartScreen 里选“更多信息 → 仍要运行”。

本项目与 Anthropic、OpenAI 无关。
