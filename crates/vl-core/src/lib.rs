//! VibeLauncher core: profiles, app discovery, launching, callback routing
//! and diagnostics. The GUI and the CLI are thin layers over [`Launcher`].

pub mod desktop_entry;
pub mod detect;
pub mod doctor;
pub mod icon;
pub mod install;
pub mod launch;
pub mod model;
pub mod paths;
pub mod procs;
pub mod provider;
pub mod router;
pub mod scheme;
pub mod shortcuts;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

pub use install::AppInstall;
pub use model::{Config, CustomApp, Profile, Settings};
pub use paths::Paths;
pub use provider::{AppId, ProviderKind};
pub use router::{Decision, Reason};

/// What the UI sends to create or edit a profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub name: String,
    pub provider: AppId,
    pub color: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub data_root: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub shortcut: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteResult {
    pub provider: ProviderKind,
    pub decision: Decision,
    /// The URL with its query removed (safe to show).
    pub redacted: String,
    pub is_login: bool,
}

pub struct Launcher {
    pub paths: Paths,
    pub config: Config,
    installs: BTreeMap<AppId, Option<AppInstall>>,
}

impl Launcher {
    pub fn open() -> Result<Self> {
        Self::with_paths(Paths::detect()?)
    }

    pub fn with_paths(paths: Paths) -> Result<Self> {
        paths.ensure()?;
        let config = Config::load(&paths)?;
        Ok(Self { paths, config, installs: BTreeMap::new() })
    }

    pub fn reload(&mut self) -> Result<()> {
        self.config = Config::load(&self.paths)?;
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.config.save(&self.paths)
    }

    // ── apps ──

    pub fn install(&mut self, app: impl Into<AppId>) -> Option<AppInstall> {
        let app = app.into();
        if let Some(i) = self.installs.get(&app) {
            return i.clone();
        }
        let found = match &app {
            AppId::Builtin(p) => {
                let over = self.config.settings.app_overrides.get(p.id()).cloned();
                install::find_install(*p, over.as_deref())
            }
            AppId::Custom(id) => self.config.custom_app(id).and_then(install::custom_install),
        };
        self.installs.insert(app, found.clone());
        found
    }

    /// Every installation found (the chosen one first), for the settings page.
    pub fn candidates(&self, provider: ProviderKind) -> Vec<AppInstall> {
        let over = self.config.settings.app_overrides.get(provider.id()).cloned();
        install::find_installs(provider, over.as_deref())
    }

    pub fn refresh_installs(&mut self) {
        self.installs.clear();
    }

    pub fn all_installs(&mut self) -> Vec<AppInstall> {
        self.config.app_ids().into_iter().filter_map(|a| self.install(a)).collect()
    }

    pub fn set_app_override(&mut self, provider: ProviderKind, path: Option<PathBuf>) -> Result<()> {
        match path {
            Some(p) => {
                if install::from_manual_path(provider, &p).is_none() {
                    bail!("{} doesn't look like {}", p.display(), provider.display_name());
                }
                self.config.settings.app_overrides.insert(provider.id().into(), p);
            }
            None => {
                self.config.settings.app_overrides.remove(provider.id());
            }
        }
        self.save()?;
        self.installs.remove(&provider.into());
        Ok(())
    }

    /// Check an app the user picked: whether it can run separate profiles,
    /// and that it isn't one of the built-in apps.
    pub fn inspect_app(&mut self, path: &Path) -> std::result::Result<detect::Detected, detect::Refusal> {
        let d = detect::detect(path)?;
        let program = d.program.canonicalize().unwrap_or_else(|_| d.program.clone());
        for p in ProviderKind::ALL {
            if let Some(i) = self.install(p) {
                let mut own = i.match_exes.clone();
                own.extend(i.bundle.clone());
                if own.iter().any(|e| e.canonicalize().unwrap_or_else(|_| e.clone()) == program) {
                    return Err(detect::Refusal::BuiltIn);
                }
            }
        }
        Ok(d)
    }

