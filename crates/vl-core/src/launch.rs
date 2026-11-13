//! Starting an app process detached from VibeLauncher.

use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::paths::create_private_dir;
use crate::provider::LaunchSpec;

/// Start the app described by `spec`. Returns once the process is spawned;
/// VibeLauncher never waits for the app to exit.
pub fn spawn(spec: &LaunchSpec) -> Result<()> {
    for d in &spec.mkdirs {
        create_private_dir(d)?;
    }
    let mut cmd = command_for(spec);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().with_context(|| format!("starting {}", spec.program.display()))?;
    // Reap the child when it exits so a long-running GUI doesn't collect
    // zombies. The thread just blocks; it costs nothing.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Like [`spawn`] but waits for the process to exit (used when handing a URL
/// to an already-running instance: the second process exits within seconds).
pub fn run_and_wait(spec: &LaunchSpec, timeout: std::time::Duration) -> Result<bool> {
    for d in &spec.mkdirs {
        create_private_dir(d)?;
    }
    let mut cmd = command_for(spec);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().with_context(|| format!("starting {}", spec.program.display()))?;
    let start = std::time::Instant::now();
    loop {
        if let Some(st) = child.try_wait()? {
            return Ok(st.success());
        }
        if start.elapsed() > timeout {
            // It became the primary instance (the target was not running);
            // leave it running.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Human-readable form of the command, for logs and the "copy command" button.
pub fn describe(spec: &LaunchSpec) -> String {
    let mut parts: Vec<String> = spec.set_env.iter().map(|(k, v)| format!("{k}={}", shell_quote(v))).collect();
    parts.push(shell_quote(&spec.program.to_string_lossy()));
    parts.extend(spec.args.iter().map(|a| shell_quote(&a.to_string_lossy())));
    parts.join(" ")
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(target_os = "macos")]
fn command_for(spec: &LaunchSpec) -> Command {
    if let Some(bundle) = &spec.bundle {
        // `open -n` starts a new instance through Launch Services, so the app
        // gets a normal Dock icon and isn't our child. `--env` passes
        // variables through (launchd would otherwise drop ours).
        let mut c = Command::new("/usr/bin/open");
        c.arg("-n").arg("-a").arg(bundle);
        for (k, v) in &spec.set_env {
            c.arg("--env").arg(format!("{k}={v}"));
        }
        if !spec.args.is_empty() {
            c.arg("--args").args(&spec.args);
        }
        return c;
    }
    direct_command(spec)
}

#[cfg(not(target_os = "macos"))]
fn command_for(spec: &LaunchSpec) -> Command {
    direct_command(spec)
}

fn direct_command(spec: &LaunchSpec) -> Command {
    let mut c = Command::new(&spec.program);
    c.args(&spec.args);
    for k in &spec.remove_env {
        c.env_remove(k);
    }
    scrub_bundle_env(&mut c);
    for (k, v) in &spec.set_env {
        c.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New session: the app survives VibeLauncher exiting or its terminal
        // closing, and gets no controlling terminal.
        unsafe {
            c.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    c
}

/// When VibeLauncher itself runs as an AppImage, its loader variables would
/// make the official app load our bundled libraries. Remove them.
fn scrub_bundle_env(c: &mut Command) {
    let Some(appdir) = std::env::var_os("APPDIR") else { return };
    let appdir = appdir.to_string_lossy().into_owned();
    for k in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
        c.env_remove(k);
    }
    for k in ["LD_LIBRARY_PATH", "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0", "GIO_EXTRA_MODULES", "GDK_PIXBUF_MODULE_FILE", "GSETTINGS_SCHEMA_DIR", "PYTHONPATH", "PERLLIB", "QT_PLUGIN_PATH", "XDG_DATA_DIRS", "PATH"] {
        if let Ok(v) = std::env::var(k) {
            let kept: Vec<&str> = v.split(':').filter(|p| !p.is_empty() && !p.starts_with(&appdir)).collect();
            if kept.is_empty() {
                c.env_remove(k);
            } else {
                c.env(k, kept.join(":"));
            }
        }
    }
}
