//! Checking for and installing a new version of VibeLauncher.
//!
//! The manifest (`latest.json`) and the packages are published with every
//! GitHub release, in the same format the previous (Tauri) window used, so
//! either can update to the other. Every download is checked against our
//! public key before anything is installed.
//!
//! Installing replaces the package this copy came from: the AppImage file,
//! the `.app` bundle, or the Windows installer run in update mode. Debian and
//! RPM packages are installed with one `pkexec` prompt; dismissing it cancels.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;

/// The update key's public half (also in `tauri.conf.json`, for the bundler).
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDkwMDk3Qjc3N0VCNDYxNEUKUldST1liUitkM3NKa09pWU1NNjhueWkzODhENkg0YndmZjRwUzdVUVh3M1FBa0tGM3o5bCtxOVoK";
const FEED: &str = "https://github.com/Gloridust/VibeLauncher/releases/latest/download/latest.json";
/// The error shown in the window's own words when the password prompt was dismissed.
pub const CANCELLED: &str = "update-cancelled";

// The bundler writes the package kind over this placeholder (deb, rpm,
// AppImage, NSIS). `mut` and `#[used]` keep it from being optimised away or
// compared at compile time.
#[used]
static mut BUNDLE_TYPE: &str = "__TAURI_BUNDLE_TYPE_VAR_UNK";

fn bundle_marker() -> &'static str {
    // SAFETY: never written at run time; only read.
    unsafe { std::ptr::read_volatile(std::ptr::addr_of!(BUNDLE_TYPE)) }
}

