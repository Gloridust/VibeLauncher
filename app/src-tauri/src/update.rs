//! Checking for and installing a new version of VibeLauncher.
//!
//! The update manifest (`latest.json`) and the packages are published with
//! every GitHub release; each package is signed and checked against the public
//! key in `tauri.conf.json` before anything is installed.
//!
//! Installing replaces the package the window came from: the AppImage file,
//! the `.app` bundle, or the Windows installer run in update mode. Debian and
//! RPM packages are installed with one `pkexec` prompt; cancelling it cancels
//! the update (the plugin's own deb/rpm installer would fall back to `sudo`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

type Res<T> = Result<T, String>;

/// Errors the page shows in its own words.
pub const CANCELLED: &str = "update-cancelled";

#[derive(Default)]
pub struct UpdateState {
    found: Mutex<Option<Update>>,
    busy: AtomicBool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current: String,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// How this copy was installed, if it can update itself.
///
/// `None` for builds that weren't made into a package (`cargo run`, a copied
/// binary): replacing "the current file" there would replace the wrong thing,
/// so the page offers the download page instead.
pub fn install_kind() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        // Only from inside a .app bundle.
        let exe = std::env::current_exe().ok()?;
        exe.ancestors().any(|p| p.extension().is_some_and(|e| e == "app")).then_some("app")
    }
    #[cfg(windows)]
    {
        use tauri::utils::{config::BundleType, platform::bundle_type};
        matches!(bundle_type(), Some(BundleType::Nsis)).then_some("nsis")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use tauri::utils::{config::BundleType, platform::bundle_type};
        match bundle_type() {
            Some(BundleType::AppImage) => std::env::var_os("APPIMAGE").is_some().then_some("appimage"),
            Some(BundleType::Deb) => linux::pkexec().map(|_| "deb"),
            Some(BundleType::Rpm) => linux::pkexec().map(|_| "rpm"),
            _ => None,
        }
    }
}

fn info(u: &Update) -> UpdateInfo {
    UpdateInfo {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        date: u.raw_json.get("pub_date").and_then(|v| v.as_str()).map(String::from),
    }
}

/// Ask the release feed whether there is a newer version.
#[tauri::command]
pub async fn check_update(app: AppHandle, st: State<'_, UpdateState>) -> Res<Option<UpdateInfo>> {
    let updater = app.updater_builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let view = found.as_ref().map(info);
    *st.found.lock().unwrap_or_else(|p| p.into_inner()) = found;
    Ok(view)
}

/// Download, check and install the version found by [`check_update`], then
/// start the new version. Progress goes out as `update-progress` events.
#[tauri::command]
pub async fn install_update(app: AppHandle, st: State<'_, UpdateState>) -> Res<()> {
    let kind = install_kind().ok_or("This copy of VibeLauncher can't update itself; download the new version instead.")?;
    let update = st.found.lock().unwrap_or_else(|p| p.into_inner()).clone().ok_or("Check for updates first.")?;
    if st.busy.swap(true, Ordering::SeqCst) {
        return Err("An update is already being installed.".into());
    }
    let r = download_and_install(&app, &update, kind).await;
    st.busy.store(false, Ordering::SeqCst);
    r?;
    #[cfg(all(unix, not(target_os = "macos")))]
    if kind == "appimage" {
        linux::leave_appimage_mount();
    }
    // Windows never gets here: the installer takes over and closes the app.
    app.restart();
}

async fn download_and_install(app: &AppHandle, update: &Update, kind: &str) -> Res<()> {
    let mut downloaded = 0u64;
    let mut last = Instant::now() - Duration::from_secs(1);
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if last.elapsed() >= Duration::from_millis(100) || total == Some(downloaded) {
                    last = Instant::now();
                    let _ = app.emit("update-progress", Progress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("update-installing", ());
    match kind {
        #[cfg(all(unix, not(target_os = "macos")))]
        "deb" | "rpm" => {
            let kind = kind.to_string();
            tauri::async_runtime::spawn_blocking(move || linux::install_package(&kind, &bytes))
                .await
                .map_err(|e| e.to_string())?
        }
        _ => {
            let update = update.clone();
            tauri::async_runtime::spawn_blocking(move || update.install(bytes))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// The AppImage starts us inside its own mount, with a handle on it and a
    /// pipe its runtime watches to know when to unmount. The restarted copy
    /// would inherit all three and keep the old mount, and the old file Tauri
    /// set aside in /tmp, alive for as long as it runs. Mark every inherited
    /// handle close-on-exec (the new copy needs none of them) and go back to
    /// the directory the AppImage was started from.
    pub fn leave_appimage_mount() {
        #[cfg(target_os = "linux")]
        if let Ok(fds) = std::fs::read_dir("/proc/self/fd") {
            let fds: Vec<libc::c_int> = fds.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).collect();
            for n in fds.into_iter().filter(|&n| n > 2) {
                // SAFETY: only sets the close-on-exec flag; fails harmlessly on
                // a descriptor that is already closed (the directory listing's own).
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
