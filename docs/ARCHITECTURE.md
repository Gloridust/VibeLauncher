# VibeLauncher 架构与技术路线

> 状态：设计稿 v0.1（2026-11-14）。还没有写代码。
> 本文每条机制都标了可信度：
> **【实测】** 在本机实际跑过；**【代码】** 从应用打包代码里读到，没跑过；**【未验证】** 推测，要在对应平台上测。
> 实测环境：Ubuntu 26.04 / GNOME Wayland，Claude Desktop **5.11492.2**（Anthropic 官方 deb），ChatGPT **26.1111.21308**（OpenAI 官方 deb）。
> macOS 和 Windows 上的内容**全部是【代码】或【未验证】**，第一阶段要先补实测（见第 13 节）。

---

## 0. 一页结论

VibeLauncher 是 Claude Desktop 和 ChatGPT 桌面版的多 Profile 启动器。它不改应用文件，不碰登录凭证，只做四件事：

1. 给每个 Profile 一个独立数据目录，带上正确的参数和环境变量启动官方应用。
2. 生成每个 Profile 的桌面快捷方式。
3. 接管 `claude://`（以及 `codex://`）回调，把它送到正确的 Profile。
4. 用 `doctor` 发现「更新重启后丢了隔离」之类的问题，并给出一键修复。

### 对原方案的修正

| 原方案 | 修正 | 依据 |
|---|---|---|
| Profile 字段写死 `claudeConfigDir` | 改成 `provider` + 通用 `env` 映射。Claude 和 ChatGPT 需要的变量不同 | ChatGPT 用 `CODEX_HOME`、`CODEX_ELECTRON_USER_DATA_PATH`【代码+实测】 |
| 只支持 Claude | 同一套核心，按 provider 做适配器 | 用户需求 |
| 隔离靠 `--user-data-dir` | **保留**，这是唯一生效的开关。不要指望 `CLAUDE_USER_DATA_DIR` 环境变量 | 只设环境变量时数据仍写进默认目录【实测】。应用会在读到参数后自己设置这个变量传给子进程【代码】 |
| 启动器在系统里注册成 `claude://` 处理程序 | **在 Linux 上行不通**：Claude 每次启动都会检查自己是不是默认处理程序，不是就抢回来 | 把默认处理程序设为别的 `.desktop` 后启动 Claude，马上被改回 `com.anthropic.Claude.desktop`【实测】；设 `CHROME_DESKTOP` 也拦不住【实测】 |
| （新增）Linux 用「同名覆盖」接管回调 | 在 `~/.local/share/applications/` 放一个同名的 `com.anthropic.Claude.desktop`，`Exec` 指向启动器。Claude 认为自己仍是处理程序，不会再改；回调实际交给启动器 | 回调 URL 确实进了路由脚本，Claude 没有改写【实测】 |
| 回调交给指定 Profile 的方式未说明 | 带上该 Profile 的 `--user-data-dir` 再执行一次应用并附上 URL。官方的单实例机制会把 URL 交给已运行的那个窗口，第二个进程约 2 秒内退出 | `second-instance` 日志，第二个进程退出码 0【实测】 |
| CLI 名 `fenshen` | 统一叫 **`vibelauncher`**，与仓库和应用名一致（可以另设别名） | 待你确认，见第 14 节 |
| macOS 是否能用 `open --env` 传环境变量「有争议」 | 倾向可以：ChatGPT 自己的代码就用 `open -n --env CODEX_HOME=… --args --user-data-dir=…` 启动另一份实例。但这只能证明 ChatGPT 可以，Claude 仍要实测 | 【代码】 |
| （新增）有一个文件不随 Profile 隔离 | Claude 带 `--user-data-dir` 时，仍会把 `os-entry-points.json`（文件管理器右键菜单开关）写到默认的 `~/.config/Claude/` | 【实测】，影响很小，写进已知限制 |

---

## 1. 定位、原则与边界

**定位**：官方桌面应用的多 Profile 启动器，跨 Linux / macOS / Windows，支持 Claude Desktop 和 ChatGPT 桌面版。

**硬性原则**

- **不改应用**：不解包、不打补丁、不替换官方二进制和资源，不分发它们。官方更新照常进行，启动器每次运行时重新定位应用。
- **不碰凭证**：不读、不复制、不同步任何 Cookie、Token、`auth.json`。Profile 之间只靠目录隔离。删除 Profile 时把目录移到回收站，不直接删除。
- **默认 Profile 不受影响**：内置的「默认」Profile 不加任何参数，就是原来那份应用，原有登录和数据不动。
- **可完全撤销**：卸载或在设置里关掉回调接管时，删掉启动器生成的所有快捷方式和同名覆盖文件，协议处理程序恢复原样。
- **不冒充官方**：用自己的名字和图标，界面里写明「非官方工具」，不用 Anthropic / OpenAI 的 Logo。Profile 快捷方式可以用官方应用图标叠一个 Profile 色标，但启动器本身不用。
- **鲁棒**：不写死安装路径。按「系统注册信息 → 已知路径 → 用户手动指定」的顺序定位应用，每次启动前都重新检查。