/// How this copy was installed, if it can update itself.
///
/// `None` for builds that weren't made into a package (`cargo run`, a copied
/// binary): replacing "the current file" there would replace the wrong thing,
/// so the window offers the download page instead.
pub fn install_kind() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().ok()?;
        exe.ancestors().any(|p| p.extension().is_some_and(|e| e == "app")).then_some("app")
    }
    #[cfg(windows)]
    {
        (bundle_marker() == "__TAURI_BUNDLE_TYPE_VAR_NSS").then_some("nsis")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        match bundle_marker() {
            "__TAURI_BUNDLE_TYPE_VAR_APP" => std::env::var_os("APPIMAGE").is_some().then_some("appimage"),
            "__TAURI_BUNDLE_TYPE_VAR_DEB" => linux::pkexec().map(|_| "deb"),
            "__TAURI_BUNDLE_TYPE_VAR_RPM" => linux::pkexec().map(|_| "rpm"),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Found {
    pub version: String,
    url: Option<String>,
    signature: Option<String>,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into()
}

fn target_keys(kind: Option<&str>) -> Vec<String> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
    let mut keys = Vec::new();
    if let Some(k) = kind {
        keys.push(format!("{os}-{arch}-{k}"));
    }
    keys.push(format!("{os}-{arch}"));
    keys
}

/// Pick the newer release out of a manifest, if there is one.
fn parse(manifest: &str, current: &str, kind: Option<&str>) -> Result<Option<Found>, String> {
    let v: serde_json::Value = serde_json::from_str(manifest).map_err(|e| format!("The update feed isn't readable: {e}"))?;
    let version = v["version"].as_str().ok_or("The update feed has no version.")?.trim_start_matches('v').to_string();
    let newer = semver::Version::parse(&version).map_err(|e| e.to_string())? > semver::Version::parse(current).map_err(|e| e.to_string())?;
    if !newer {
        return Ok(None);
    }
    let entry = target_keys(kind).iter().find_map(|k| v["platforms"].get(k).cloned());
    Ok(Some(Found {
        version,
        url: entry.as_ref().and_then(|e| e["url"].as_str()).map(String::from),
        signature: entry.as_ref().and_then(|e| e["signature"].as_str()).map(String::from),
    }))
}

/// Ask the release feed whether there is a newer version.
pub fn check() -> Result<Option<Found>, String> {
    let body = agent(Duration::from_secs(20))
        .get(FEED)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    parse(&body, env!("CARGO_PKG_VERSION"), install_kind())
}

/// Download the package, reporting (downloaded, total) as it goes.
pub fn download(found: &Found, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, String> {
    let url = found.url.as_deref().ok_or("This release has no package for this system.")?;
    let mut resp = agent(Duration::from_secs(600)).get(url).call().map_err(|e| e.to_string())?;
    let total = resp.body().content_length();
    let mut reader = resp.body_mut().as_reader();
    let mut out = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        progress(out.len() as u64, total);
    }
    Ok(out)
}

fn b64_text(s: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Check the download against our public key.
pub fn verify(bytes: &[u8], found: &Found) -> Result<(), String> {
    verify_with(bytes, found.signature.as_deref(), PUBKEY)
}

/// `sig` and `pubkey` as the release feed and the Tauri signer write them:
/// base64 of the minisign text.
fn verify_with(bytes: &[u8], sig: Option<&str>, pubkey: &str) -> Result<(), String> {
    let bad = |e: String| format!("The update's signature doesn't check out ({e}); nothing was installed.");
    let sig = sig.ok_or_else(|| bad("no signature".into()))?;
    let key = minisign_verify::PublicKey::decode(&b64_text(pubkey).map_err(bad)?).map_err(|e| bad(e.to_string()))?;
    let sig = minisign_verify::Signature::decode(&b64_text(sig).map_err(bad)?).map_err(|e| bad(e.to_string()))?;
    key.verify(bytes, &sig, true).map_err(|e| bad(e.to_string()))
}

/// Install a verified package. On Windows the installer takes over and this
/// process exits; elsewhere call [`restart`] afterwards.
pub fn install(kind: &str, bytes: &[u8], version: &str) -> Result<(), String> {
    let _ = version;
    match kind {
        #[cfg(all(unix, not(target_os = "macos")))]
        "deb" | "rpm" => linux::install_package(kind, bytes),
        #[cfg(all(unix, not(target_os = "macos")))]
        "appimage" => linux::replace_appimage(bytes),
        #[cfg(target_os = "macos")]
        "app" => macos::replace_bundle(bytes),
        #[cfg(windows)]
        "nsis" => windows::run_installer(bytes, version),
        _ => Err("This copy of VibeLauncher can't update itself; download the new version instead.".into()),
    }
}

/// Start the newly installed version and quit this one.
pub fn restart(kind: &str) -> ! {
    let exe: PathBuf = match kind {
        "appimage" => std::env::var_os("APPIMAGE").map(PathBuf::from).unwrap_or_default(),
        _ => std::env::current_exe().unwrap_or_default(),
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    if kind == "appimage" {
        linux::leave_appimage_mount();
    }
    let _ = std::process::Command::new(exe).spawn();
    std::process::exit(0);
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::PathBuf;
    use std::process::Command;

    /// The AppImage starts us inside its own mount, with a handle on it and a
    /// pipe its runtime watches to know when to unmount. The new copy would
    /// inherit them and keep the old mount alive; mark every inherited handle
    /// close-on-exec and leave the mount's directory.
    pub fn leave_appimage_mount() {
        if let Ok(fds) = std::fs::read_dir("/proc/self/fd") {
            let fds: Vec<libc::c_int> = fds.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect();
            for n in fds.into_iter().filter(|&n| n > 2) {
                // SAFETY: only sets the close-on-exec flag; fails harmlessly
                // on a descriptor that is already closed.
                unsafe {
                    let flags = libc::fcntl(n, libc::F_GETFD);
                    if flags >= 0 {
                        libc::fcntl(n, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                    }
                }
            }
        }
        let appdir = std::env::var_os("APPDIR").map(PathBuf::from);
        let inside = |p: &std::path::Path| appdir.as_deref().is_some_and(|a| p.starts_with(a));
        let target = std::env::var_os("OWD")
            .map(PathBuf::from)
            .filter(|p| p.is_dir() && !inside(p))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_dir()))
            .unwrap_or_else(|| PathBuf::from("/"));
        let _ = std::env::set_current_dir(target);
    }

    pub fn pkexec() -> Option<PathBuf> {
        ["/usr/bin/pkexec", "/bin/pkexec"].iter().map(PathBuf::from).find(|p| p.exists())
    }

    /// Write the new AppImage next to the current one, then move it over it.
    pub fn replace_appimage(bytes: &[u8]) -> Result<(), String> {
        if !bytes.starts_with(b"\x7fELF") {
            return Err("The downloaded update isn't an AppImage.".into());
        }
        let path = PathBuf::from(std::env::var_os("APPIMAGE").ok_or("Not running from an AppImage.")?);
        let dir = path.parent().ok_or("The AppImage has no folder.")?;
        let tmp = dir.join(format!(".{}.update-{}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
        let mode = std::fs::metadata(&path).map(|m| m.permissions().mode()).unwrap_or(0o755);
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            std::fs::rename(&tmp, &path)
        };
        write().map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("Couldn't replace {}: {e}", path.display())
        })
    }

    /// Install a downloaded (and already signature-checked) package with one
    /// administrator prompt.
    pub fn install_package(kind: &str, bytes: &[u8]) -> Result<(), String> {
        let (looks_right, tool, args): (bool, &[&str], &[&str]) = match kind {
            "deb" => (bytes.starts_with(b"!<arch>\ndebian-binary"), &["/usr/bin/dpkg", "/bin/dpkg"], &["-i"]),
            _ => (bytes.starts_with(&[0xed, 0xab, 0xee, 0xdb]), &["/usr/bin/rpm", "/bin/rpm"], &["-U"]),
        };
        if !looks_right {
            return Err(format!("The downloaded update isn't a .{kind} package."));
        }
        let tool = tool.iter().map(PathBuf::from).find(|p| p.exists()).ok_or_else(|| format!("{} isn't installed.", tool[0]))?;
        let pkexec = pkexec().ok_or("pkexec isn't installed.")?;

        let path = std::env::temp_dir().join(format!("vibelauncher-update-{}.{kind}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&path).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        drop(f);

        let status = Command::new(pkexec).arg(tool).args(args).arg(&path).status();
        let _ = std::fs::remove_file(&path);
        let status = status.map_err(|e| e.to_string())?;
        match status.code() {
            Some(0) => Ok(()),
            // 126: the prompt was dismissed; 127: not authorized.
            Some(126) | Some(127) => Err(super::CANCELLED.into()),
            _ => Err(format!("Installing the update failed ({status}).")),
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};

    fn bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
    }

    /// Unpack the new `.app` next to the current one and swap them.
    pub fn replace_bundle(bytes: &[u8]) -> Result<(), String> {
        let app = bundle().ok_or("Not running from an app bundle.")?;
        let parent = app.parent().ok_or("The app has no folder.")?;
        let stage = parent.join(format!(".VibeLauncher-update-{}", std::process::id()));
        let old = parent.join(format!(".VibeLauncher-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&stage);
        let denied = |e: std::io::Error| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                format!("VibeLauncher can't write to {}; download the new version and install it by hand.", parent.display())
            } else {
                e.to_string()
            }
        };
        std::fs::create_dir(&stage).map_err(denied)?;
        let unpack = || -> std::io::Result<PathBuf> {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
            archive.unpack(&stage)?;
            std::fs::read_dir(&stage)?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or_else(|| std::io::Error::other("no app in the update"))
        };
        let new = match unpack() {
            Ok(p) => p,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&stage);
                return Err(e.to_string());
            }
        };
        let swap = || -> std::io::Result<()> {
            std::fs::rename(&app, &old)?;
            if let Err(e) = std::fs::rename(&new, &app) {
                let _ = std::fs::rename(&old, &app);
                return Err(e);
            }
            Ok(())
        };
        let r = swap().map_err(denied);
        let _ = std::fs::remove_dir_all(&stage);
        let _ = std::fs::remove_dir_all(&old);
        r
    }
}

#[cfg(windows)]
mod windows {
    /// Run the installer in update mode (its hooks keep links and shortcuts);
    /// it closes this window, installs, and starts the new version.
    pub fn run_installer(bytes: &[u8], version: &str) -> Result<(), String> {
        if !bytes.starts_with(b"MZ") {
            return Err("The downloaded update isn't an installer.".into());
        }
        let dir = std::env::temp_dir().join(format!("VibeLauncher-{version}-update"));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("VibeLauncher_{version}_x64-setup.exe"));
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        std::process::Command::new(&path).args(["/P", "/UPDATE", "/R"]).spawn().map_err(|e| e.to_string())?;
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{"version":"0.3.0","platforms":{
        "linux-x86_64":{"url":"https://x/a.AppImage","signature":"s1"},
        "linux-x86_64-deb":{"url":"https://x/a.deb","signature":"s2"}}}"#;

    #[test]
    fn picks_the_package_of_our_kind() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let f = parse(MANIFEST, "0.2.0", Some("deb")).unwrap().unwrap();
        assert_eq!(f.url.as_deref(), Some("https://x/a.deb"));
        let f = parse(MANIFEST, "0.2.0", Some("appimage")).unwrap().unwrap();
        assert_eq!(f.url.as_deref(), Some("https://x/a.AppImage"));
        assert!(parse(MANIFEST, "0.3.0", None).unwrap().is_none());
        assert!(parse(MANIFEST, "0.4.0-beta.1", None).unwrap().is_none());
    }

    #[test]
    fn public_key_matches_the_bundler_config() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["plugins"]["updater"]["pubkey"].as_str(), Some(PUBKEY));
        assert!(minisign_verify::PublicKey::decode(&b64_text(PUBKEY).unwrap()).is_ok());
    }

    /// Signed with `tauri signer sign` and a throwaway key: the same tool and
    /// format the release workflow uses.
    #[test]
    fn accepts_what_the_tauri_signer_signs() {
        let data = include_bytes!("update-fixture.bin");
        let sig = include_str!("update-fixture.sig").trim();
        let key = include_str!("update-fixture.pub").trim();
        assert_eq!(verify_with(data, Some(sig), key), Ok(()));
        let mut tampered = data.to_vec();
        tampered[0] ^= 1;
        assert!(verify_with(&tampered, Some(sig), key).is_err());
        // Signed, but not with our key.
        assert!(verify_with(data, Some(sig), PUBKEY).is_err());
    }

    #[test]
    fn rejects_a_bad_signature() {
        let f = Found { version: "1.0.0".into(), url: None, signature: Some("bm9wZQ==".into()) };
        assert!(verify(b"data", &f).is_err());
    }
}
