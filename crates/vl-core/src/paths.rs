//! Where VibeLauncher keeps its own files.
//!
//! Every path can be redirected with `VIBELAUNCHER_HOME` (used by tests and by
//! people who want a portable setup): config, data and state then all live
//! under that directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;

#[derive(Debug, Clone)]
pub struct Paths {
    /// `profiles.toml` lives here.
    pub config_dir: PathBuf,
    /// Default parent of every isolated profile's data.
    pub profiles_dir: PathBuf,
    /// Pending-login markers, router log, generated icons.
    pub state_dir: PathBuf,
}

impl Paths {
    pub fn detect() -> Result<Self> {
        if let Some(home) = std::env::var_os("VIBELAUNCHER_HOME").filter(|v| !v.is_empty()) {
            return Ok(Self::under(Path::new(&home)));
        }
        let dirs = ProjectDirs::from("io.github", "Gloridust", "VibeLauncher")
            .context("cannot determine the home directory")?;
        // On Linux these follow XDG; on macOS they are under
        // ~/Library/Application Support; on Windows %APPDATA% / %LOCALAPPDATA%.
        let config_dir = if cfg!(target_os = "linux") {
            dirs.config_dir().with_file_name("vibelauncher")
        } else {
            dirs.config_dir().to_path_buf()
        };
        let data_dir = if cfg!(target_os = "linux") {
            dirs.data_dir().with_file_name("vibelauncher")
        } else {
            dirs.data_local_dir().to_path_buf()
        };
        let state_dir = if cfg!(target_os = "linux") {
            dirs.state_dir()
                .map(|d| d.with_file_name("vibelauncher"))
                .unwrap_or_else(|| data_dir.join("state"))
        } else {
            data_dir.join("state")
        };
        // Store (MSIX) builds of the apps see a virtualized %APPDATA% /
        // %LOCALAPPDATA%: a profile dir there could be redirected into the
        // package's private copy. Keep profile data in the home folder.
        let profiles_dir = if cfg!(target_os = "windows") {
            home_dir().join("VibeLauncher Profiles")
        } else {
            data_dir.join("profiles")
        };
        Ok(Self { config_dir, profiles_dir, state_dir })
    }

    pub fn under(root: &Path) -> Self {
        Self {
            config_dir: root.join("config"),
            profiles_dir: root.join("profiles"),
            state_dir: root.join("state"),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("profiles.toml")
    }

    pub fn icons_dir(&self) -> PathBuf {
        self.state_dir.join("icons")
    }

    pub fn ensure(&self) -> Result<()> {
        for d in [&self.config_dir, &self.profiles_dir, &self.state_dir] {
            create_private_dir(d)?;
        }
        Ok(())
    }
}

/// Create a directory (and parents) readable only by the current user.
pub fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Write a file atomically: write a sibling temp file, then rename over.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// File name of the command-line program (what shortcuts and link handlers run).
pub const CLI_NAME: &str = if cfg!(windows) { "vibelauncher.exe" } else { "vibelauncher" };

/// Path of the `vibelauncher` command, as other programs should call it.
///
/// Shortcuts and link handlers always point at the small command-line program,
/// never at the GUI, so opening a profile doesn't load a web view. When called
/// from the GUI, this is the CLI sitting next to it. For an AppImage it is the
/// AppImage file itself (it dispatches by arguments).
pub fn self_exe() -> PathBuf {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return PathBuf::from(appimage);
    }
    if let Some(over) = std::env::var_os("VIBELAUNCHER_EXE").filter(|v| !v.is_empty()) {
        return PathBuf::from(over);
    }
    let me = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()).map(plain);
    if let Some(me) = &me {
        if me.file_name().is_some_and(|n| n == CLI_NAME) {
            return me.clone();
        }
        if let Some(dir) = me.parent() {
            let cli = dir.join(CLI_NAME);
            if cli.is_file() {
                return cli;
            }
            // macOS app bundle: GUI in Contents/MacOS, CLI next to it.
        }
    }
    me.unwrap_or_else(|| PathBuf::from("vibelauncher"))
}

/// The GUI program, next to the CLI.
pub fn gui_exe() -> Option<PathBuf> {
    let name = if cfg!(windows) { "vibelauncher-gui.exe" } else { "vibelauncher-gui" };
    if let Some(over) = std::env::var_os("VIBELAUNCHER_GUI").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(over));
    }
    // Inside an AppImage the image itself opens the window when run without
    // a command.
    if let Some(img) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(img));
    }
    let me = plain(std::env::current_exe().ok()?.canonicalize().ok()?);
    let p = me.parent()?.join(name);
    p.is_file().then_some(p)
}

/// Undo the `\\?\` form `canonicalize` gives on Windows.
///
/// Paths written into the registry, shortcuts and command lines have to be
/// ordinary ones: the shell refuses to run a link handler written as
/// `"\\?\C:\…\vibelauncher.exe"` (it shows an error box instead), and many
/// programs don't understand the prefix. Paths that only work in the long form
/// (longer than 260 characters) are left as they are.
pub fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    let short = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
            rest.to_string()
        } else {
            return p;
        }
    } else {
        return p;
    };
    if short.len() >= 260 { p } else { PathBuf::from(short) }
}

pub fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::plain;
    use std::path::PathBuf;

    #[test]
    fn plain_paths() {
        let p = |s: &str| plain(PathBuf::from(s)).to_string_lossy().into_owned();
        assert_eq!(p(r"\\?\D:\a\b\vibelauncher.exe"), r"D:\a\b\vibelauncher.exe");
        assert_eq!(p(r"\\?\UNC\server\share\v.exe"), r"\\server\share\v.exe");
        assert_eq!(p(r"\\?\Volume{1234}\x"), r"\\?\Volume{1234}\x");
        assert_eq!(p("/usr/bin/vibelauncher"), "/usr/bin/vibelauncher");
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(p(&long), long);
    }
}
