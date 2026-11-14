//! Which profiles are running right now.
//!
//! A process belongs to a profile if its command line carries that profile's
//! `--user-data-dir` (readable on every platform). Environment variables are
//! only used by `doctor`, to spot instances that lost their isolation after an
//! update relaunch.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

use crate::install::AppInstall;
use crate::model::Config;
use crate::paths::Paths;
use crate::provider::{profile_user_data_dir, ProviderKind};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningApp {
    pub pid: u32,
    pub provider: ProviderKind,
    /// `None` when the user-data-dir doesn't belong to any profile.
    pub profile_id: Option<String>,
    pub user_data_dir: Option<PathBuf>,
    /// Seconds since the Unix epoch.
    pub start_time: u64,
    /// Environment of the process, when readable (same user).
    #[serde(skip)]
    pub environ: Option<HashMap<String, String>>,
    /// Whether `environ` could be read (shown by `vibelauncher ps`).
    pub env_readable: bool,
}

pub struct Snapshot {
    pub apps: Vec<RunningApp>,
}

impl Snapshot {
    pub fn take(cfg: &Config, paths: &Paths, installs: &[AppInstall]) -> Self {
        let mut sys = System::new_with_specifics(RefreshKind::nothing());
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            // Threads are listed as processes by default; we only want processes.
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cmd(UpdateKind::Always)
                .with_exe(UpdateKind::Always),
        );
        // Environments are big; read them only for the apps' own processes
        // and their direct children (the fallback below needs those).
        let mains: HashSet<sysinfo::Pid> = sys
            .processes()
            .iter()
            .filter(|(_, p)| p.thread_kind().is_none())
            .filter(|(_, p)| {
                let cmd: Vec<String> = p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect();
                classify(p.exe(), &normalize_cmd(cmd), installs).is_some()
            })
            .map(|(pid, _)| *pid)
            .collect();
        let wanted: Vec<sysinfo::Pid> = sys
            .processes()
            .iter()
            .filter(|(pid, p)| {
                mains.contains(pid)
                    || p.parent().is_some_and(|pp| mains.contains(&pp))
                    || p.cmd().iter().any(|a| a.to_string_lossy().contains("Crashpad"))
            })
            .map(|(pid, _)| *pid)
            .collect();
        if !wanted.is_empty() {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&wanted),
                false,
                ProcessRefreshKind::nothing().without_tasks().with_environ(UpdateKind::Always),
            );
        }
        // Every process once, with a normalized command line.
        struct Info {
            parent: Option<u32>,
            cmd: Vec<String>,
            env: Option<HashMap<String, String>>,
        }
        let mut all: HashMap<u32, Info> = HashMap::new();
        for (pid, p) in sys.processes() {
            if p.thread_kind().is_some() {
                continue;
            }
            all.insert(
                pid.as_u32(),
                Info {
                    parent: p.parent().map(|x| x.as_u32()),
                    cmd: normalize_cmd(p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect()),
                    env: parse_environ(p.environ()),
                },
            );
        }
        if std::env::var_os("VIBELAUNCHER_DEBUG_PROCS").is_some() {
            debug_dump(&sys, installs);
        }
        let mut apps = Vec::new();
        for (pid, p) in sys.processes() {
            if p.thread_kind().is_some() {
                continue;
            }
            let Some(info) = all.get(&pid.as_u32()) else { continue };
            if info.cmd.iter().any(|a| a.starts_with("--type=")) {
                continue; // Chromium helper (renderer, GPU, zygote, …)
            }
            let exe = p.exe().map(Path::to_path_buf);
            let Some(provider) = classify(exe.as_deref(), &info.cmd, installs) else { continue };
            // A helper re-executing the main binary without --type= (rare)
            // has a parent of the same app; skip it.
            if let Some(parent) = info.parent.and_then(|pp| sys.process(sysinfo::Pid::from_u32(pp))) {
                if parent.exe().is_some() && parent.exe() == p.exe() {
                    continue;
                }
            }
            let udd = user_data_dir_arg(&info.cmd);
            let profile_id = match &udd {
                Some(dir) => cfg
                    .profiles
                    .iter()
                    .find(|pr| {
                        pr.provider == provider
                            && profile_user_data_dir(pr, paths).is_some_and(|d| same(&d, dir))
                    })
                    .map(|pr| pr.id.clone()),
                None => Some(provider.default_profile_id().to_string()),
            };
            // Electron overwrites its own environment block when it sets the
            // process title, so fall back to a child that inherited it
            // (crashpad handler, Code-tab helpers, …).
            let my = pid.as_u32();
            let environ = info.env.clone().or_else(|| {
                let crash_dir = udd.as_ref().map(|d| d.join("Crashpad").to_string_lossy().into_owned());
                all.iter()
                    .filter(|(_, i)| {
                        i.parent == Some(my)
                            || crash_dir.as_ref().is_some_and(|c| i.cmd.iter().any(|a| a.contains(c.as_str())))
                    })
                    .find_map(|(_, i)| i.env.clone())
            });
            apps.push(RunningApp {
                pid: my,
                provider,
                profile_id,
                user_data_dir: udd,
                start_time: p.start_time(),
                env_readable: environ.is_some(),
                environ,
            });
        }
        apps.sort_by_key(|a| a.start_time);
        Self { apps }
    }

    pub fn for_profile(&self, id: &str) -> Vec<&RunningApp> {
        self.apps.iter().filter(|a| a.profile_id.as_deref() == Some(id)).collect()
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.apps.iter().any(|a| a.profile_id.as_deref() == Some(id))
    }

    pub fn running_ids(&self, provider: ProviderKind) -> Vec<String> {
        let mut v: Vec<String> = self
            .apps
            .iter()
            .filter(|a| a.provider == provider)
            .filter_map(|a| a.profile_id.clone())
            .collect();
        v.dedup();
        v
    }
}