**明确不做**：账号切换（不在同一个数据目录里换账号）、凭证同步、修改应用行为、绕过任何限制。

---

## 2. 实测记录（Linux）

实验都在临时目录里用 `systemd-run --user` 单独起进程（限内存 3G），用私有的 `XDG_CONFIG_HOME` / `XDG_DATA_HOME` 隔开真实配置。结束后已确认真实的 `claude://`、`codex://` 处理程序和 `~/.codex/auth.json` 都没被改。

| 编号 | 做法 | 结果 |
|---|---|---|
| 1A | 只加 `--user-data-dir=X` | 所有数据进 X（Cookie、`claude_desktop_config.json`、`config.json`、`logs/`、`sentry/`、单实例锁）。默认目录只多了 `os-entry-points.json` |
| 1B | 只设 `CLAUDE_USER_DATA_DIR=X` | **不生效**，全部写进默认目录 |
| 1C | 两个都设 | 同 1A |
| 2 | 1A 运行中，再执行一次 `claude-desktop --user-data-dir=X 'claude://claude.ai/new?…'` | 第二个进程 2.2 秒退出（码 0），X 那个实例的日志出现 `second-instance`。默认 Profile 没收到 |
| 3A | 先把 `claude://` 默认处理程序设成 `vl-router.desktop`，再启动 Claude | 启动后被改回 `com.anthropic.Claude.desktop` |
| 3B | 同 3A，并设 `CHROME_DESKTOP=vl-router.desktop` | 同样被改回，这个变量拦不住 |
| 3C | 在用户目录放同名 `com.anthropic.Claude.desktop`，`Exec` 指向路由脚本 | Claude 不再改（它看到的名字就是自己）；`gio open claude://…` 进了路由脚本 |
| 4 | ChatGPT：同一组 `CODEX_HOME` + `CODEX_ELECTRON_USER_DATA_PATH` + `--user-data-dir` 启动两次 | 第二次立即退出（Chromium 自带的目录锁仍然生效），数据都在指定目录，真实 `~/.codex` 没被碰。启动时会把 `codex://` 写回 `chatgpt.desktop` |

**代码里读到的相关事实（Claude 5.11492.2）**

- 启动时调用 `setAsDefaultProtocolClient`，带 `keepExistingDefault: true`。它的实际含义是「如果自己还不是默认处理程序，就设为自己」，所以别的程序注册了也会被抢回（与 3A 一致）。
- 有 `[deepLinkOwnership]` 逻辑：Google 登录（`/login/app-google-auth`）会先查自己是不是 `claude://` 的处理程序，不是就改用别的登录方式（macOS 上是 ASWebAuthenticationSession）。→ **接管回调时必须让 Claude 仍然认为自己是处理程序**，否则登录路径会变。Linux 的同名覆盖正好满足这一点。
- 代码里约 77 处引用 `CLAUDE_CONFIG_DIR`，用于桌面端 Code 页签。
- 单实例锁、`second-instance` 处理都按数据目录区分。

**ChatGPT（26.1111.21308）**

- 读 `CODEX_HOME`（Codex 的配置与 `auth.json`，默认 `~/.codex`，**与 Codex CLI 共用**）和 `CODEX_ELECTRON_USER_DATA_PATH`（Electron 数据目录）。
- 设了 `CODEX_ELECTRON_USER_DATA_PATH` 时会跳过它自己的单实例锁【代码】，但 Chromium 目录锁仍挡住了重复启动【实测 4】。
- Windows 上不注册 `codex://`【代码】。它的 `.desktop` 还声明了 `http/https` 和一堆文档类型。
- **登录怎么回到应用还没查清**（可能是 `codex://`，也可能是本地回环端口）。设计 ChatGPT 回调路由前要先查清楚。

---

## 3. 总体架构

