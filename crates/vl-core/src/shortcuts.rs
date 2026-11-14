//! Per-profile shortcuts in the system's app launcher.
//!
//! - Linux: `~/.local/share/applications/vibelauncher-<id>.desktop`
//! - macOS: `~/Applications/VibeLauncher/<Name>.app` (a tiny AppleScript applet)
//! - Windows: Start menu `VibeLauncher\<Name>.lnk`
//!
//! Every shortcut runs `vibelauncher launch <id>`, so it keeps working when the
//! official app updates or moves.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::install::AppInstall;
use crate::model::{Config, Profile};
use crate::paths::Paths;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutState {
    pub profile_id: String,
    pub path: PathBuf,
    pub exists: bool,
    /// Exists but points at a different VibeLauncher executable.
    pub stale: bool,
}

/// Display name of a profile's shortcut, e.g. "Claude · Work".
pub fn display_name(p: &Profile) -> String {
    if p.isolated {
        format!("{} · {}", p.provider.display_name(), p.name)
    } else {
        format!("{} (VibeLauncher)", p.provider.display_name())
    }
}

/// File-system-safe version of a name.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
fn safe_file_name(s: &str) -> String {
    let v: String = s
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '-' } else { c })
        .collect();
    v.trim().trim_matches('.').to_string()
}

pub fn shortcut_path(p: &Profile) -> PathBuf {
    imp::shortcut_path(p)
}

pub fn state(p: &Profile) -> ShortcutState {
    let path = shortcut_path(p);
    let exists = path.exists();
    ShortcutState { profile_id: p.id.clone(), stale: exists && imp::is_stale(&path), path, exists }
}

pub fn create(paths: &Paths, install: Option<&AppInstall>, p: &Profile) -> Result<PathBuf> {
    let icon = crate::icon::write_profile_icon(paths, install, p)?;
    imp::create(paths, p, &icon)
}

pub fn remove(p: &Profile) -> Result<()> {
    imp::remove_path(&shortcut_path(p))
}

/// Make the shortcuts match the config: create/refresh wanted ones, remove
/// ones for deleted profiles or with `shortcut = false`. Returns problems.
pub fn sync(paths: &Paths, cfg: &Config, installs: &dyn Fn(crate::provider::ProviderKind) -> Option<AppInstall>) -> Vec<String> {
    let mut errors = Vec::new();
    let wanted: Vec<PathBuf> = cfg.profiles.iter().filter(|p| p.shortcut).map(shortcut_path).collect();
    for p in &cfg.profiles {
        if p.shortcut {
            if let Err(e) = create(paths, installs(p.provider).as_ref(), p) {
                errors.push(format!("{}: {e:#}", p.name));
            }
        }
    }
    for path in imp::all_ours() {
        if !wanted.contains(&path) {
            if let Err(e) = imp::remove_path(&path) {
                errors.push(format!("{}: {e:#}", path.display()));
            }
        }
    }
    errors
}

