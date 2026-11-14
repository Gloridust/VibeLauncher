//! Finding the installed official apps.
//!
//! Nothing is hard-coded as *the* location: each platform asks the system
//! first (desktop entries / Launch Services / MSIX packages), then falls back
//! to well-known paths, and the user can always pick the executable by hand.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::provider::ProviderKind;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppInstall {
    pub provider: ProviderKind,
    /// The command to run; `base_args` go right after it (e.g. a Flatpak's
    /// `run --branch=… app.id`).
    pub program: PathBuf,
    pub base_args: Vec<String>,
    /// macOS `.app` bundle.
    pub bundle: Option<PathBuf>,
    pub version: Option<String>,
    /// Human-readable origin: "desktop entry", "manual", "Launch Services", …
    pub source: String,
    /// Linux: id of the app's own desktop entry (e.g. `com.anthropic.Claude.desktop`).
    pub desktop_id: Option<String>,
    pub desktop_file: Option<PathBuf>,
    /// Best available icon (PNG or ICNS).
    pub icon: Option<PathBuf>,
    /// Executables that identify a running main process of this install.
    pub match_exes: Vec<PathBuf>,
}

impl AppInstall {
    fn new(provider: ProviderKind, program: PathBuf, source: &str) -> Self {
        let mut me = Self {
            provider,
            match_exes: vec![],
            base_args: vec![],
            bundle: None,
            version: None,
            source: source.into(),
            desktop_id: None,
            desktop_file: None,
            icon: None,
            program,
        };
        me.fill_match_exes();
        me
    }

    fn fill_match_exes(&mut self) {
        let mut v = vec![self.program.clone()];
        if let Ok(c) = self.program.canonicalize() {
            v.push(c.clone());
            // Linux packages often ship a shell wrapper next to the real binary
            // (ChatGPT: codex-launcher → ChatGPT).
            if let Some(dir) = c.parent() {
                for name in self.provider.linux_known_binaries() {
                    let n = Path::new(name).file_name().unwrap();
                    let cand = dir.join(n);
                    if cand.is_file() && !v.contains(&cand) {
                        v.push(cand);
                    }
                }
            }
        }
        v.dedup();
        self.match_exes = v;
    }

    pub fn exists(&self) -> bool {
        self.program.exists() || self.bundle.as_ref().is_some_and(|b| b.exists())
    }

    /// Directory holding the Electron resources (for version and icon lookup).
    fn resources_dir(&self) -> Option<PathBuf> {
        if let Some(b) = &self.bundle {
            return Some(b.join("Contents/Resources"));
        }
        for exe in &self.match_exes {
            let dir = exe.parent()?;
            let r = dir.join("resources");
            if r.join("app.asar").is_file() {
                return Some(r);
            }
        }
        None
    }

    fn fill_version_and_icon(&mut self) {
        if self.version.is_none() {
            self.version = self.resources_dir().and_then(|r| asar_package_version(&r.join("app.asar")));
        }
        if self.icon.is_none() {
            if let Some(r) = self.resources_dir() {
                for n in ["icon.png", "icon-chatgpt.png"] {
                    if r.join(n).is_file() {
                        self.icon = Some(r.join(n));
                        break;
                    }
                }
            }
        }
    }
}

/// Read `version` from the app's own `package.json` inside `app.asar`.
/// Works the same on every platform and needs no package manager.
///
/// The archive's index lists every file in the app (megabytes of JSON), so
/// only the one entry is decoded, and the answer is kept for as long as the
/// file is unchanged — the window asks on every refresh.
pub fn asar_package_version(asar: &Path) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<String>>>> = OnceLock::new();
    let meta = std::fs::metadata(asar).ok()?;
    let key: Key = (asar.to_path_buf(), meta.len(), meta.modified().ok());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().ok()?.get(&key) {
        return v.clone();
    }
    let v = read_asar_package_version(asar);
    if let Ok(mut c) = cache.lock() {
        c.retain(|k, _| k.0 != key.0);
        c.insert(key, v.clone());
    }
    v
}