    /// Add an app checked with [`Launcher::inspect_app`] and return its id;
    /// one that was added before keeps its id.
    pub fn add_app(&mut self, mut d: detect::Detected) -> Result<String> {
        d.program = std::path::absolute(&d.program)?;
        let program = d.program.canonicalize().unwrap_or_else(|_| d.program.clone());
        if let Some(a) = self.config.apps.iter().find(|a| a.program.canonicalize().unwrap_or_else(|_| a.program.clone()) == program) {
            return Ok(a.id.clone());
        }
        let id = self.config.unique_app_id(&d.name);
        let icon = match d.icon_png {
            Some(png) => {
                let f = self.paths.icons_dir().join(format!("app-{id}.png"));
                std::fs::create_dir_all(self.paths.icons_dir())?;
                paths::write_atomic(&f, &png)?;
                Some(f)
            }
            None => d.icon,
        };
        self.config.apps.push(CustomApp {
            id: id.clone(),
            name: d.name.trim().to_string(),
            program: d.program,
            base_args: d.base_args,
            engine: d.engine.as_str().into(),
            icon,
            desktop_file: d.desktop_file,
            added_at: Utc::now(),
        });
        self.config.ensure_defaults();
        self.save()?;
        self.installs.remove(&AppId::Custom(id.clone()));
        Ok(id)
    }

    /// Remove an added app. Its other profiles must be deleted first; its
    /// default profile goes with it.
    pub fn remove_app(&mut self, id: &str) -> Result<()> {
        if self.config.custom_app(id).is_none() {
            bail!("no added app {id:?}");
        }
        let app = AppId::Custom(id.into());
        let name = self.config.app_name(&app);
        if self.config.profiles.iter().any(|p| p.provider == app && p.isolated) {
            bail!("delete {name}'s profiles first");
        }
        for p in self.config.profiles.iter().filter(|p| p.provider == app) {
            let _ = shortcuts::remove(p, &name);
        }
        self.config.profiles.retain(|p| p.provider != app);
        self.config.apps.retain(|a| a.id != id);
        self.save()?;
        self.installs.remove(&app);
        let _ = std::fs::remove_file(self.paths.icons_dir().join(format!("app-{id}.png")));
        Ok(())
    }

    pub fn snapshot(&mut self) -> procs::Snapshot {
        let installs = self.all_installs();
        procs::Snapshot::take(&self.config, &self.paths, &installs)
    }

    // ── launching ──

    fn profile(&self, id: &str) -> Result<Profile> {
        self.config.find(id).cloned().ok_or_else(|| anyhow!("no profile named {id:?}"))
    }

    pub fn launch_spec(&mut self, id: &str, url: Option<&str>) -> Result<provider::LaunchSpec> {
        let p = self.profile(id)?;
        let Some(inst) = self.install(&p.provider) else {
            let name = self.config.app_name(&p.provider);
            return Err(match p.provider {
                AppId::Builtin(_) => anyhow!("{name} isn't installed (or wasn't found). Choose it in Settings."),
                AppId::Custom(_) => anyhow!("{name} isn't where it was when you added it. Remove it in Settings and add it again."),
            });
        };
        Ok(provider::build_launch_spec(&p, &self.paths, &inst.program, &inst.base_args, inst.bundle.as_deref(), url))
    }

    /// Start (or bring to front) a profile, optionally handing it a URL.
    pub fn launch(&mut self, id: &str, url: Option<&str>) -> Result<()> {
        let p = self.profile(id)?;
        let spec = self.launch_spec(&p.id, url)?;
        launch::spawn(&spec)?;
        let version = self.install(&p.provider).and_then(|i| i.version);
        // Another process (the window, a shortcut) may have changed the
        // configuration since we read it; don't write back a stale copy.
        if let Ok(fresh) = Config::load(&self.paths) {
            self.config = fresh;
        }
        if let Some(prof) = self.config.get_mut(&p.id) {
            prof.last_launched_at = Some(Utc::now());
        }
        let _ = self.save();
        if let Some(v) = version {
            doctor::remember_version(&self.paths, &p.provider, &v);
        }
        if let Some(b) = p.provider.builtin() {
            self.schedule_reassert(b);
        }
        Ok(())
    }

    /// Launch a profile and remember that a login is in progress, so the
    /// browser's callback comes back to it.
    pub fn login(&mut self, id: &str) -> Result<()> {
        let p = self.profile(id)?;
        // Added apps have no sign-in link to route: just open them.
        if let Some(b) = p.provider.builtin() {
            let store = router::PendingStore::new(&self.paths);
            store.mark(b, &p.id)?;
            store.log(&format!("login started for {}", p.id));
        }
        self.launch(&p.id, None)
    }

    pub fn cancel_login(&self, provider: ProviderKind) -> Result<()> {
        router::PendingStore::new(&self.paths).consume(provider)
    }