```
┌──────────────────────────────── VibeLauncher ────────────────────────────────┐
│                                                                              │
│  GUI（Tauri 2 + Web 前端）       CLI：vibelauncher        URL 路由入口         │
│  Profile 列表 / 编辑 / 诊断       list / launch / login    vibelauncher open-url │
│          │                       doctor / route / …       （系统把回调交给它）  │
│          └──────────────┬────────────────┴──────────────────────┘            │
│                         ▼                                                    │
│                 vl-core（Rust 库，所有逻辑都在这里）                           │
│   ┌──────────┬───────────┬────────────┬───────────┬────────────┬──────────┐  │
│   │ 配置存储  │ 应用定位   │ 启动器      │ 快捷方式   │ 回调路由     │ 诊断      │  │
│   │ profiles │ locator   │ launcher   │ shortcuts │ router     │ doctor   │  │
│   └──────────┴───────────┴────────────┴───────────┴────────────┴──────────┘  │
│                         │                                                    │
│        provider 适配器：claude / chatgpt（每个应用需要什么参数和变量）           │
│        platform 适配器：linux / macos / windows（怎么找、怎么启动、怎么注册）     │
└──────────────────────────────────────────────────────────────────────────────┘
          │ 只做两件事：带参数启动官方应用；把回调 URL 转交给正确的实例
          ▼
   官方 Claude Desktop / ChatGPT（原样，不修改）
```

**关键设计**

- **一个核心，三个入口**。GUI、CLI、URL 路由入口都调用同一个 `vl-core`，行为一致，测试只测核心。
- **两层适配**。`provider × platform` 正交：加一个应用只写 provider；加一个平台只写 platform。
- **路由入口要快**。系统打开 `claude://` 时会启动一个短命进程，它不能拉起 GUI，要在 100ms 级别内完成转交后退出。所以它是同一个二进制的无界面子命令，不依赖 GUI 是否在运行。
- **GUI 不常驻**。没有后台服务。需要持续监视的功能（见第 9 节）做成可选的轻量托盘模式。

---

## 4. 数据模型

配置文件：`profiles.toml`，存在各平台标准位置：

| 平台 | 配置 | 默认 Profile 数据根目录 | 运行时状态（待登录标记、锁） |
|---|---|---|---|
| Linux | `$XDG_CONFIG_HOME/vibelauncher/` | `$XDG_DATA_HOME/vibelauncher/profiles/<id>/` | `$XDG_STATE_HOME/vibelauncher/` |
| macOS | `~/Library/Application Support/VibeLauncher/` | 同左 `profiles/<id>/` | 同左 `state/` |
| Windows | `%APPDATA%\VibeLauncher\` | `%LOCALAPPDATA%\VibeLauncher\profiles\<id>\` | `%LOCALAPPDATA%\VibeLauncher\state\` |

用 Rust 的 `directories` crate 取路径，不自己拼。

```toml
schema = 1

[[profile]]
id = "work"                 # 稳定 ID，只用小写字母、数字、横线；用于目录名、快捷方式名、WM_CLASS
name = "工作"                # 显示名，可以改
provider = "claude"         # claude | chatgpt
color = "#D97757"           # 色标；也可以给 emoji / SF Symbol 名
icon = "briefcase"
data_root = "…/profiles/work"   # 可选；默认按上表生成，允许放到别的盘
created_at = 2026-11-14T08:00:00Z

[profile.env]               # 用户自定义的额外环境变量（provider 需要的变量自动加，不用写在这里）
HTTPS_PROXY = "http://127.0.0.1:7890"

[profile.args]              # 额外命令行参数（谨慎使用）
extra = []
```

每个 Profile 的目录结构由 provider 决定，例如：

```
profiles/work/
  electron/        → Claude: --user-data-dir
  claude-config/   → Claude: CLAUDE_CONFIG_DIR（Code 页签）
profiles/personal-gpt/
  electron/        → ChatGPT: --user-data-dir + CODEX_ELECTRON_USER_DATA_PATH
  codex-home/      → ChatGPT: CODEX_HOME（与 Codex CLI 的 ~/.codex 分开）
```

`id = "default"` 是保留的默认 Profile：不加任何参数、不设任何变量，等于直接打开官方应用。

**约束**：同一个数据目录不能被两个 Profile 指向；目录创建时设 `0700`；配置写入用「写临时文件再 rename」，避免写一半损坏；带 `schema` 版本号，以后改格式时做迁移。

---

## 5. Provider 适配器

```rust
trait Provider {
    fn id(&self) -> &'static str;                         // "claude" | "chatgpt"
    fn schemes(&self) -> &[&str];                         // ["claude"] | ["codex"]
    fn locate(&self, p: &dyn Platform) -> Result<AppInstall>;
    fn launch_spec(&self, prof: &Profile, app: &AppInstall, url: Option<&Url>) -> LaunchSpec; // 程序 + 参数 + 环境变量
    fn identify(&self, proc: &ProcInfo) -> Option<ProfileMatch>;  // 从进程命令行认出它属于哪个 Profile
    fn doctor_checks(&self) -> Vec<Check>;
    fn known_versions(&self) -> VersionRange;             // 实测过的版本范围，超出时 doctor 提醒
}
```

| | Claude | ChatGPT |
|---|---|---|
| 隔离参数 | `--user-data-dir=<electron>`（必需） | `--user-data-dir=<electron>` |
| 环境变量 | `CLAUDE_CONFIG_DIR=<claude-config>` | `CODEX_ELECTRON_USER_DATA_PATH=<electron>`、`CODEX_HOME=<codex-home>` |
| 协议 | `claude://` | `codex://`（Linux/macOS） |
| 认进程 | 命令行里的 `--user-data-dir` | 同左 |
| 已知共享（不能隔离） | Cowork 虚拟机、`os-entry-points.json` | 待查 |

