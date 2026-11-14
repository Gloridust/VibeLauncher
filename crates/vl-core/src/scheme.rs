//! Taking over an app's URL scheme so callbacks reach the right profile.
//!
//! Linux (tested): place a desktop entry with the *same id* as the app's own
//! (e.g. `com.anthropic.Claude.desktop`) in the user's applications dir. The
//! XDG spec gives it precedence over the system copy, the app still sees itself
//! as the default handler (so it neither re-registers nor changes its login
//! flow), and every `claude://` URL goes through VibeLauncher. If VibeLauncher
//! is ever removed, the entry falls back to running the app directly.
//!
//! macOS / Windows (experimental): register VibeLauncher as the handler and
//! re-assert it shortly after each launch, because the apps reclaim the scheme
//! when they start.

use std::path::PathBuf;

#[allow(unused_imports)]
use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::install::AppInstall;
use crate::paths::Paths;
use crate::provider::ProviderKind;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchemeStatus {
    pub provider: ProviderKind,
    /// "shadow-desktop-entry" | "registry" | "launch-services" | "none"
    pub strategy: &'static str,
    pub supported: bool,
    pub experimental: bool,
    pub claimed: bool,
    /// Claimed but generated from an older copy of the app's entry.
    pub stale: bool,
    /// Something else (not ours) is in the way.
    pub conflict: Option<String>,
    /// Current handler, as the system reports it.
    pub handler: Option<String>,
}

pub fn status(provider: ProviderKind, install: Option<&AppInstall>, paths: &Paths) -> SchemeStatus {
    imp::status(provider, install, paths)
}

pub fn claim(provider: ProviderKind, install: &AppInstall, paths: &Paths) -> Result<()> {
    imp::claim(provider, install, paths)
}

pub fn release(provider: ProviderKind, install: Option<&AppInstall>, paths: &Paths) -> Result<()> {
    imp::release(provider, install, paths)
}

/// Re-register after the app had a chance to reclaim the scheme. No-op where
/// the app doesn't reclaim (Linux).
pub fn reassert(provider: ProviderKind, install: &AppInstall, paths: &Paths) -> Result<()> {
    imp::reassert(provider, install, paths)
}

pub fn needs_reassert() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