fn read_asar_package_version(asar: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    #[derive(serde::Deserialize)]
    struct Header {
        files: Files,
    }
    #[derive(serde::Deserialize)]
    struct Files {
        #[serde(rename = "package.json")]
        package: Entry,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        size: u64,
        offset: String,
    }
    #[derive(serde::Deserialize)]
    struct Package {
        version: String,
    }
    let mut f = std::fs::File::open(asar).ok()?;
    let mut head = [0u8; 16];
    f.read_exact(&mut head).ok()?;
    let word = |i: usize| u32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap()) as u64;
    let header_size = word(1);
    let json_len = word(3);
    if json_len > 64 << 20 {
        return None;
    }
    let mut json = vec![0u8; json_len as usize];
    f.read_exact(&mut json).ok()?;
    let header: Header = serde_json::from_slice(&json).ok()?;
    drop(json);
    let Entry { size, offset } = header.files.package;
    let offset: u64 = offset.parse().ok()?;
    if size > 4 << 20 {
        return None;
    }
    f.seek(SeekFrom::Start(8 + header_size + offset)).ok()?;
    let mut pkg = vec![0u8; size as usize];
    f.read_exact(&mut pkg).ok()?;
    let pkg: Package = serde_json::from_slice(&pkg).ok()?;
    Some(pkg.version)
}

/// All installs found for `provider`, best first. `override_path` (chosen by
/// the user) always wins.
pub fn find_installs(provider: ProviderKind, override_path: Option<&Path>) -> Vec<AppInstall> {
    let mut out: Vec<AppInstall> = Vec::new();
    if let Some(p) = override_path {
        if let Some(mut i) = from_manual_path(provider, p) {
            i.fill_version_and_icon();
            out.push(i);
        }
    }
    for mut i in platform_installs(provider) {
        i.fill_version_and_icon();
        let dup = out.iter().any(|o| {
            o.match_exes.iter().any(|e| i.match_exes.contains(e)) && o.base_args == i.base_args
        });
        if !dup {
            out.push(i);
        }
    }
    out
}

pub fn find_install(provider: ProviderKind, override_path: Option<&Path>) -> Option<AppInstall> {
    // The user's choice wins, so there's no need to search (on Windows that
    // means asking PowerShell for Store packages, about a second each).
    if let Some(mut i) = override_path.and_then(|p| from_manual_path(provider, p)) {
        i.fill_version_and_icon();
        return Some(i);
    }
    find_installs(provider, override_path).into_iter().next()
}

/// The user picked a file: an executable, a `.app`, or a `.desktop` entry.
pub fn from_manual_path(provider: ProviderKind, p: &Path) -> Option<AppInstall> {
    if !p.exists() {
        return None;
    }
    if p.extension().is_some_and(|e| e == "app") {
        #[cfg(target_os = "macos")]
        return macos::from_bundle(provider, p, "manual");
        #[cfg(not(target_os = "macos"))]
        return None;
    }
    if p.extension().is_some_and(|e| e == "desktop") {
        return linux_from_desktop(provider, p, "manual");
    }
    Some(AppInstall::new(provider, p.to_path_buf(), "manual"))
}