**为什么环境变量和参数都要传**：命令行在三个平台上都能被其他进程读到，用来认出进程属于哪个 Profile 最可靠；环境变量只在启动时生效，更新重启时可能丢（第 9 节）。

---

## 6. 平台层

```rust
trait Platform {
    fn find_installs(&self, provider: &dyn Provider) -> Vec<AppInstall>;  // 可能有多份
    fn spawn(&self, spec: &LaunchSpec) -> Result<()>;                    // 脱离父进程启动
    fn install_shortcut(&self, prof: &Profile, app: &AppInstall) -> Result<()>;
    fn remove_shortcut(&self, prof: &Profile) -> Result<()>;
    fn claim_scheme(&self, provider: &dyn Provider) -> Result<()>;       // 接管回调（第 7 节）
    fn release_scheme(&self, provider: &dyn Provider) -> Result<()>;     // 原样恢复
    fn scheme_status(&self, provider: &dyn Provider) -> SchemeStatus;
    fn processes(&self) -> Vec<ProcInfo>;                                 // 命令行、环境变量、启动时间
}
```

### 6.1 定位应用（鲁棒性的核心）

不写死路径，按顺序找，找到多份就都列出来让用户选（并记住）：

**Linux**
1. 查系统里声明了 `x-scheme-handler/claude`（或 `codex`）的 `.desktop` 文件（`/usr/share/applications`、`~/.local/share/applications`、Flatpak、Snap 的导出目录），解析 `Exec` 拿到命令。这一步能覆盖官方 deb、aaddrick 和 patrickjaja 的社区包、Flatpak、Nix，不管它们装在哪。
2. 跳过 VibeLauncher 自己生成的文件（有 `X-VibeLauncher=true` 标记）。
3. 已知路径兜底：`/usr/lib/claude-desktop/claude-desktop`、`/opt/Claude/…`、`/usr/lib/chatgpt/ChatGPT` 等；`PATH` 里的 `claude-desktop`、`chatgpt`。
4. AppImage：让用户指定文件，记住路径；每次启动前检查还在不在。
5. 解析符号链接和包装脚本，得到真正的二进制，用来比对进程。Flatpak 的启动方式是 `flatpak run <id> --user-data-dir=…`，传参方式要单独测。

**macOS**【未验证】
1. 按 Bundle ID 找：Launch Services（`LSCopyApplicationURLsForBundleIdentifier`），Bundle ID 先实测确认（Claude 可能是 `com.anthropic.claudefordesktop`）。
2. 兜底：`/Applications`、`~/Applications`、Spotlight `mdfind kMDItemCFBundleIdentifier == …`。
3. 读 `Info.plist` 拿版本号。

