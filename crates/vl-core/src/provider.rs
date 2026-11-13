//! What each supported desktop app needs to run an isolated profile.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Profile;
use crate::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Claude,
    #[serde(rename = "chatgpt")]
    ChatGpt,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::Claude, ProviderKind::ChatGpt];

    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::ChatGpt => "chatgpt",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "chatgpt" | "codex" | "openai" => Some(Self::ChatGpt),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::ChatGpt => "ChatGPT",
        }
    }

    pub fn default_profile_id(self) -> &'static str {
        match self {
            Self::Claude => "claude-default",
            Self::ChatGpt => "chatgpt-default",
        }
    }

    pub fn brand_color(self) -> &'static str {
        match self {
            Self::Claude => "#D97757",
            Self::ChatGpt => "#3D9A7A",
        }
    }

    /// URL schemes the app registers for itself.
    pub fn schemes(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude"],
            Self::ChatGpt => &["codex"],
        }
    }

    pub fn from_scheme(scheme: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.schemes().iter().any(|s| s.eq_ignore_ascii_case(scheme)))
    }

    /// Desktop entry ids the official Linux packages ship, most specific first.
    pub fn linux_desktop_ids(self) -> &'static [&'static str] {
        match self {
            // Official deb, and the community packages (aaddrick, patrickjaja).
            Self::Claude => &["com.anthropic.Claude.desktop", "claude-desktop.desktop", "claude-desktop-bin.desktop"],
            Self::ChatGpt => &["chatgpt.desktop", "com.openai.chat.desktop", "codex.desktop"],
        }
    }

    pub fn linux_known_binaries(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[
                "/usr/lib/claude-desktop/claude-desktop",
                "/usr/lib/claude-desktop-bin/claude-desktop",
                "/opt/Claude/claude-desktop",
                "/opt/claude-desktop/claude-desktop",
                "/usr/bin/claude-desktop",
            ],
            Self::ChatGpt => &["/usr/lib/chatgpt/ChatGPT", "/opt/ChatGPT/ChatGPT", "/usr/bin/chatgpt"],
        }
    }

    pub fn linux_commands(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude-desktop"],
            Self::ChatGpt => &["chatgpt"],
        }
    }

    pub fn macos_bundle_ids(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["com.anthropic.claudefordesktop"],
            Self::ChatGpt => &["com.openai.codex", "com.openai.chat"],
        }
    }

    pub fn macos_app_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["Claude.app"],
            Self::ChatGpt => &["ChatGPT.app", "Codex.app"],
        }
    }

    pub fn windows_exe_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude.exe"],
            Self::ChatGpt => &["chatgpt.exe", "codex.exe"],
        }
    }

    /// Substrings of the MSIX package name (Get-AppxPackage -Name).
    pub fn windows_package_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["Claude"],
            Self::ChatGpt => &["ChatGPT", "Codex"],
        }
    }

    /// Process-name stems used to recognise a running main process when the
    /// exact executable path is unknown.
    pub fn process_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude-desktop", "claude", "claude.exe"],
            Self::ChatGpt => &["chatgpt", "chatgpt.exe", "codex", "codex.exe"],
        }
    }

    /// Variables this app reads per profile; `doctor` checks them on running
    /// processes.
    pub fn isolation_env(self, root: &Path) -> Vec<(String, String)> {
        let electron = electron_dir(root);
        match self {
            Self::Claude => vec![("CLAUDE_CONFIG_DIR".into(), path_str(&root.join("claude-config")))],
            Self::ChatGpt => vec![
                ("CODEX_ELECTRON_USER_DATA_PATH".into(), path_str(&electron)),
                ("CODEX_HOME".into(), path_str(&root.join("codex-home"))),
            ],
        }
    }

    /// Directories to create before the first launch.
    pub fn isolation_dirs(self, root: &Path) -> Vec<PathBuf> {
        match self {
            Self::Claude => vec![electron_dir(root), root.join("claude-config")],
            Self::ChatGpt => vec![electron_dir(root), root.join("codex-home")],
        }
    }

    /// Environment variables that must not leak from VibeLauncher's own
    /// environment into the app (they change how Electron or the app starts).
    pub fn scrub_env(self, isolated: bool) -> Vec<&'static str> {
        let mut v = vec![
            "ELECTRON_RUN_AS_NODE",
            "ELECTRON_NO_ATTACH_CONSOLE",
            "ELECTRON_ENABLE_LOGGING",
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_SSE_PORT",
        ];
        if isolated {
            match self {
                Self::Claude => v.extend(["CLAUDE_USER_DATA_DIR", "CLAUDE_CONFIG_DIR"]),
                Self::ChatGpt => v.extend(["CODEX_ELECTRON_USER_DATA_PATH", "CODEX_HOME"]),
            }
        }
        v
    }

    pub fn is_login_url(self, url: &url::Url) -> bool {
        let hay = format!("{}{}", url.host_str().unwrap_or(""), url.path()).to_ascii_lowercase();
        ["login", "auth", "callback", "oauth", "magic", "sso", "verify"].iter().any(|k| hay.contains(k))
    }
}