// ───────────────────────────── Linux ─────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::*;
    use crate::desktop_entry as de;
    #[cfg(test)]
    use crate::desktop_entry::DesktopEntry;

    pub fn shortcut_path(p: &Profile) -> PathBuf {
        de::user_applications_dir().join(format!("vibelauncher-{}.desktop", p.id))
    }

    pub fn render(p: &Profile, icon: &Path, exe: &str) -> String {
        let name = de::escape_string(&display_name(p));
        let launch = de::build_exec(&[exe.to_string(), "launch".into(), p.id.clone()]);
        let login = de::build_exec(&[exe.to_string(), "login".into(), p.id.clone()]);
        let manage = de::build_exec(&[exe.to_string()]);
        format!(
            "[Desktop Entry]\nType=Application\nVersion=1.5\nName={name}\nComment={comment}\nExec={launch}\nIcon={icon}\nTerminal=false\nStartupNotify=true\nCategories=Network;Chat;\nNoDisplay=false\nActions=login;manage;\nX-VibeLauncher=true\nX-VibeLauncher-Profile={id}\n\n[Desktop Action login]\nName=Sign in…\nName[zh_CN]=登录…\nExec={login}\n\n[Desktop Action manage]\nName=Manage profiles\nName[zh_CN]=管理配置\nExec={manage}\n",
            comment = de::escape_string(&format!("Open {} with the “{}” profile", p.provider.display_name(), p.name)),
            icon = de::escape_string(&icon.to_string_lossy()),
            id = p.id,
        )
    }

    pub fn create(_paths: &Paths, p: &Profile, icon: &Path) -> Result<PathBuf> {
        let path = shortcut_path(p);
        let text = render(p, icon, &crate::paths::self_exe().to_string_lossy());
        crate::paths::write_atomic(&path, text.as_bytes())?;
        Ok(path)
    }

    pub fn is_stale(path: &Path) -> bool {
        let exe = crate::paths::self_exe();
        de::read(path)
            .and_then(|e| e.exec_args("Desktop Entry"))
            .and_then(|a| a.first().cloned())
            .is_none_or(|first| Path::new(&first) != exe)
    }

    pub fn all_ours() -> Vec<PathBuf> {
        let dir = de::user_applications_dir();
        let Ok(rd) = std::fs::read_dir(&dir) else { return vec![] };
        rd.filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("vibelauncher-"))
                    && de::read(p).is_some_and(|e| e.main("X-VibeLauncher-Profile").is_some())
            })
            .collect()
    }

    pub fn remove_path(path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn renders_valid_entry() {
            let mut p = Profile::default_for(crate::provider::ProviderKind::Claude);
            p.id = "work".into();
            p.name = "Work".into();
            p.isolated = true;
            let t = render(&p, Path::new("/x/work.png"), "/opt/Vibe Launcher/vibelauncher");
            let e = DesktopEntry::parse(&t);
            assert_eq!(e.main("Name").as_deref(), Some("Claude · Work"));
            assert_eq!(
                e.exec_args("Desktop Entry").unwrap(),
                vec!["/opt/Vibe Launcher/vibelauncher".to_string(), "launch".into(), "work".into()]
            );
        }
    }
}

// ───────────────────────────── macOS ─────────────────────────────

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::process::Command;

    fn dir() -> PathBuf {
        crate::paths::home_dir().join("Applications/VibeLauncher")
    }

    pub fn shortcut_path(p: &Profile) -> PathBuf {
        dir().join(format!("{}.app", safe_file_name(&display_name(p))))
    }

    fn marker(app: &Path) -> PathBuf {
        app.join("Contents/Resources/vibelauncher-profile")
    }

    pub fn create(paths: &Paths, p: &Profile, icon: &Path) -> Result<PathBuf> {
        // A renamed profile leaves its old app behind; remove it.
        for old in all_ours() {
            if std::fs::read_to_string(marker(&old)).ok().as_deref() == Some(p.id.as_str()) {
                let _ = std::fs::remove_dir_all(&old);
            }
        }
        let app = shortcut_path(p);
        std::fs::create_dir_all(dir())?;
        let exe = crate::paths::self_exe().to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "on run\n  do shell script quoted form of \"{exe}\" & \" launch {id} > /dev/null 2>&1 &\"\nend run\n",
            id = p.id
        );
        let src = paths.state_dir.join(format!("shortcut-{}.applescript", p.id));
        crate::paths::write_atomic(&src, script.as_bytes())?;
        let _ = std::fs::remove_dir_all(&app);
        if !Command::new("/usr/bin/osacompile").arg("-o").arg(&app).arg(&src).status()?.success() {
            anyhow::bail!("osacompile failed");
        }
        std::fs::copy(icon, app.join("Contents/Resources/applet.icns"))?;
        let _ = std::fs::remove_file(app.join("Contents/Resources/Assets.car"));
        let plist = app.join("Contents/Info.plist");
        let plutil = |args: &[&str]| Command::new("/usr/bin/plutil").args(args).arg(&plist).status();
        plutil(&["-replace", "CFBundleIdentifier", "-string", &format!("io.github.gloridust.vibelauncher.profile.{}", p.id)])?;
        plutil(&["-replace", "CFBundleName", "-string", &display_name(p)])?;
        plutil(&["-remove", "CFBundleIconName"])?;
        std::fs::write(marker(&app), &p.id)?;
        std::fs::write(app.join("Contents/Resources/vibelauncher-exe"), crate::paths::self_exe().to_string_lossy().as_bytes())?;
        let _ = Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&app).status();
        let _ = Command::new("/usr/bin/touch").arg(&app).status();
        Ok(app)
    }

    pub fn is_stale(app: &Path) -> bool {
        std::fs::read_to_string(app.join("Contents/Resources/vibelauncher-exe")).ok().as_deref()
            != Some(&*crate::paths::self_exe().to_string_lossy())
    }

    pub fn all_ours() -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(dir()) else { return vec![] };
        rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| marker(p).is_file()).collect()
    }

    pub fn remove_path(path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
        Ok(())
    }
}