**Windows**【未验证】
1. MSIX：运行时调 `Get-AppxPackage`（或 WinRT `PackageManager`）拿 `InstallLocation`，每次启动都查，官方更新换了目录也不会失效。
2. 旧版 Squirrel 安装：`%LOCALAPPDATA%\AnthropicClaude\` 下取最新的 `app-*`。
3. 注册表 `HKCU\Software\Classes\claude\shell\open\command` 反查。
4. ⚠️ 直接执行 `WindowsApps` 里的 exe 时，应用拿不拿得到包身份、环境变量能不能传进去，**必须实测**。不行的话改用 `Invoke-CommandInPackage` 或应用执行别名。

### 6.2 启动

- Linux：`fork` + `setsid` 脱离启动器，`stdin/stdout` 接到 `/dev/null`，带上 `env` 和参数直接执行二进制。不经过 shell，参数不用转义。
- macOS【未验证】：首选 `open -n -a <App路径> --env K=V … --args --user-data-dir=…`（`-n` 才会开新实例）。若 Claude 收不到环境变量，改为直接执行 `Claude.app/Contents/MacOS/Claude`（代价是 Dock 行为可能不同），两种都实测后再定。
- Windows【未验证】：`CreateProcess` 带环境块；`DETACHED_PROCESS`。

启动前统一检查：应用还在吗、版本在实测范围内吗、这个 Profile 是不是已经在运行（在运行就只是转交 URL / 把窗口调到前面）。

### 6.3 快捷方式与任务栏分组

| 平台 | 产物 | 要点 |
|---|---|---|
| Linux | `~/.local/share/applications/vibelauncher-<id>.desktop` | `Exec=vibelauncher launch <id> %U`；独立的 `StartupWMClass`；图标为「官方图标 + 色标」合成后放 `~/.local/share/icons/hicolor/`；刷新 `update-desktop-database` 和图标缓存（用户目录要 `gtk-update-icon-cache -t`） |
| macOS | `~/Applications/VibeLauncher/<名字>.app` | 生成一个很小的 `.app`（只含 `Info.plist` 和一个调用 `vibelauncher launch <id>` 的脚本或小二进制），图标同样合成；不需要 `osacompile` |
| Windows | 开始菜单 `.lnk` | 目标是 `vibelauncher.exe launch <id>`；设 `System.AppUserModel.ID` 让任务栏不和其他 Profile 合并 |

**Linux 任务栏分组**【未验证】：Claude 窗口的 app_id 来自它打包时的 `desktopName`（`com.anthropic.Claude`），所以默认所有 Profile 的窗口都会合成一个图标。要分开需要改 app_id，候选：Chromium 的 `--class=`（X11）、`--wayland-app-id=`（Wayland，版本相关）。实测 3B 已证明 `CHROME_DESKTOP` 不能改协议注册，它对 app_id 有没有影响也要测。改不了的话就接受合并，用窗口标题区分（Claude 窗口标题是固定的，所以只能靠 Dock 图标菜单）。

---

## 7. 回调路由（最重要、最容易出错的部分）

### 7.1 问题

登录（尤其 Google 登录）结束时，浏览器打开 `claude://…` 回调。系统只认一个处理程序，回调会被交给「最先注册的那个 Claude」或默认 Profile，于是 Profile A 发起的登录跑到了 Profile B，或者登录失败。三个平台都有人报告。

### 7.2 接管方式（按平台）

**Linux：同名覆盖【实测可行】**
- 在 `~/.local/share/applications/` 写一份 `com.anthropic.Claude.desktop`：内容从系统那份复制（名字、图标、`Actions`、`MimeType` 原样），只把所有 `Exec=` 改成 `vibelauncher open-url --provider claude …`，并加 `X-VibeLauncher=true`。用户目录的同名文件按 XDG 规范优先于系统那份。
- 结果：Claude 查「我是不是默认处理程序」时答案是「是」，所以既不抢回，也不改变登录方式；而回调实际交给了启动器。
- 应用菜单里的「Claude」图标也会走启动器，打开的是默认 Profile，与原来一样。
- 官方包更新可能改 `.desktop`（加新的 Action），所以启动器每次运行都比对系统那份，内容变了就重新生成。
- 撤销：删掉这个文件即可，不需要改 `mimeapps.list`。
- ChatGPT 同理覆盖 `chatgpt.desktop`。但它还声明了 `http/https`，覆盖后这些链接也会先到启动器，启动器要把非 `codex://` 的 URL 原样转给默认 Profile。是否需要接管 ChatGPT，取决于它登录是否走 `codex://`（第 2 节，待查）。

**macOS【未验证】**
- 这里没有「同名覆盖」，处理程序按 Bundle ID 记在 Launch Services 里。Claude 启动时会把自己设回默认，而它的 Google 登录又依赖「自己是处理程序」。
- 候选方案，按顺序实测：
  1. **不接管，靠时机**：发起登录时只打开目标 Profile，确保登录期间它是唯一运行中的 Claude（或最后启动的那个），回调自然会到它。用待登录标记 + 界面提示「请先关闭其他 Claude 窗口」配合。最稳，但体验一般。
  2. **启动器注册为处理程序，并在每次启动 Profile 后几秒重新注册**（`LSSetDefaultHandlerForURLScheme`）。风险：Claude 发现自己不是处理程序后会换登录方式，可能反而更顺。要实测看结果。
  3. 兜底：粘贴回调链接手动转交。

**Windows【未验证】**
- Claude 启动时会写 `HKCU\Software\Classes\claude`。MSIX 版的协议声明在包清单里，两者优先级要实测。
- 候选：启动器在每次启动 Profile 后重新写注册表项；或者和 macOS 方案 1 一样靠时机。