/// Locate a command on PATH.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let cmd_path = Path::new(cmd);
    if cmd_path.is_absolute() {
        return cmd_path.is_file().then(|| cmd_path.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let c = dir.join(cmd);
        if c.is_file() {
            return Some(c);
        }
        #[cfg(windows)]
        {
            let c = dir.join(format!("{cmd}.exe"));
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

fn linux_from_desktop(provider: ProviderKind, file: &Path, source: &str) -> Option<AppInstall> {
    use crate::desktop_entry as de;
    let entry = de::read(file)?;
    if entry.main("X-VibeLauncher").is_some() {
        return None;
    }
    let args: Vec<String> = entry
        .exec_args("Desktop Entry")?
        .into_iter()
        .filter(|a| !de::is_field_code(a))
        .collect();
    let (first, rest) = args.split_first()?;
    let program = which(first)?;
    let mut i = AppInstall::new(provider, program, source);
    i.base_args = rest.to_vec();
    i.desktop_id = file.file_name().map(|n| n.to_string_lossy().into_owned());
    i.desktop_file = Some(file.to_path_buf());
    i.icon = entry.main("Icon").and_then(|name| linux_icon(&name));
    Some(i)
}

/// Resolve an icon name to the largest PNG in the hicolor theme or pixmaps.
pub fn linux_icon(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    let mut best: Option<(u32, PathBuf)> = None;
    for dir in crate::desktop_entry::xdg_data_dirs() {
        let theme = dir.join("icons/hicolor");
        let Ok(rd) = std::fs::read_dir(&theme) else { continue };
        for size_dir in rd.flatten() {
            let size_name = size_dir.file_name().to_string_lossy().into_owned();
            let size: u32 = size_name.split('x').next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let cand = size_dir.path().join("apps").join(format!("{name}.png"));
            if cand.is_file() && best.as_ref().is_none_or(|(s, _)| size > *s) {
                best = Some((size, cand));
            }
        }
        let pix = dir.join("pixmaps").join(format!("{name}.png"));
        if best.is_none() && pix.is_file() {
            best = Some((0, pix));
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(target_os = "linux")]
fn platform_installs(provider: ProviderKind) -> Vec<AppInstall> {
    use crate::desktop_entry as de;
    let mut out = Vec::new();
    let mime = format!("x-scheme-handler/{}", provider.schemes()[0]);
    // 1. Desktop entries that handle the app's URL scheme (covers distro
    //    packages, community builds, Flatpak, Snap), known ids first.
    let mut files: Vec<PathBuf> = Vec::new();
    for id in provider.linux_desktop_ids() {
        for f in de::find_by_id(id) {
            if de::read(&f).is_some_and(|e| e.main("X-VibeLauncher").is_none()) && !files.contains(&f) {
                files.push(f);
            }
        }
    }
    for (_, f) in de::system_handlers_of(&mime) {
        if !files.contains(&f) {
            files.push(f);
        }
    }
    for f in files {
        if let Some(i) = linux_from_desktop(provider, &f, "desktop entry") {
            out.push(i);
        }
    }
    // 2. Well-known binaries and commands.
    for b in provider.linux_known_binaries() {
        let p = PathBuf::from(b);
        if p.is_file() {
            out.push(AppInstall::new(provider, p, "known path"));
        }
    }
    for c in provider.linux_commands() {
        if let Some(p) = which(c) {
            out.push(AppInstall::new(provider, p, "PATH"));
        }
    }
    // Icon fallback for non-desktop finds.
    for i in &mut out {
        if i.icon.is_none() {
            i.icon = provider.linux_commands().iter().find_map(|c| linux_icon(c));
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn platform_installs(provider: ProviderKind) -> Vec<AppInstall> {
    macos::installs(provider)
}

#[cfg(target_os = "windows")]
fn platform_installs(provider: ProviderKind) -> Vec<AppInstall> {
    windows::installs(provider)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_installs(_: ProviderKind) -> Vec<AppInstall> {
    vec![]
}

#[cfg(target_os = "macos")]
pub mod macos {
    use super::*;
    use std::process::Command;

    pub fn installs(provider: ProviderKind) -> Vec<AppInstall> {
        let mut bundles: Vec<PathBuf> = Vec::new();
        for id in provider.macos_bundle_ids() {
            if let Ok(out) = Command::new("/usr/bin/mdfind")
                .arg(format!("kMDItemCFBundleIdentifier == '{id}'"))
                .output()
            {
                for l in String::from_utf8_lossy(&out.stdout).lines() {
                    let p = PathBuf::from(l.trim());
                    if p.extension().is_some_and(|e| e == "app") && !bundles.contains(&p) {
                        bundles.push(p);
                    }
                }
            }
        }
        let home = crate::paths::home_dir();
        for base in [PathBuf::from("/Applications"), home.join("Applications")] {
            for n in provider.macos_app_names() {
                let p = base.join(n);
                if p.is_dir() && !bundles.contains(&p) {
                    bundles.push(p);
                }
            }
        }
        // Prefer /Applications over copies inside DMGs or Downloads.
        bundles.sort_by_key(|b| !b.starts_with("/Applications"));
        bundles.iter().filter_map(|b| from_bundle(provider, b, "Launch Services")).collect()
    }

    pub fn plist_value(bundle: &Path, key: &str) -> Option<String> {
        let out = Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(bundle.join("Contents/Info.plist"))
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn from_bundle(provider: ProviderKind, bundle: &Path, source: &str) -> Option<AppInstall> {
        let exe_name = plist_value(bundle, "CFBundleExecutable")?;
        let exe = bundle.join("Contents/MacOS").join(exe_name);
        if !exe.is_file() {
            return None;
        }
        let mut i = AppInstall::new(provider, exe, source);
        i.bundle = Some(bundle.to_path_buf());
        i.version = plist_value(bundle, "CFBundleShortVersionString");
        if let Some(icon) = plist_value(bundle, "CFBundleIconFile") {
            let mut p = bundle.join("Contents/Resources").join(&icon);
            if p.extension().is_none() {
                p.set_extension("icns");
            }
            i.icon = p.is_file().then_some(p);
        }
        Some(i)
    }
}

#[cfg(target_os = "windows")]
pub mod windows {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn powershell(script: &str) -> Option<String> {
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn find_exe(dir: &Path, names: &[&str], depth: u32) -> Option<PathBuf> {
        let rd = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            let n = e.file_name().to_string_lossy().to_lowercase();
            if p.is_file() && names.contains(&n.as_str()) {
                return Some(p);
            }
            if p.is_dir() {
                subdirs.push(p);
            }
        }
        if depth == 0 {
            return None;
        }
        subdirs.into_iter().find_map(|d| find_exe(&d, names, depth - 1))
    }

    fn largest_png(dir: &Path) -> Option<PathBuf> {
        let mut best: Option<(u64, PathBuf)> = None;
        for sub in [dir.join("assets"), dir.join("Assets"), dir.join("images"), dir.to_path_buf()] {
            let Ok(rd) = std::fs::read_dir(&sub) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let n = e.file_name().to_string_lossy().to_lowercase();
                if n.ends_with(".png") && (n.contains("logo") || n.contains("icon")) {
                    let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                    if best.as_ref().is_none_or(|(l, _)| len > *l) {
                        best = Some((len, p));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }

    pub fn installs(provider: ProviderKind) -> Vec<AppInstall> {
        let mut out = Vec::new();
        // 1. MSIX packages: resolve the install location at run time so app
        //    updates (which change the versioned folder) never break us.
        let filter = provider
            .windows_package_names()
            .iter()
            .map(|n| format!("$_.Name -like '*{n}*'"))
            .collect::<Vec<_>>()
            .join(" -or ");
        let script = format!(
            "Get-AppxPackage | Where-Object {{ ({filter}) -and $_.SignatureKind -ne 'System' }} | Select-Object Name,InstallLocation,Version | ConvertTo-Json -Compress"
        );
        if let Some(json) = powershell(&script) {
            let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap_or(serde_json::Value::Null);
            let items = match v {
                serde_json::Value::Array(a) => a,
                serde_json::Value::Null => vec![],
                o => vec![o],
            };
            for it in items {
                let Some(loc) = it.get("InstallLocation").and_then(|x| x.as_str()) else { continue };
                let dir = PathBuf::from(loc);
                if let Some(exe) = find_exe(&dir, provider.windows_exe_names(), 3) {
                    let mut i = AppInstall::new(provider, exe, "MSIX package");
                    i.version = it.get("Version").and_then(|x| x.as_str()).map(String::from);
                    i.icon = largest_png(&dir);
                    out.push(i);
                }
            }
        }
        // 2. Classic per-user (Squirrel) installs.
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            let roots: &[&str] = match provider {
                ProviderKind::Claude => &["AnthropicClaude", "Programs\\Claude"],
                ProviderKind::ChatGpt => &["Programs\\ChatGPT", "ChatGPT", "Programs\\Codex"],
            };
            for r in roots {
                let dir = local.join(r);
                if let Some(exe) = find_exe(&dir, provider.windows_exe_names(), 0) {
                    out.push(AppInstall::new(provider, exe, "per-user install"));
                }
            }
        }
        // 3. Whatever the URL scheme points at.
        for scheme in provider.schemes() {
            if let Some(exe) = crate::scheme::windows_registered_exe(scheme) {
                if exe.is_file() && !exe.to_string_lossy().to_lowercase().contains("vibelauncher") {
                    out.push(AppInstall::new(provider, exe, "URL handler"));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asar_version_reader() {
        // Build a tiny asar: [4][hdr_size][json_len+4][json_len] json body
        let body = br#"{"name":"x","version":"1.2.3"}"#;
        let json = format!(r#"{{"files":{{"package.json":{{"size":{},"offset":"0"}}}}}}"#, body.len());
        let jl = json.len() as u32;
        let pad = (4 - (jl % 4)) % 4;
        let hdr = 4 + 4 + jl + pad;
        let mut bytes = Vec::new();
        for w in [4u32, hdr, hdr - 4, jl] {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        bytes.extend_from_slice(json.as_bytes());
        bytes.extend(std::iter::repeat_n(0u8, pad as usize));
        bytes.extend_from_slice(body);
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("app.asar");
        std::fs::write(&f, bytes).unwrap();
        assert_eq!(asar_package_version(&f).as_deref(), Some("1.2.3"));
    }
}