// ───────────────────────────── Linux ─────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::*;
    use crate::desktop_entry::{self as de, DesktopEntry};
    use sha2::{Digest, Sha256};

    fn target_id(provider: ProviderKind, install: Option<&AppInstall>) -> Option<String> {
        if let Some(id) = install.and_then(|i| i.desktop_id.clone()) {
            return Some(id);
        }
        provider
            .linux_desktop_ids()
            .iter()
            .find(|id| system_file(id).is_some())
            .map(|s| s.to_string())
    }

    /// The app's own desktop file (outside the user dir).
    fn system_file(id: &str) -> Option<PathBuf> {
        let user = de::user_applications_dir().join(id);
        de::find_by_id(id).into_iter().find(|p| *p != user)
    }

    fn shadow_path(id: &str) -> PathBuf {
        de::user_applications_dir().join(id)
    }

    fn hash(text: &str) -> String {
        let h = Sha256::digest(text.as_bytes());
        h.iter().take(12).map(|b| format!("{b:02x}")).collect()
    }

    fn shell_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// Build the shadow entry from the app's own entry.
    pub fn render_shadow(
        provider: ProviderKind,
        source_text: &str,
        source_path: &str,
        self_exe: &str,
    ) -> String {
        let mut entry = DesktopEntry::parse(source_text);
        let groups = entry.groups();
        let main_args = entry.exec_args("Desktop Entry").unwrap_or_default();
        // Arguments that are part of *how* to run the app (program + e.g.
        // `flatpak run … app.id`), as opposed to the URL / field codes after.
        let prefix_len = main_args
            .iter()
            .position(|a| de::is_field_code(a))
            .unwrap_or(main_args.len())
            .max(1);
        for g in groups {
            let Some(args) = entry.exec_args(&g) else {
                continue;
            };
            let n = prefix_len.min(args.len());
            let (prefix, tail) = args.split_at(n);
            let fallback = prefix
                .iter()
                .map(|a| shell_quote(a))
                .collect::<Vec<_>>()
                .join(" ");
            let script = format!(
                // Any failure of VibeLauncher (removed, broken config, crash)
                // falls through to running the app itself.
                "if [ -x \"$0\" ] && \"$0\" open-url --provider {} -- \"$@\"; then exit 0; fi; exec {} \"$@\"",
                provider.id(),
                fallback
            );
            let mut new_args = vec![
                "/bin/sh".to_string(),
                "-c".into(),
                script,
                self_exe.to_string(),
            ];
            new_args.extend(tail.iter().cloned());
            entry.set_raw(&g, "Exec", &de::build_exec(&new_args));
            entry.remove(&g, "TryExec");
        }
        // D-Bus activation would bypass Exec entirely.
        entry.remove("Desktop Entry", "DBusActivatable");
        entry.set_raw("Desktop Entry", "X-VibeLauncher", "true");
        entry.set_raw(
            "Desktop Entry",
            "X-VibeLauncher-Source",
            &de::escape_string(source_path),
        );
        entry.set_raw(
            "Desktop Entry",
            "X-VibeLauncher-Source-Hash",
            &hash(source_text),
        );
        let mut text = String::from(
            "# Generated by VibeLauncher so links reach the right profile.\n# Remove it (or turn routing off in VibeLauncher) to restore the original.\n",
        );
        text.push_str(&entry.to_text());
        text
    }

    pub fn status(
        provider: ProviderKind,
        install: Option<&AppInstall>,
        _paths: &Paths,
    ) -> SchemeStatus {
        let mut st = SchemeStatus {
            provider,
            strategy: "shadow-desktop-entry",
            supported: false,
            experimental: false,
            claimed: false,
            stale: false,
            conflict: None,
            handler: query_default(provider),
        };
        let Some(id) = target_id(provider, install) else {
            return st;
        };
        let Some(src) = system_file(&id) else {
            return st;
        };
        st.supported = true;
        let shadow = shadow_path(&id);
        if let Some(e) = de::read(&shadow) {
            if e.main("X-VibeLauncher").is_some() {
                st.claimed = true;
                let src_text = std::fs::read_to_string(&src).unwrap_or_default();
                let self_exe = crate::paths::self_exe();
                let ok_hash = e.main("X-VibeLauncher-Source-Hash").as_deref()
                    == Some(hash(&src_text).as_str());
                let ok_exe = e
                    .main("Exec")
                    .is_some_and(|x| x.contains(&*self_exe.to_string_lossy()));
                st.stale = !(ok_hash && ok_exe);
            } else {
                st.conflict = Some(format!(
                    "{} is a custom entry not made by VibeLauncher",
                    shadow.display()
                ));
            }
        }
        if let Some(h) = &st.handler {
            if *h != id && st.claimed {
                st.conflict = Some(format!("the default handler is {h}, not {id}"));
            }
        }
        st
    }

    pub fn claim(provider: ProviderKind, install: &AppInstall, _paths: &Paths) -> Result<()> {
        let Some(id) = target_id(provider, Some(install)) else {
            bail!(
                "no desktop entry found for {}; is it installed as a package?",
                provider.display_name()
            )
        };
        let src = system_file(&id).context("the app's desktop entry disappeared")?;
        let shadow = shadow_path(&id);
        if let Some(e) = de::read(&shadow) {
            if e.main("X-VibeLauncher").is_none() {
                bail!(
                    "{} already exists and was not made by VibeLauncher; not overwriting it",
                    shadow.display()
                );
            }
        }
        let text = std::fs::read_to_string(&src)?;
        let out = render_shadow(
            provider,
            &text,
            &src.to_string_lossy(),
            &crate::paths::self_exe().to_string_lossy(),
        );
        crate::paths::write_atomic(&shadow, out.as_bytes())?;
        refresh_db();
        // Make sure the scheme still points at the app's id (normally it already does).
        let mime = format!("x-scheme-handler/{}", provider.schemes()[0]);
        match query_default(provider) {
            Some(h) if h == id => {}
            Some(h) if !h.is_empty() && !h.starts_with("vibelauncher") => {}
            _ => {
                let _ = std::process::Command::new("xdg-mime")
                    .args(["default", &id, &mime])
                    .status();
            }
        }
        Ok(())
    }

    pub fn release(
        provider: ProviderKind,
        install: Option<&AppInstall>,
        _paths: &Paths,
    ) -> Result<()> {
        let Some(id) = target_id(provider, install) else {
            return Ok(());
        };
        let shadow = shadow_path(&id);
        if de::read(&shadow).is_some_and(|e| e.main("X-VibeLauncher").is_some()) {
            std::fs::remove_file(&shadow)?;
            refresh_db();
        }
        Ok(())
    }

    pub fn reassert(_: ProviderKind, _: &AppInstall, _: &Paths) -> Result<()> {
        Ok(())
    }

    fn refresh_db() {
        let dir = de::user_applications_dir();
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    pub fn query_default(provider: ProviderKind) -> Option<String> {
        let out = std::process::Command::new("xdg-mime")
            .args([
                "query",
                "default",
                &format!("x-scheme-handler/{}", provider.schemes()[0]),
            ])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn shadow_keeps_actions_and_falls_back() {
            let src = "[Desktop Entry]\nName=Claude\nExec=claude-desktop %U\nTryExec=claude-desktop\nDBusActivatable=true\nMimeType=x-scheme-handler/claude;\nActions=NewChat;\n\n[Desktop Action NewChat]\nName=New Chat\nExec=claude-desktop \"claude://claude.ai/new?surface=chat\"\n";
            let out = render_shadow(
                ProviderKind::Claude,
                src,
                "/usr/share/applications/x.desktop",
                "/opt/Vibe Launcher/vibelauncher",
            );
            let e = DesktopEntry::parse(&out);
            let main = e.exec_args("Desktop Entry").unwrap();
            assert_eq!(main[0], "/bin/sh");
            assert_eq!(main[3], "/opt/Vibe Launcher/vibelauncher");
            assert_eq!(main[4], "%U");
            assert!(main[2].contains("open-url --provider claude"));
            assert!(main[2].contains("then exit 0; fi; exec 'claude-desktop' \"$@\""));
            let act = e.exec_args("Desktop Action NewChat").unwrap();
            assert_eq!(act[4], "claude://claude.ai/new?surface=chat");
            assert!(e.main("TryExec").is_none());
            assert!(e.main("DBusActivatable").is_none());
            assert_eq!(e.main("Name").as_deref(), Some("Claude"));
            assert_eq!(e.main("X-VibeLauncher").as_deref(), Some("true"));
        }

        #[test]
        fn discovery_ignores_our_shadow() {
            let dir = tempfile::tempdir().unwrap();
            let user = dir.path().join("user");
            let sys = dir.path().join("sys");
            std::fs::create_dir_all(user.join("applications")).unwrap();
            std::fs::create_dir_all(sys.join("applications")).unwrap();
            let bin = dir.path().join("claude-desktop");
            std::fs::write(&bin, "#!/bin/sh\n").unwrap();
            let src = format!(
                "[Desktop Entry]\nName=Claude\nExec={} %U\nMimeType=x-scheme-handler/claude;\n",
                bin.display()
            );
            std::fs::write(sys.join("applications/com.anthropic.Claude.desktop"), &src).unwrap();
            let shadow = render_shadow(ProviderKind::Claude, &src, "s", "/vl");
            std::fs::write(
                user.join("applications/com.anthropic.Claude.desktop"),
                shadow,
            )
            .unwrap();
            std::env::set_var("XDG_DATA_HOME", &user);
            std::env::set_var("XDG_DATA_DIRS", &sys);
            let found = crate::install::find_installs(ProviderKind::Claude, None);
            assert!(!found.is_empty());
            assert_eq!(found[0].program, bin);
            let shadow_file = user.join("applications/com.anthropic.Claude.desktop");
            assert!(
                found
                    .iter()
                    .all(|i| i.program != std::path::Path::new("/bin/sh")
                        && i.desktop_file.as_ref() != Some(&shadow_file)),
                "{found:?}"
            );
            assert_eq!(
                found[0].desktop_id.as_deref(),
                Some("com.anthropic.Claude.desktop")
            );
        }

        #[test]
        fn shadow_keeps_flatpak_prefix() {
            let src = "[Desktop Entry]\nName=Claude\nExec=/usr/bin/flatpak run --branch=stable com.example.Claude %U\n";
            let out = render_shadow(ProviderKind::Claude, src, "s", "/vl");
            let e = DesktopEntry::parse(&out);
            let main = e.exec_args("Desktop Entry").unwrap();
            assert!(main[2].contains(
                "exec '/usr/bin/flatpak' 'run' '--branch=stable' 'com.example.Claude' \"$@\""
            ));
            assert_eq!(&main[4..], &["%U".to_string()]);
        }
    }
}