### 7.3 路由算法

`vibelauncher open-url <url>` 收到 URL 后：

1. **识别 provider**：按协议（`claude` / `codex`）。
2. **待登录标记**：`vibelauncher login <id>`（或界面里点「登录」）会写 `state/pending-login.json`：`{provider, profile, created_at, nonce}`，有效期 **5 分钟**。收到回调时若有未过期的标记，交给那个 Profile，并删掉标记（只用一次）。
3. **只有一个 Profile 在运行**：交给它。
4. **多个在运行或都没运行，且没有标记**：
   - 普通链接（如 `claude://claude.ai/new`、文件关联）：交给最近一次被启动或聚焦的 Profile（启动器记录），没有记录就给默认 Profile。
   - 登录类链接（路径含 `login`、`auth`、`callback`）：弹一个小选择窗（「这个登录要交给哪个 Profile？」，默认选最近发起过登录的那个），不猜。
5. **转交**：用目标 Profile 的完整启动配置执行一次应用并附上 URL（实测 2）。目标没在运行时就是正常启动并带上 URL。
6. 所有路由决定写进日志 `state/router.log`（只记 provider、Profile、URL 的协议和路径，**不记查询参数**，因为里面可能有一次性凭证）。

**兜底**：`vibelauncher route <url> --to <id>`，界面里也有「粘贴回调链接」，用于自动路由失败的情况。

**安全**：路由入口只接受 `claude://`、`codex://` 和该应用 `.desktop` 里声明的类型；URL 只当参数转交，不解析执行；不把 URL 发给任何网络服务。

---

## 8. doctor（诊断）

`vibelauncher doctor` 和界面里的「诊断」页跑同一组检查，每项给出 ✅ / ⚠️ / ❌ 和一键修复：

| 检查 | 怎么查 | 修复 |
|---|---|---|
| 应用能找到、版本在实测范围内 | 定位 + 读版本 | 重新定位 / 提示「此版本未经测试」 |
| 每个运行中的进程属于哪个 Profile | 读命令行里的 `--user-data-dir` | — |
| 有没有「带了数据目录却丢了 `CLAUDE_CONFIG_DIR`」的进程（更新重启后常见） | 读进程环境变量：Linux `/proc/<pid>/environ`；macOS `sysctl KERN_PROCARGS2`；Windows 读 PEB。用 `sysinfo` crate 统一 | 「重启这个 Profile」 |
| 有没有不认识的 Claude 进程（不是默认、也不是任何 Profile） | 比对数据目录 | 提示 |
| 同一个数据目录有没有两个主进程 | 计数 | 提示关闭其中一个 |
| 回调接管状态 | Linux：覆盖文件存在且与系统那份同步；macOS/Windows：当前处理程序是谁 | 重新生成 / 重新注册 |
| 多个 Profile 的启动时间过于接近（更新会一起重启） | 比较进程启动时间 | 提示错开启动 |
| 数据目录权限、磁盘空间 | `stat` | 修权限 |
| 已知限制提醒（Cowork 共享虚拟机等） | 按平台 | 只提示 |

注意：Claude 启动后会自己在进程内设置 `CLAUDE_USER_DATA_DIR`，但这不会出现在 `/proc/<pid>/environ` 里（那里只有启动时的环境），所以**认进程只看命令行**。

---

## 9. 更新重启相关问题

- **更新重启后丢失 `CLAUDE_CONFIG_DIR`**：Electron 的 `app.relaunch()` 默认沿用原来的命令行参数，所以 `--user-data-dir` 通常还在，数据不会串；但环境变量是否保留取决于更新器（macOS/Windows 的更新器是独立进程）。后果是该 Profile 的 Code 页签用回默认的 `~/.claude`。对策：
  - doctor 检测（第 8 节）；
  - 可选的**托盘模式**：每 30 秒扫一次进程，发现某个 Profile 环境变量不对就弹通知「工作 Profile 更新后丢了 Code 隔离，点此重启」。不自动杀进程（可能有正在跑的会话）。
- **多个 Profile 同时被更新重启**：更新计时器从启动时开始算，同时启动的 Profile 会同时重启。启动器在「全部启动」时自动错开（间隔可配，默认 20 秒）；doctor 提示时间过近的情况。启动器不去改应用的更新逻辑。

---

## 10. 已知限制（README 要写清楚）