pub fn electron_dir(root: &Path) -> PathBuf {
    root.join("electron")
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Everything needed to start one profile.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub set_env: Vec<(String, String)>,
    pub remove_env: Vec<String>,
    /// macOS: the .app bundle, so the platform layer can use `open -n`.
    pub bundle: Option<PathBuf>,
    /// Directories to create before launching.
    pub mkdirs: Vec<PathBuf>,
}

/// The `--user-data-dir` a profile runs with, if isolated.
pub fn profile_user_data_dir(profile: &Profile, paths: &Paths) -> Option<PathBuf> {
    profile.isolated.then(|| electron_dir(&profile.data_root(paths)))
}

pub fn build_launch_spec(
    profile: &Profile,
    paths: &Paths,
    program: &Path,
    base_args: &[String],
    bundle: Option<&Path>,
    url: Option<&str>,
) -> LaunchSpec {
    let provider = profile.provider;
    // e.g. `flatpak run com.example.App`: these come before the app's own flags.
    let mut args: Vec<OsString> = base_args.iter().map(OsString::from).collect();
    let mut set_env = Vec::new();
    let mut mkdirs = Vec::new();
    if profile.isolated {
        let root = profile.data_root(paths);
        let electron = electron_dir(&root);
        args.push(format!("--user-data-dir={}", path_str(&electron)).into());
        set_env.extend(provider.isolation_env(&root));
        mkdirs.extend(provider.isolation_dirs(&root));
        // No `--class`: Chromium derives the Wayland app_id / WM_CLASS from the
        // app's desktop name, so every profile's windows group under the app.
    }
    args.extend(profile.extra_args.iter().map(OsString::from));
    for (k, v) in &profile.env {
        set_env.push((k.clone(), v.clone()));
    }
    if let Some(u) = url {
        args.push(u.into());
    }
    LaunchSpec {
        program: program.to_path_buf(),
        args,
        set_env,
        remove_env: provider.scrub_env(profile.isolated).into_iter().map(String::from).collect(),
        bundle: bundle.map(Path::to_path_buf),
        mkdirs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Profile;

    #[test]
    fn spec_for_isolated_claude() {
        let paths = Paths::under(Path::new("/tmp/vl"));
        let mut p = Profile::default_for(ProviderKind::Claude);
        p.id = "work".into();
        p.isolated = true;
        p.env.insert("FOO".into(), "bar".into());
        let s = build_launch_spec(&p, &paths, Path::new("/usr/bin/claude-desktop"), &[], None, Some("claude://x"));
        assert_eq!(s.args[0], OsString::from("--user-data-dir=/tmp/vl/profiles/work/electron"));
        assert_eq!(s.args.last().unwrap(), &OsString::from("claude://x"));
        assert!(s.set_env.contains(&("CLAUDE_CONFIG_DIR".into(), "/tmp/vl/profiles/work/claude-config".into())));
        assert!(s.set_env.contains(&("FOO".into(), "bar".into())));
        assert!(s.remove_env.contains(&"CLAUDE_USER_DATA_DIR".to_string()));
    }

    #[test]
    fn default_profile_runs_plain() {
        let paths = Paths::under(Path::new("/tmp/vl"));
        let p = Profile::default_for(ProviderKind::ChatGpt);
        let s = build_launch_spec(&p, &paths, Path::new("/usr/bin/chatgpt"), &[], None, None);
        assert!(s.args.is_empty());
        assert!(s.set_env.is_empty());
        assert!(!s.remove_env.contains(&"CODEX_HOME".to_string()));
    }

    #[test]
    fn login_urls() {
        let u = url::Url::parse("claude://claude.ai/login/app-google-auth?code=x").unwrap();
        assert!(ProviderKind::Claude.is_login_url(&u));
        let u = url::Url::parse("claude://claude.ai/new?surface=chat").unwrap();
        assert!(!ProviderKind::Claude.is_login_url(&u));
    }
}