// ───────────────────────────── Windows ─────────────────────────────

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    fn key_path(scheme: &str) -> String {
        format!("Software\\Classes\\{scheme}\\shell\\open\\command")
    }

    fn our_command(provider: ProviderKind) -> String {
        format!(
            "\"{}\" open-url --provider {} -- \"%1\"",
            crate::paths::self_exe().display(),
            provider.id()
        )
    }

    fn backup_file(paths: &Paths, scheme: &str) -> PathBuf {
        paths.state_dir.join(format!("scheme-backup-{scheme}.txt"))
    }

    fn read_command(scheme: &str) -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(key_path(scheme))
            .ok()?
            .get_value::<String, _>("")
            .ok()
    }

    pub fn status(
        provider: ProviderKind,
        _install: Option<&AppInstall>,
        _paths: &Paths,
    ) -> SchemeStatus {
        let scheme = provider.schemes()[0];
        let cmd = read_command(scheme);
        let claimed = cmd
            .as_deref()
            .is_some_and(|c| c.to_lowercase().contains("vibelauncher"));
        SchemeStatus {
            provider,
            strategy: "registry",
            supported: true,
            experimental: true,
            claimed,
            stale: claimed && cmd.as_deref() != Some(our_command(provider).as_str()),
            conflict: None,
            handler: cmd,
        }
    }

    pub fn claim(provider: ProviderKind, _install: &AppInstall, paths: &Paths) -> Result<()> {
        let scheme = provider.schemes()[0];
        if let Some(prev) = read_command(scheme) {
            if !prev.to_lowercase().contains("vibelauncher") {
                crate::paths::write_atomic(&backup_file(paths, scheme), prev.as_bytes())?;
            }
        }
        write(provider)
    }

    fn write(provider: ProviderKind) -> Result<()> {
        let scheme = provider.schemes()[0];
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (root, _) = hkcu.create_subkey(format!("Software\\Classes\\{scheme}"))?;
        root.set_value("", &format!("URL:{} Protocol", provider.display_name()))?;
        root.set_value("URL Protocol", &"")?;
        let (cmd, _) = hkcu.create_subkey(key_path(scheme))?;
        cmd.set_value("", &our_command(provider))?;
        Ok(())
    }

    pub fn release(
        provider: ProviderKind,
        _install: Option<&AppInstall>,
        paths: &Paths,
    ) -> Result<()> {
        let scheme = provider.schemes()[0];
        let backup = backup_file(paths, scheme);
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        match std::fs::read_to_string(&backup) {
            Ok(prev) => {
                let (cmd, _) = hkcu.create_subkey(key_path(scheme))?;
                cmd.set_value("", &prev)?;
                let _ = std::fs::remove_file(backup);
            }
            Err(_) => {
                if read_command(scheme).is_some_and(|c| c.to_lowercase().contains("vibelauncher")) {
                    let _ = hkcu.delete_subkey_all(format!("Software\\Classes\\{scheme}"));
                }
            }
        }
        Ok(())
    }

    pub fn reassert(provider: ProviderKind, _install: &AppInstall, paths: &Paths) -> Result<()> {
        let scheme = provider.schemes()[0];
        if let Some(prev) = read_command(scheme) {
            if !prev.to_lowercase().contains("vibelauncher") {
                crate::paths::write_atomic(&backup_file(paths, scheme), prev.as_bytes())?;
            }
        }
        write(provider)
    }
}