    pub fn pending_login(&self, provider: ProviderKind) -> Option<router::Pending> {
        router::PendingStore::new(&self.paths).active(provider)
    }

    /// Ask the app to quit; `true` if a signal was sent.
    pub fn quit(&mut self, id: &str) -> Result<bool> {
        let snap = self.snapshot();
        let apps = snap.for_profile(id);
        if apps.is_empty() {
            return Ok(false);
        }
        // Every copy, not just the first (`any` would stop early).
        let mut sent = false;
        for a in apps {
            sent |= procs::terminate(a.pid);
        }
        Ok(sent)
    }

    /// Quit the given profiles, wait for them to exit, then start them one by
    /// one `stagger` apart (so their auto-update timers don't line up again).
    pub fn restart_staggered(&mut self, ids: &[String], stagger: Duration) -> Result<()> {
        for id in ids {
            self.quit(id)?;
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            let snap = self.snapshot();
            if ids.iter().all(|id| !snap.is_running(id)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        for (n, id) in ids.iter().enumerate() {
            if n > 0 {
                std::thread::sleep(stagger);
            }
            self.launch(id, None)?;
        }
        Ok(())
    }

    // ── routing ──

    /// Handle a URL the system gave us. `provider` comes from the shadow
    /// desktop entry; without it, the scheme decides.
    pub fn open_url(&mut self, provider: Option<ProviderKind>, url: Option<&str>) -> Result<RouteResult> {
        let url = url.map(str::trim).filter(|u| !u.is_empty());
        let Some(url) = url else {
            // The app's own menu entry was clicked (no URL): open the default.
            let provider = provider.unwrap_or(ProviderKind::Claude);
            let id = provider.default_profile_id().to_string();
            self.launch(&id, None)?;
            return Ok(RouteResult {
                provider,
                decision: Decision::To { profile: id, reason: Reason::Default },
                redacted: String::new(),
                is_login: false,
            });
        };
        let parsed = url::Url::parse(url).with_context(|| "not a valid URL".to_string())?;
        let provider = ProviderKind::from_scheme(parsed.scheme())
            .or(provider)
            .ok_or_else(|| anyhow!("VibeLauncher doesn't handle {}:// links", parsed.scheme()))?;
        let res = self.decide_route(provider, &parsed);
        if let Decision::To { profile, reason } = &res.decision {
            self.deliver(profile, url, *reason == Reason::PendingLogin && res.is_login)?;
            router::PendingStore::new(&self.paths).log(&format!(
                "{} → {} ({})",
                res.redacted,
                profile,
                reason.describe()
            ));
        } else {
            router::PendingStore::new(&self.paths).log(&format!("{} → asking the user", res.redacted));
        }
        Ok(res)
    }

    pub fn decide_route(&mut self, provider: ProviderKind, url: &url::Url) -> RouteResult {
        let is_login = provider.is_login_url(url);
        let snap = self.snapshot();
        let running = snap.running_ids(&provider.into());
        let candidates: Vec<router::Candidate> = self
            .config
            .profiles
            .iter()
            .filter(|p| p.provider == provider)
            .map(|p| router::Candidate {
                id: p.id.clone(),
                running: running.contains(&p.id),
                last_launched_at: p.last_launched_at,
            })
            .collect();
        let pending = router::PendingStore::new(&self.paths).load();
        let decision = router::decide(provider, is_login, &pending, &candidates, Utc::now());
        RouteResult { provider, decision, redacted: router::redact(url.as_str()), is_login }
    }

    /// Hand `url` to profile `id` (starting it if needed).
    pub fn deliver(&mut self, id: &str, url: &str, consume_pending: bool) -> Result<()> {
        let p = self.profile(id)?;
        if let (true, Some(b)) = (consume_pending, p.provider.builtin()) {
            let _ = router::PendingStore::new(&self.paths).consume(b);
        }
        self.launch(&p.id, Some(url))
    }

    /// The user picked a profile for an ambiguous callback.
    pub fn deliver_choice(&mut self, id: &str, url: &str) -> Result<()> {
        let p = self.profile(id)?;
        let parsed = url::Url::parse(url.trim()).context("not a valid URL")?;
        if !p.provider.builtin().is_some_and(|b| b.schemes().contains(&parsed.scheme())) {
            bail!("{} can't open {}:// links", self.config.app_name(&p.provider), parsed.scheme());
        }
        router::PendingStore::new(&self.paths).log(&format!("{} → {} (chosen by you)", router::redact(url), p.id));
        self.deliver(&p.id, url, true)
    }

    fn schedule_reassert(&mut self, provider: ProviderKind) {
        if !scheme::needs_reassert() || !self.routing_wanted(provider) {
            return;
        }
        launch::keep_std_handles_private();
        let exe = paths::self_exe();
        let mut c = std::process::Command::new(exe);
        c.args(["internal-reassert", provider.id()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000 | 0x0000_0008); // CREATE_NO_WINDOW | DETACHED_PROCESS
        }
        let _ = c.spawn();
    }

    /// Re-take the scheme a few times after a launch: the apps register
    /// themselves as handler when they start (macOS / Windows).
    pub fn reassert_after_launch(&mut self, provider: ProviderKind) {
        for wait in [3u64, 8, 20, 45] {
            std::thread::sleep(Duration::from_secs(wait));
            if !self.reassert_now(provider) {
                return;
            }
        }
    }

    /// One re-take, unless routing was turned off or the scheme released
    /// meanwhile. Returns whether it's still worth trying later.
    pub fn reassert_now(&mut self, provider: ProviderKind) -> bool {
        if self.reload().is_err() || !self.routing_wanted(provider) {
            return false;
        }
        let Some(inst) = self.install(provider) else { return false };
        let _ = scheme::reassert(provider, &inst, &self.paths);
        true
    }

    pub fn routing_enabled(&self, provider: ProviderKind) -> bool {
        match provider {
            ProviderKind::Claude => self.config.settings.route_claude,
            ProviderKind::ChatGpt => self.config.settings.route_chatgpt,
        }
    }

    /// Routing is only worth taking over when there is more than one profile.
    pub fn routing_wanted(&self, provider: ProviderKind) -> bool {
        self.routing_enabled(provider) && self.config.profiles.iter().any(|p| p.provider == provider && p.isolated)
    }

    /// Claim or release each scheme to match the settings. Returns problems.
    pub fn apply_routing(&mut self) -> Vec<String> {
        let mut errs = Vec::new();
        for provider in ProviderKind::ALL {
            let inst = self.install(provider);
            let st = scheme::status(provider, inst.as_ref(), &self.paths);
            let res = if self.routing_wanted(provider) {
                match &inst {
                    Some(i) if st.supported && (!st.claimed || st.stale) => scheme::claim(provider, i, &self.paths),
                    _ => {
                        if st.claimed {
                            scheme::remember_claim(provider, &self.paths);
                        }
                        Ok(())
                    }
                }
            } else if st.claimed {
                scheme::release(provider, inst.as_ref(), &self.paths)
            } else {
                Ok(())
            };
            if let Err(e) = res {
                errs.push(format!("{}: {e:#}", provider.display_name()));
            }
        }
        errs
    }

    // ── profiles ──

    pub fn create_profile(&mut self, input: ProfileInput) -> Result<Profile> {
        if let AppId::Custom(app) = &input.provider {
            if self.config.custom_app(app).is_none() {
                bail!("no added app {app:?}");
            }
        }
        let id = self.config.unique_id(&input.name, &input.provider);
        let p = Profile {
            id,
            name: input.name.trim().to_string(),
            provider: input.provider,
            color: input.color,
            icon: input.icon.unwrap_or_else(|| "sparkle".into()),
            isolated: true,
            data_root: input.data_root.filter(|p| !p.as_os_str().is_empty()),
            env: input.env,
            extra_args: input.extra_args,
            shortcut: input.shortcut,
            created_at: Utc::now(),
            last_launched_at: None,
        };
        self.config.validate_profile(&p, &self.paths, None)?;
        if let Some(root) = &p.data_root {
            if !root.is_absolute() {
                bail!("the data folder must be an absolute path");
            }
        }
        self.config.profiles.push(p.clone());
        self.save()?;
        self.after_profile_change(&p);
        Ok(p)
    }

    pub fn update_profile(&mut self, id: &str, input: ProfileInput) -> Result<Profile> {
        let old = self.profile(id)?;
        if input.provider != old.provider {
            bail!("a profile's app can't be changed; create a new profile instead");
        }
        let mut p = old.clone();
        p.name = input.name.trim().to_string();
        p.color = input.color;
        if let Some(icon) = input.icon {
            p.icon = icon;
        }
        p.shortcut = input.shortcut;
        if old.isolated {
            p.data_root = input.data_root.filter(|p| !p.as_os_str().is_empty());
            p.env = input.env;
            p.extra_args = input.extra_args;
        }
        self.config.validate_profile(&p, &self.paths, Some(&old.id))?;
        if p.data_root != old.data_root && self.snapshot().is_running(&old.id) {
            bail!("quit {} before moving its data folder", old.name);
        }
        *self.config.get_mut(&old.id).unwrap() = p.clone();
        self.save()?;
        if old.shortcut && !p.shortcut {
            let _ = shortcuts::remove(&old, &self.config.app_name(&old.provider));
        }
        self.after_profile_change(&p);
        Ok(p)
    }

    fn after_profile_change(&mut self, p: &Profile) {
        if p.shortcut {
            let inst = self.install(&p.provider);
            let _ = shortcuts::create(&self.paths, inst.as_ref(), p, &self.config.app_name(&p.provider));
        }
        let _ = self.apply_routing();
    }

    /// Remove a profile. Its data goes to the trash only when `delete_data`.
    pub fn delete_profile(&mut self, id: &str, delete_data: bool) -> Result<()> {
        let p = self.profile(id)?;
        if !p.isolated {
            bail!("the default profile can't be deleted");
        }
        if self.snapshot().is_running(&p.id) {
            bail!("quit {} first", p.name);
        }
        let _ = shortcuts::remove(&p, &self.config.app_name(&p.provider));
        if delete_data {
            let root = p.data_root(&self.paths);
            if root.exists() {
                trash::delete(&root).with_context(|| format!("moving {} to the trash", root.display()))?;
            }
        }
        self.config.profiles.retain(|x| x.id != p.id);
        self.save()?;
        let _ = self.apply_routing();
        Ok(())
    }

    pub fn reorder(&mut self, ids: &[String]) -> Result<()> {
        let mut out: Vec<Profile> = Vec::new();
        for id in ids {
            if let Some(p) = self.config.get(id) {
                out.push(p.clone());
            }
        }
        for p in &self.config.profiles {
            if !out.iter().any(|x| x.id == p.id) {
                out.push(p.clone());
            }
        }
        self.config.profiles = out;
        self.save()
    }

    pub fn set_settings(&mut self, s: Settings) -> Result<Vec<String>> {
        let overrides_changed = s.app_overrides != self.config.settings.app_overrides;
        self.config.settings = s;
        self.save()?;
        if overrides_changed {
            self.refresh_installs();
        }
        Ok(self.apply_routing())
    }

    /// Undo what VibeLauncher set up outside its own folders, before it is
    /// uninstalled: give the link schemes back to the apps and remove the
    /// shortcuts. Profiles, their data and the settings stay, so installing
    /// again brings everything back. Returns problems.
    pub fn cleanup(&mut self) -> Vec<String> {
        let mut errs = Vec::new();
        for provider in ProviderKind::ALL {
            let inst = self.install(provider);
            let st = scheme::status(provider, inst.as_ref(), &self.paths);
            if st.claimed || scheme::was_claimed(provider, &self.paths) {
                if let Err(e) = scheme::release(provider, inst.as_ref(), &self.paths) {
                    errs.push(format!("{}: {e:#}", provider.display_name()));
                }
            }
        }
        for p in &self.config.profiles {
            let app = self.config.app_name(&p.provider);
            let path = shortcuts::shortcut_path(p, &app);
            if path.exists() {
                if let Err(e) = shortcuts::remove(p, &app) {
                    errs.push(format!("{}: {e:#}", p.name));
                }
            }
        }
        errs
    }

    pub fn sync_shortcuts(&mut self) -> Vec<String> {
        let installs: BTreeMap<AppId, Option<AppInstall>> =
            self.config.app_ids().into_iter().map(|a| (a.clone(), self.install(a))).collect();
        shortcuts::sync(&self.paths, &self.config, &|a| installs.get(a).cloned().flatten())
    }

    pub fn data_size(&self, id: &str) -> Option<u64> {
        let p = self.config.get(id)?;
        if !p.isolated {
            return None;
        }
        Some(dir_size(&p.data_root(&self.paths), 0))
    }
}

/// Total size of the files under `p` (symlinks not followed).
pub fn dir_size(p: &std::path::Path, depth: u32) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(p) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() || depth > 24 {
        return 0;
    }
    std::fs::read_dir(p)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| dir_size(&e.path(), depth + 1)).sum())
        .unwrap_or(0)
}
