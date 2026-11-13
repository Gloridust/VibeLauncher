//! Which profiles are running right now.
//!
//! A process belongs to a profile if its command line carries that profile's
//! `--user-data-dir` (readable on every platform). Environment variables are
//! only used by `doctor`, to spot instances that lost their isolation after an
//! update relaunch.

use std::collections::HashMap;
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
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .with_exe(UpdateKind::Always)
                .with_environ(UpdateKind::Always),
        );
        let mut apps = Vec::new();
        for (pid, p) in sys.processes() {
            // Skip threads (Linux lists them as tasks).
            if p.thread_kind().is_some() {
                continue;
            }
            let cmd: Vec<String> = p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect();
            if cmd.iter().any(|a| a.starts_with("--type=")) {
                continue; // Chromium helper (renderer, GPU, zygote, …)
            }
            let exe = p.exe().map(Path::to_path_buf);
            let Some(provider) = classify(exe.as_deref(), &cmd, installs) else { continue };
            let udd = user_data_dir_arg(&cmd);
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
            let environ = {
                let env = p.environ();
                (!env.is_empty()).then(|| {
                    env.iter()
                        .filter_map(|kv| {
                            let s = kv.to_string_lossy();
                            s.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))
                        })
                        .collect()
                })
            };
            apps.push(RunningApp {
                pid: pid.as_u32(),
                provider,
                profile_id,
                user_data_dir: udd,
                start_time: p.start_time(),
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
    fn udd_forms() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(user_data_dir_arg(&v(&["x", "--user-data-dir=/a b"])), Some("/a b".into()));
        assert_eq!(user_data_dir_arg(&v(&["x", "--user-data-dir", "/c"])), Some("/c".into()));
        assert_eq!(user_data_dir_arg(&v(&["x"])), None);
    }
}