// ───────────────────────────── Windows ─────────────────────────────

#[cfg(target_os = "windows")]
mod imp {
    use super::*;

    fn dir() -> PathBuf {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(crate::paths::home_dir);
        appdata.join("Microsoft\\Windows\\Start Menu\\Programs\\VibeLauncher")
    }

    pub fn shortcut_path(p: &Profile) -> PathBuf {
        dir().join(format!("{}.lnk", safe_file_name(&display_name(p))))
    }

    fn index(paths_dir: &Path) -> PathBuf {
        paths_dir.join(".vibelauncher-index")
    }

    pub fn create(_paths: &Paths, p: &Profile, icon: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(dir())?;
        // Remove an old shortcut for this profile (renamed profile).
        let idx = index(&dir());
        let mut lines: Vec<String> = std::fs::read_to_string(&idx).unwrap_or_default().lines().map(String::from).collect();
        lines.retain(|l| match l.split_once('\t') {
            Some((id, file)) if id == p.id => {
                let _ = std::fs::remove_file(dir().join(file));
                false
            }
            _ => true,
        });
        let path = shortcut_path(p);
        let exe = crate::paths::self_exe();
        let mut sl = mslnk::ShellLink::new(&exe)?;
        sl.set_arguments(Some(format!("launch {}", p.id)));
        sl.set_icon_location(Some(icon.to_string_lossy().into_owned()));
        sl.set_name(Some(format!("Open {} with the “{}” profile", p.provider.display_name(), p.name)));
        sl.create_lnk(&path)?;
        lines.push(format!("{}\t{}\t{}", p.id, path.file_name().unwrap().to_string_lossy(), exe.display()));
        std::fs::write(&idx, lines.join("\n"))?;
        Ok(path)
    }

    pub fn is_stale(path: &Path) -> bool {
        let exe = crate::paths::self_exe().display().to_string();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        !std::fs::read_to_string(index(&dir()))
            .unwrap_or_default()
            .lines()
            .any(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                f.len() == 3 && f[1] == name && f[2] == exe
            })
    }

    pub fn all_ours() -> Vec<PathBuf> {
        std::fs::read_to_string(index(&dir()))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split('\t').nth(1).map(|f| dir().join(f)))
            .filter(|p| p.exists())
            .collect()
    }

    pub fn remove_path(path: &Path) -> Result<()> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let idx = index(&dir());
        if let Ok(text) = std::fs::read_to_string(&idx) {
            let kept: Vec<&str> = text.lines().filter(|l| l.split('\t').nth(1) != Some(name.as_str())).collect();
            std::fs::write(&idx, kept.join("\n"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_names() {
        assert_eq!(safe_file_name("Claude · A/B: c?"), "Claude · A-B- c-");
    }
}