- **Cowork 虚拟机不隔离**：Windows 上虚拟机镜像路径不跟随 `--user-data-dir`；Linux 上虚拟机守护进程是共享的。启动器解决不了。
- **`os-entry-points.json` 共享**：文件管理器右键菜单的开关总在默认目录，各 Profile 共用。
- **依赖未公开的行为**：`--user-data-dir` 能完全隔离、单实例按目录区分、启动时抢协议处理程序，这些都是官方应用当前版本的行为，不是公开承诺。每个新版本都要过一遍自动化检查（第 12.4 节），doctor 对超出实测范围的版本给出提示。
- **任务栏分组**：Linux 上能否按 Profile 分开要看 app_id 能否改（第 6.3 节）。
- **Codex CLI**：ChatGPT Profile 用独立的 `CODEX_HOME`，所以终端里的 `codex` 仍用 `~/.codex`，和默认 Profile 是同一个账号。

---

## 11. 界面与体验

### 11.1 设计方向

「Claude 的暖色调 + iOS 的结构与动效」：

- **配色**：暖米白底（浅色 `#FAF9F5` 一类）、深色模式暖炭灰；强调色用珊瑚橙（接近 Claude 品牌色，但不直接用官方 Logo）；ChatGPT Profile 用中性灰绿作色标，与 Claude 区分。全部做成 CSS 变量，跟随系统浅色/深色。
- **字体**：标题用衬线（如 Source Serif / Noto Serif SC），正文用系统字体（`-apple-system`、`Segoe UI Variable`、`Inter`/`Noto Sans SC`）。字体随应用打包，不从网络加载。
- **结构**：iOS 设置式的分组圆角列表（inset grouped）、大标题随滚动收起、底部弹出式表单（sheet）、分段控件、开关、滑动删除（桌面上配右键菜单）。
- **材质**：半透明毛玻璃只用在标题栏和弹层，少量使用。Linux 的 WebKitGTK 上 `backdrop-filter` 性能一般，透明窗口不可靠，所以**窗口本身不透明**，毛玻璃只在窗口内部。
- **动效**：弹簧曲线，列表增删、sheet 弹出、启动时卡片轻微缩放。尊重系统「减少动态效果」设置。
- **无障碍**：键盘全程可操作（↑↓ 选择、回车启动、⌘/Ctrl+N 新建、⌘/Ctrl+, 设置）；对比度达到 WCAG AA；屏幕阅读器标签齐全。

### 11.2 页面

1. **首页**：大标题「Profiles」；Claude、ChatGPT 两组卡片，每张卡片显示色标、名字、运行状态（绿点）、上次使用时间；点一下启动，已运行则切到前台。右上角「+」。顶部一条状态提示（例如「工作 Profile 需要重启」），点了直接修复。
2. **新建 / 编辑（sheet）**：名字、应用（分段控件）、颜色和图标选择、数据目录（默认自动，可改）、高级（环境变量、额外参数，折叠）。保存后可选「创建桌面快捷方式」。
3. **登录引导**：点「登录」后显示三步（打开应用 → 在浏览器完成登录 → 自动回到该 Profile），带 5 分钟倒计时；超时或失败时出现「粘贴回调链接」。
4. **诊断**：第 8 节的检查清单，每项一键修复。
5. **设置**：回调接管开关（关掉即完全撤销）、托盘模式、启动错开间隔、语言（中文/英文）、检查更新、关于（写明非官方）。
6. **首次运行**：自动定位应用 → 说明会做什么、不会做什么 → 询问是否接管回调（默认开，可跳过）→ 建第一个 Profile。

### 11.3 CLI

```
vibelauncher list                        # 列出 Profile 和运行状态
vibelauncher launch <id> [url]           # 启动（或转交 URL）
vibelauncher login <id>                  # 写待登录标记并启动
vibelauncher route <url> --to <id>       # 手动转交回调
vibelauncher open-url <url>              # 系统回调入口（一般不手动用）
vibelauncher create <id> --provider claude --name 工作
vibelauncher doctor [--fix]
vibelauncher shortcuts sync              # 重新生成全部快捷方式和覆盖文件
vibelauncher scheme claim|release|status
```

输出默认人读格式，`--json` 给脚本用。

---

## 12. 技术选型与工程

### 12.1 选型

| 部分 | 选择 | 理由 |
|---|---|---|
| 桌面外壳 | **Tauri 2** | 安装包几 MB（Electron 要上百 MB）；Rust 适合做进程检查、注册表、Launch Services 这类系统调用；三平台都有官方打包。本机已有 Rust 1.99、`libwebkit2gtk-4.1-dev` |
| 核心 | Rust 库 `vl-core` | GUI 和 CLI 共用 |
| 前端 | React + TypeScript + Vite，样式手写 CSS 变量，动效用 `motion` | 不引入大型组件库，才能做出自己的 iOS 风格 |
| 关键 crate | `directories`、`serde`/`toml`、`sysinfo`（进程与环境变量）、`url`、`tracing`、Windows：`windows`（注册表、`.lnk`、PEB）、macOS：`objc2-*`（Launch Services） | |
| 单实例 | `tauri-plugin-single-instance` | GUI 只开一个窗口 |