/// The executable a scheme's registered command points at (Windows).
#[cfg(target_os = "windows")]
pub fn windows_registered_exe(scheme: &str) -> Option<PathBuf> {
    use winreg::enums::HKEY_CLASSES_ROOT;
    let cmd: String = winreg::RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey(format!("{scheme}\\shell\\open\\command"))
        .ok()?
        .get_value("")
        .ok()?;
    let cmd = cmd.trim();
    let exe = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        cmd.split_whitespace().next()?
    };
    Some(PathBuf::from(exe))
}

// ───────────────────────────── macOS ─────────────────────────────

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};
    use std::process::Command;

    pub const ROUTER_BUNDLE_ID: &str = "io.github.gloridust.vibelauncher.router";

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSSetDefaultHandlerForURLScheme(scheme: CFStringRef, bundle_id: CFStringRef) -> i32;
        fn LSCopyDefaultHandlerForURLScheme(scheme: CFStringRef) -> CFStringRef;
    }

    fn router_app(paths: &Paths) -> PathBuf {
        paths.state_dir.join("VibeLauncher Router.app")
    }

    fn backup_file(paths: &Paths, scheme: &str) -> PathBuf {
        paths.state_dir.join(format!("scheme-backup-{scheme}.txt"))
    }

    pub fn current_handler(scheme: &str) -> Option<String> {
        unsafe {
            let r = LSCopyDefaultHandlerForURLScheme(CFString::new(scheme).as_concrete_TypeRef());
            if r.is_null() {
                return None;
            }
            Some(CFString::wrap_under_create_rule(r).to_string())
        }
    }

    fn set_handler(scheme: &str, bundle_id: &str) -> Result<()> {
        let rc = unsafe {
            LSSetDefaultHandlerForURLScheme(
                CFString::new(scheme).as_concrete_TypeRef(),
                CFString::new(bundle_id).as_concrete_TypeRef(),
            )
        };
        if rc != 0 {
            bail!("Launch Services refused the handler change (error {rc})");
        }
        Ok(())
    }

    fn applescript_quote(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }

    /// A tiny AppleScript applet: macOS hands URLs to apps as Apple Events,
    /// which `on open location` receives.
    const LSREGISTER: &str =
        "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

    fn is_router(h: Option<&str>) -> bool {
        h.is_some_and(|h| h.eq_ignore_ascii_case(ROUTER_BUNDLE_ID))
    }

    /// The schemes the router app was built to declare: what has been
    /// claimed, independent of which app Launch Services prefers right now
    /// (the apps take their scheme back when they start).
    fn declared_schemes(paths: &Paths) -> Vec<String> {
        let plist = router_app(paths).join("Contents/Info.plist");
        if !plist.exists() {
            return Vec::new();
        }
        Command::new("/usr/bin/plutil")
            .args([
                "-extract",
                "CFBundleURLTypes.0.CFBundleURLSchemes",
                "json",
                "-o",
                "-",
            ])
            .arg(&plist)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<Vec<String>>(&o.stdout).ok())
            .unwrap_or_default()
    }

    fn with_scheme(mut list: Vec<String>, scheme: &str) -> Vec<String> {
        if !list.iter().any(|s| s == scheme) {
            list.push(scheme.to_string());
        }
        list
    }

    /// Build the router declaring exactly `schemes`, so a scheme we let go of
    /// isn't still claimed by it.
    fn build_router(paths: &Paths, schemes: &[String]) -> Result<PathBuf> {
        let app = router_app(paths);
        let exe = crate::paths::self_exe();
        let script = format!(
            "on open location theURL\n  do shell script quoted form of {exe} & \" open-url -- \" & quoted form of theURL & \" > /dev/null 2>&1 &\"\nend open location\non run\n  do shell script quoted form of {exe} & \" > /dev/null 2>&1 &\"\nend run\n",
            exe = applescript_quote(&exe.to_string_lossy())
        );
        let src = paths.state_dir.join("router.applescript");
        crate::paths::write_atomic(&src, script.as_bytes())?;
        let _ = std::fs::remove_dir_all(&app);
        let ok = Command::new("/usr/bin/osacompile")
            .arg("-o")
            .arg(&app)
            .arg(&src)
            .status()?
            .success();
        if !ok {
            bail!("osacompile failed");
        }
        let plist = app.join("Contents/Info.plist");
        let plutil = |args: &[&str]| {
            Command::new("/usr/bin/plutil")
                .args(args)
                .arg(&plist)
                .status()
        };
        plutil(&[
            "-replace",
            "CFBundleIdentifier",
            "-string",
            ROUTER_BUNDLE_ID,
        ])?;
        plutil(&["-replace", "CFBundleName", "-string", "VibeLauncher Router"])?;
        plutil(&["-replace", "LSUIElement", "-bool", "YES"])?;
        let types = serde_json::json!([{ "CFBundleURLName": "VibeLauncher", "CFBundleURLSchemes": schemes }]).to_string();
        plutil(&["-replace", "CFBundleURLTypes", "-json", &types])?;
        let _ = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&app)
            .status();
        let _ = Command::new(LSREGISTER).arg("-f").arg(&app).status();
        Ok(app)
    }

    fn remove_router(paths: &Paths) {
        let app = router_app(paths);
        if app.exists() {
            let _ = Command::new(LSREGISTER).arg("-u").arg(&app).status();
            let _ = std::fs::remove_dir_all(&app);
        }
    }

    pub fn status(
        provider: ProviderKind,
        _install: Option<&AppInstall>,
        paths: &Paths,
    ) -> SchemeStatus {
        let scheme = provider.schemes()[0];
        let handler = current_handler(scheme);
        let claimed = handler
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(ROUTER_BUNDLE_ID));
        SchemeStatus {
            provider,
            strategy: "launch-services",
            supported: true,
            experimental: true,
            claimed,
            stale: claimed && !router_app(paths).exists(),
            conflict: None,
            handler,
        }
    }

    pub fn claim(provider: ProviderKind, _install: &AppInstall, paths: &Paths) -> Result<()> {
        let scheme = provider.schemes()[0];
        if let Some(prev) = current_handler(scheme) {
            if !prev.eq_ignore_ascii_case(ROUTER_BUNDLE_ID) {
                crate::paths::write_atomic(&backup_file(paths, scheme), prev.as_bytes())?;
            }
        }
        build_router(paths, &with_scheme(declared_schemes(paths), scheme))?;
        set_handler(scheme, ROUTER_BUNDLE_ID)
    }

    pub fn release(
        provider: ProviderKind,
        install: Option<&AppInstall>,
        paths: &Paths,
    ) -> Result<()> {
        let scheme = provider.schemes()[0];
        let prev = std::fs::read_to_string(backup_file(paths, scheme))
            .ok()
            .or_else(|| {
                install
                    .and_then(|i| i.bundle.as_ref())
                    .and_then(|b| crate::install::macos::plist_value(b, "CFBundleIdentifier"))
            });
        // Stop declaring the scheme first: unregistering the router can make
        // Launch Services forget the scheme's default, which would undo a
        // handler restored before it.
        let declared = declared_schemes(paths);
        let rest: Vec<String> = declared.iter().filter(|s| *s != scheme).cloned().collect();
        if rest.is_empty() {
            remove_router(paths);
        } else if rest.len() != declared.len() {
            build_router(paths, &rest)?;
        }
        if let Some(prev) = prev.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            set_handler(scheme, prev)
                .with_context(|| format!("giving {scheme}:// links back to {prev}"))?;
        }
        let _ = std::fs::remove_file(backup_file(paths, scheme));
        if router_app(paths).exists() && is_router(current_handler(scheme).as_deref()) {
            bail!("macOS still sends {scheme}:// links to VibeLauncher; choose another app for them in that app's settings");
        }
        Ok(())
    }

    pub fn reassert(provider: ProviderKind, _install: &AppInstall, paths: &Paths) -> Result<()> {
        let scheme = provider.schemes()[0];
        let declared = declared_schemes(paths);
        if !router_app(paths).exists() || !declared.iter().any(|s| s == scheme) {
            build_router(paths, &with_scheme(declared, scheme))?;
        }
        if let Some(prev) = current_handler(scheme) {
            if !prev.eq_ignore_ascii_case(ROUTER_BUNDLE_ID) {
                crate::paths::write_atomic(&backup_file(paths, scheme), prev.as_bytes())?;
            }
        }
        set_handler(scheme, ROUTER_BUNDLE_ID)
    }
}