/// `VIBELAUNCHER_DEBUG_PROCS=1 vibelauncher ps`: what the scan sees for every
/// process that looks like one of the apps, and why it was (not) counted.
fn debug_dump(sys: &System, installs: &[AppInstall]) {
    for i in installs {
        eprintln!("install {:?}: program {:?}, matches {:?}", i.provider, i.program, i.match_exes);
    }
    let names: Vec<String> = ProviderKind::ALL
        .iter()
        .flat_map(|p| p.process_names().iter().map(|n| n.trim_end_matches(".exe").to_string()))
        .collect();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_lowercase();
        if !names.iter().any(|n| name.contains(n.as_str())) {
            continue;
        }
        let cmd: Vec<String> = p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect();
        let canon = p.exe().map(|e| e.canonicalize());
        eprintln!(
            "pid {pid} name {name:?} parent {:?} thread {:?}\n  exe {:?} canonical {canon:?}\n  cmd {cmd:?}\n  class {:?} environ {}",
            p.parent(),
            p.thread_kind(),
            p.exe(),
            classify(p.exe(), &normalize_cmd(cmd.clone()), installs),
            p.environ().len(),
        );
    }
}

fn classify(exe: Option<&Path>, cmd: &[String], installs: &[AppInstall]) -> Option<ProviderKind> {
    let exe_c = exe.and_then(|e| e.canonicalize().ok()).or_else(|| exe.map(Path::to_path_buf));
    for i in installs {
        if let Some(e) = &exe_c {
            if i.match_exes.iter().any(|m| same(m, e)) {
                return Some(i.provider);
            }
        }
    }
    // Fallback by name, but only for exact executable names (avoid matching
    // e.g. the `claude` CLI or our own shortcuts).
    let name = exe
        .and_then(|e| e.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .or_else(|| cmd.first().map(|c| Path::new(c).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()))?;
    // `claude` alone is the Claude Code CLI on Linux; require the desktop name there.
    for p in ProviderKind::ALL {
        let names = p.process_names();
        let hit = names.iter().any(|n| *n == name);
        if hit && !(cfg!(target_os = "linux") && name == "claude") {
            // Electron main processes always have a resources dir next to them.
            let looks_electron = exe
                .and_then(|e| e.parent())
                .is_some_and(|d| d.join("resources").is_dir() || d.ends_with("MacOS"));
            if looks_electron {
                return Some(p);
            }
        }
    }
    None
}

/// Chromium rewrites its process title, which turns `/proc/<pid>/cmdline`
/// into a single space-joined string. Split it back at ` --` boundaries (an
/// executable path may itself contain spaces, flags never start mid-word).
pub fn normalize_cmd(cmd: Vec<String>) -> Vec<String> {
    if cmd.len() != 1 || !cmd[0].contains(" --") {
        return cmd;
    }
    let s = &cmd[0];
    let mut out = Vec::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find(" --") {
        out.push(rest[..i].to_string());
        rest = &rest[i + 1..];
    }
    out.push(rest.to_string());
    out
}

/// Parse an environment block; `None` if it doesn't look like one. When
/// Chromium sets its process title it overwrites the start of its own
/// environment block, so the first entry must be a real variable.
fn parse_environ(env: &[std::ffi::OsString]) -> Option<HashMap<String, String>> {
    let valid_key = |k: &str| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    // Windows starts the block with hidden per-drive entries (`=C:=C:\…`).
    let first = env.iter().find(|kv| !kv.to_string_lossy().starts_with('='))?.to_string_lossy();
    if !first.split_once('=').is_some_and(|(k, _)| valid_key(k)) {
        return None;
    }
    let m: HashMap<String, String> = env
        .iter()
        .filter_map(|kv| {
            let s = kv.to_string_lossy();
            let (k, v) = s.split_once('=')?;
            valid_key(k).then(|| (k.to_string(), v.to_string()))
        })
        .collect();
    // Windows spells it `Path`.
    m.keys().any(|k| k.eq_ignore_ascii_case("PATH") || k == "HOME").then_some(m)
}

/// The value of `--user-data-dir` (either `--user-data-dir=X` or `--user-data-dir X`).
pub fn user_data_dir_arg(cmd: &[String]) -> Option<PathBuf> {
    let mut it = cmd.iter();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix("--user-data-dir=") {
            return Some(PathBuf::from(v.trim_matches('"')));
        }
        if a == "--user-data-dir" {
            return it.next().map(PathBuf::from);
        }
    }
    None
}

