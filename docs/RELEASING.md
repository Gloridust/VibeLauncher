# 发版与签名

推一个 `v*` 标签（如 `v0.1.0`），Release 工作流会构建四个平台的安装包，挂到一个草稿 Release 上；检查后手动发布。

## 签名（可选，配了才生效）

没配签名时照常出**未签名**的包（macOS 只有临时签名，用户要右键“打开”；Windows 会弹 SmartScreen）。在仓库 **Settings → Secrets and variables → Actions** 里加下面的 secret，之后的构建会自动签名，并在构建后检查签名是否有效（检查不过构建就失败）。

### macOS（签名 + 公证）

需要 Apple Developer Program 账号（个人或公司，年费）。

| Secret | 内容 |
|---|---|
| `APPLE_CERTIFICATE` | “Developer ID Application” 证书导出的 `.p12`，再 `base64 -i cert.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 `.p12` 时设的密码 |
| `APPLE_SIGNING_IDENTITY` | 证书全名，如 `Developer ID Application: Your Name (TEAMID)`（`security find-identity -v -p codesigning` 可看） |
| `APPLE_ID` | 开发者账号的 Apple ID（邮箱）；不设就只签名不公证 |
| `APPLE_PASSWORD` | 这个 Apple ID 的 **App 专用密码**（appleid.apple.com → 登录与安全 → App 专用密码），不是登录密码 |
| `APPLE_TEAM_ID` | 10 位 Team ID（developer.apple.com → Membership） |

Tauri 会把证书导入临时钥匙串，给窗口程序和随附的 `vibelauncher` 命令签名，再提交公证并把票据钉进 `.app`。登录链接用的“VibeLauncher Router”是在用户电脑上生成的，只做临时签名，这是正常的：它只在本机注册，不分发。

### Windows

| Secret | 内容 |
|---|---|
| `WINDOWS_CERTIFICATE` | 代码签名证书的 `.pfx`，再 `base64 -w0 cert.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | `.pfx` 的密码 |

工作流把证书导入当前用户的证书库，把指纹交给 Tauri，用 SHA-256 和 DigiCert 时间戳签安装包与程序。

注意：2023 年以后新发的 OV/EV 证书多半放在硬件密钥或云端（如 Azure Trusted Signing），拿不到 `.pfx`。那种情况要改用 Tauri 的 `bundle.windows.signCommand`，调用对应厂商的签名工具；工作流里 `Signing` 那一步就是放这个的地方。

## 发版前

1. 改 `Cargo.toml`（workspace）、`app/package.json`、`app/src-tauri/tauri.conf.json` 里的版本号。
2. CI、Window 两个工作流全绿（Window 会在 macOS、Windows 上打开窗口并把操作点一遍）。
3. 如果这一版动了登录链接的转交，在 macOS、Windows 上用真实账号按 [TESTING.md](TESTING.md) 走一遍。
4. `git tag v0.1.0 && git push origin v0.1.0`，等草稿 Release 出来，下载各平台的包装一次，再发布。
