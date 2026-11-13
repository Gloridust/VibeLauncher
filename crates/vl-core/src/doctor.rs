//! Diagnostics: find the things that quietly break multi-profile setups and
//! offer one-click fixes.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::paths::Paths;
use crate::provider::ProviderKind;
use crate::{scheme, shortcuts, Launcher};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Fix {
    /// Take over the URL scheme again.
    ClaimScheme { provider: ProviderKind },
    /// Recreate / remove shortcuts to match the profiles.
    SyncShortcuts,
    /// Quit these profiles and start them again one by one.
    Restart { profiles: Vec<String> },
    /// The app wasn't found: let the user pick it.
    ChooseApp { provider: ProviderKind },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: String,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub fix: Option<Fix>,
}

impl Check {
    fn new(id: impl Into<String>, level: Level, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { id: id.into(), level, title: title.into(), detail: detail.into(), fix: None }
    }
    fn fix(mut self, f: Fix) -> Self {
        self.fix = Some(f);
        self
    }
}

fn versions_file(paths: &Paths) -> PathBuf {
    paths.state_dir.join("seen-versions.json")
}

fn load_versions(paths: &Paths) -> BTreeMap<String, String> {
    std::fs::read(versions_file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Remember the app version VibeLauncher last launched, to notice updates.
pub fn remember_version(paths: &Paths, provider: ProviderKind, v: &str) {
    let mut m = load_versions(paths);
    if m.get(provider.id()).map(String::as_str) != Some(v) {
        m.insert(provider.id().into(), v.into());
        if let Ok(b) = serde_json::to_vec_pretty(&m) {
            let _ = crate::paths::write_atomic(&versions_file(paths), &b);
        }
    }
}

pub fn run(l: &mut Launcher) -> Vec<Check> {
    let mut out = Vec::new();
    let snap = l.snapshot();
    let seen = load_versions(&l.paths);
    for provider in ProviderKind::ALL {
        let profiles: Vec<crate::Profile> = l.config.profiles.iter().filter(|p| p.provider == provider).cloned().collect();
        let isolated: Vec<&crate::Profile> = profiles.iter().filter(|p| p.isolated).collect();
        let name = provider.display_name();
        let inst = l.install(provider);

        // 1. Is the app there?
        match &inst {
            None if !isolated.is_empty() => out.push(
                Check::new(
                    format!("{}-missing", provider.id()),
                    Level::Error,
                    format!("{name} wasn't found"),
                    "Your profiles can't start until VibeLauncher knows where the app is. Install it, or choose it by hand.",
                )
                .fix(Fix::ChooseApp { provider }),
            ),
            None => {
                out.push(Check::new(
                    format!("{}-missing", provider.id()),
                    Level::Info,
                    format!("{name} isn't installed"),
                    "Nothing to do unless you want to use it.",
                ));
                continue;
            }
            Some(i) => out.push(Check::new(
                format!("{}-found", provider.id()),
                Level::Ok,
                format!("{name} {}", i.version.as_deref().unwrap_or("")).trim().to_string(),
                format!("{} ({})", i.bundle.as_ref().unwrap_or(&i.program).display(), i.source),
            )),
        }

        // 2. Login links reach VibeLauncher?
        if l.routing_wanted(provider) {
            let st = scheme::status(provider, inst.as_ref(), &l.paths);
            let scheme_name = provider.schemes()[0];
            let id = format!("{}-routing", provider.id());
            if !st.supported {
                out.push(Check::new(
                    id,
                    Level::Warning,
                    format!("{scheme_name}:// links can't be routed"),
                    format!("{name} has no menu entry VibeLauncher can stand in for, so sign-in links open its default profile. Use “Paste link” when logging in to another profile."),
                ));
            } else if !st.claimed || st.stale {
                out.push(
                    Check::new(
                        id,
                        Level::Warning,
                        format!("{scheme_name}:// links aren't going through VibeLauncher"),
                        if st.stale {
                            "The link handler points at an old copy of VibeLauncher or an old version of the app.".to_string()
                        } else {
                            format!("Sign-in links will open whichever {name} window the system picks.")
                        },
                    )
                    .fix(Fix::ClaimScheme { provider }),
                );
            } else if let Some(c) = st.conflict {
                out.push(Check::new(id, Level::Warning, "Another link handler is in the way", c).fix(Fix::ClaimScheme { provider }));
            } else {
                let mut detail = format!("Links open in the profile that asked for them. Current handler: {}.", st.handler.as_deref().unwrap_or("?"));
                if st.experimental {
                    detail.push_str(" (Experimental on this system.)");
                }
                out.push(Check::new(id, Level::Ok, format!("{scheme_name}:// links are routed"), detail));
            }
        }

        // 3. Instances that lost their isolation (update relaunch).
        let mut lost: Vec<String> = Vec::new();
        for p in &isolated {
            let root = p.data_root(&l.paths);
            let expected = provider.isolation_env(&root);
            for app in snap.for_profile(&p.id) {
                let Some(env) = &app.environ else { continue };
                let bad = expected.iter().find(|(k, v)| env.get(k).map(String::as_str) != Some(v.as_str()));
                if let Some((k, _)) = bad {
                    if !lost.contains(&p.id) {
                        lost.push(p.id.clone());
                    }
                    out.push(
                        Check::new(
                            format!("{}-env", p.id),
                            Level::Warning,
                            format!("“{}” is running without its own {k}", p.name),
                            match provider {
                                ProviderKind::Claude => "Usually after an update restarted the app by itself. Its Code tab now reads the shared ~/.claude settings. Restart the profile to fix it.",
                                ProviderKind::ChatGpt => "Usually after an update restarted the app by itself. It now reads the shared ~/.codex data. Restart the profile to fix it.",
                            },
                        )
                        .fix(Fix::Restart { profiles: vec![p.id.clone()] }),
                    );
                }
            }
        }
        for app in snap.apps.iter().filter(|a| a.provider == provider && a.profile_id.is_none()) {
            out.push(Check::new(
                format!("{}-unknown-{}", provider.id(), app.pid),
                Level::Info,
                format!("A {name} window isn't from any profile"),
                format!("Process {} uses {}.", app.pid, app.user_data_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default()),
            ));
        }

        // 4. Updated since the last launch from VibeLauncher.
        if let (Some(v), Some(old)) = (inst.as_ref().and_then(|i| i.version.clone()), seen.get(provider.id())) {
            if &v != old && !isolated.is_empty() {
                let default_running = snap.is_running(provider.default_profile_id());
                let restart: Vec<String> = isolated
                    .iter()
                    .filter(|p| p.last_launched_at.is_some_and(|t| chrono::Utc::now() - t < chrono::Duration::days(7)))
                    .map(|p| p.id.clone())
                    .collect();
                let mut c = Check::new(
                    format!("{}-updated", provider.id()),
                    Level::Info,
                    format!("{name} was updated ({old} → {v})"),
                    if default_running {
                        "If the update reopened windows on its own, they may have come back without their profile. Restart your profiles from VibeLauncher to be sure."
                    } else {
                        "Profiles you open from VibeLauncher get the new version automatically."
                    },
                );
                if default_running && !restart.is_empty() {
                    c = c.fix(Fix::Restart { profiles: restart });
                }
                out.push(c);
            }
        }

        // 5. Several instances started together will also update together.
        let mut starts: Vec<(u64, String)> = isolated
            .iter()
            .flat_map(|p| snap.for_profile(&p.id).into_iter().map(|a| (a.start_time, p.id.clone())))
            .collect();
        starts.sort();
        let together: Vec<String> = starts
            .windows(2)
            .filter(|w| w[1].0.saturating_sub(w[0].0) < 5)
            .flat_map(|w| [w[0].1.clone(), w[1].1.clone()])
            .fold(Vec::new(), |mut v, id| {
                if !v.contains(&id) {
                    v.push(id);
                }
                v
            });
        if together.len() >= 2 && lost.is_empty() {
            out.push(
                Check::new(
                    format!("{}-simultaneous", provider.id()),
                    Level::Info,
                    format!("{} {name} profiles started at the same moment", together.len()),
                    "Instances that start together tend to update and restart together, which is when profiles get mixed up. Restarting them a few seconds apart avoids that.",
                )
                .fix(Fix::Restart { profiles: together }),
            );
        }

        // 6. Known limitation.
        if provider == ProviderKind::Claude && !isolated.is_empty() {
            out.push(Check::new(
                "claude-cowork",
                Level::Info,
                "Cowork is shared between profiles",
                "Claude's Cowork virtual machine is tied to your system account, not to a profile: only one profile at a time can use it.",
            ));
        }
    }

    // 7. Shortcuts.
    let broken: Vec<String> = l
        .config
        .profiles
        .iter()
        .filter(|p| p.shortcut)
        .filter(|p| {
            let s = shortcuts::state(p);
            !s.exists || s.stale
        })
        .map(|p| p.name.clone())
        .collect();
    if !broken.is_empty() {
        out.push(
            Check::new(
                "shortcuts",
                Level::Warning,
                "Some shortcuts are missing or out of date",
                broken.join(", "),
            )
            .fix(Fix::SyncShortcuts),
        );
    }

    // 8. Data folders.
    for p in l.config.profiles.iter().filter(|p| p.isolated) {
        let root = p.data_root(&l.paths);
        if root.exists() && !root.is_dir() {
            out.push(Check::new(
                format!("{}-data", p.id),
                Level::Error,
                format!("“{}” can't use its data folder", p.name),
                format!("{} exists but isn't a folder.", root.display()),
            ));
        }
    }

    out.sort_by(|a, b| b.level.cmp(&a.level));
    out
}

/// Apply a fix. `Restart` blocks for the stagger time between launches.
pub fn apply(l: &mut Launcher, fix: &Fix) -> Result<()> {
    match fix {
        Fix::ClaimScheme { provider } => {
            let inst = l.install(*provider).ok_or_else(|| anyhow::anyhow!("{} isn't installed", provider.display_name()))?;
            scheme::claim(*provider, &inst, &l.paths)
        }
        Fix::SyncShortcuts => {
            let errs = l.sync_shortcuts();
            if errs.is_empty() {
                Ok(())
            } else {
                anyhow::bail!(errs.join("; "))
            }
        }
        Fix::Restart { profiles } => {
            let stagger = Duration::from_secs(l.config.settings.stagger_seconds.min(120) as u64);
            l.restart_staggered(profiles, stagger)
        }
        Fix::ChooseApp { .. } => anyhow::bail!("choose the app in Settings"),
    }
}