fn same(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Ask a process to quit (SIGTERM / WM_CLOSE-equivalent).
pub fn terminate(pid: u32) -> bool {
    #[cfg(windows)]
    {
        // Windows has no SIGTERM. `taskkill` without /F asks the app's
        // windows to close, as clicking their close buttons would; force it
        // only if it is still running a few seconds later.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let spid = sysinfo::Pid::from_u32(pid);
        let mut sys = System::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            sys.refresh_processes(ProcessesToUpdate::Some(&[spid]), true);
            match sys.process(spid) {
                None => return true,
                Some(p) if std::time::Instant::now() >= deadline => return p.kill(),
                Some(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
            }
        }
    }
    #[allow(unreachable_code)]
    let mut sys = System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    match sys.process(pid) {
        Some(p) => p.kill_with(sysinfo::Signal::Term).unwrap_or_else(|| p.kill()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_environment_block() {
        let env: Vec<std::ffi::OsString> = ["=C:=C:\\work", "=ExitCode=00000000", "ALLUSERSPROFILE=C:\\ProgramData", "Path=C:\\Windows", "CLAUDE_CONFIG_DIR=C:\\p\\claude-config"]
            .iter()
            .map(Into::into)
            .collect();
        let m = parse_environ(&env).expect("a Windows block is readable");
        assert_eq!(m.get("CLAUDE_CONFIG_DIR").map(String::as_str), Some("C:\\p\\claude-config"));
        // A block whose start was overwritten (Chromium's process title) still isn't.
        let bad: Vec<std::ffi::OsString> = ["claude --type=renderer", "PATH=/usr/bin"].iter().map(Into::into).collect();
        assert!(parse_environ(&bad).is_none());
    }

    #[test]
    fn udd_forms() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(user_data_dir_arg(&v(&["x", "--user-data-dir=/a b"])), Some("/a b".into()));
        assert_eq!(user_data_dir_arg(&v(&["x", "--user-data-dir", "/c"])), Some("/c".into()));
        assert_eq!(user_data_dir_arg(&v(&["x"])), None);
        let joined = normalize_cmd(v(&["/opt/My App/claude --user-data-dir=/a b/electron --type=gpu claude://x"]));
        assert_eq!(joined[0], "/opt/My App/claude");
        assert_eq!(user_data_dir_arg(&joined), Some("/a b/electron".into()));
        assert!(joined.iter().any(|a| a.starts_with("--type=")));
    }
}