**不使用** `tauri-plugin-deep-link` 接管协议：它的做法是「注册成默认处理程序」，正是第 7 节说的会被 Claude 抢回并改变登录方式的做法。

### 12.2 仓库结构

```
crates/
  vl-core/         配置、定位、启动、路由、诊断（无界面依赖）
    src/provider/{claude,chatgpt}.rs
    src/platform/{linux,macos,windows}.rs
  vl-cli/          vibelauncher 命令行
app/               Tauri 应用（src-tauri 调用 vl-core）
  src/             前端
docs/              本文档、平台实测记录
spikes/            各平台验证脚本（第 13 节）
```

### 12.3 打包发布

- Linux：`.deb`、`.rpm`、AppImage（AppImage 需要把自身路径写进快捷方式，并在移动后自我修复）；也可提供 AUR。
- macOS：通用二进制 `.dmg`，签名 + 公证（需要 Apple 开发者账号）。
- Windows：MSI 或 NSIS，签名（没有证书时 SmartScreen 会拦）。
- GitHub Actions 三平台矩阵构建；应用内检查 GitHub Release 更新。

### 12.4 测试

- **核心单元测试**：路由算法（标记过期、多实例、只有一个实例等）、配置读写与迁移、`.desktop` 解析与改写、命令行解析。用假的进程表和假的平台实现。
- **集成测试（Linux CI 可跑）**：用私有 `XDG_*` 目录跑完整的「生成覆盖文件 → `gio open` → 路由 → 转交」链路，以假应用代替 Claude。
- **兼容性检查脚本**：对真实安装的 Claude / ChatGPT 自动跑一遍第 2 节的实验（数据落点、转交、协议抢回），输出报告。官方每次发新版本时手动跑，结果记进 `docs/compat.md` 并更新 `known_versions`。
- **UI**：Playwright 跑前端（用假的 Tauri 接口），截图对比浅色/深色/中英文。

---

## 13. 路线图（先做风险最高的）

**阶段 0：各平台验证（1 周）**
- [x] Linux 实测：隔离参数、转交、协议抢回、同名覆盖（本文第 2 节）
- [ ] Linux：app_id / WM_CLASS 能否按 Profile 改；Flatpak 包怎么传参；aaddrick、patrickjaja 社区包的行为
- [ ] ChatGPT：登录怎么回到应用（`codex://` 还是本地端口）；它是否也有「检查自己是否处理程序」的逻辑
- [ ] macOS：`open -n --env` 能否把 `CLAUDE_CONFIG_DIR` 传给 Claude；`--user-data-dir` 隔离是否完整；启动时是否抢回 `claude://`；第 7.2 节两个方案哪个能让 Google 登录回到正确 Profile；更新重启后参数和环境变量是否保留
- [ ] Windows：MSIX 下直接执行 exe 能否传参和环境变量；协议注册的优先级；Cowork 虚拟机路径
- 产出：`spikes/` 脚本 + `docs/compat.md`。若某平台的回调接管走不通，该平台降级为「时机 + 手动粘贴」方案，并在界面里说明。

**阶段 1：核心 + CLI（Linux 优先，1–2 周）**
- `vl-core` 的配置、定位、启动、快捷方式、Linux 路由、doctor
- `vibelauncher` CLI 全部命令
- 单元测试和 Linux 集成测试

**阶段 2：GUI（2 周）**
- 首页、编辑、登录引导、诊断、设置、首次运行
- 浅色/深色、中英文、键盘操作

**阶段 3：macOS、Windows 适配（各 1–2 周，取决于阶段 0 的结果）**

**阶段 4：发布**
- 三平台打包、签名、README（含已知限制）、兼容性表

---

## 14. 需要你决定的事

1. **CLI 名字**：建议统一用 `vibelauncher`（原方案是 `fenshen`）。可以再装一个短别名。
2. **ChatGPT 的优先级**：是和 Claude 一起做，还是 Claude 先做完再加？它的登录回调方式还没查清。
3. **Linux 回调接管默认开还是关**：同名覆盖会让应用菜单里的「Claude」图标先经过启动器（打开的仍是默认 Profile）。我建议首次运行时询问，默认开。
4. **macOS / Windows 签名**：需要 Apple 开发者账号和 Windows 代码签名证书，否则用户会看到系统安全警告。
5. **托盘模式**是否进第一版。
