//! The window's commands: thin wrappers over [`vl_core::Launcher`].
//!
//! The configuration is re-read before every command, because the
//! `vibelauncher` command (run by shortcuts and links) writes to it too.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use vl_core::{doctor, icon, router, scheme, shortcuts, AppInstall, Decision, Launcher, Profile, ProfileInput, ProviderKind, Settings};

type Res<T> = Result<T, String>;

fn e(err: anyhow::Error) -> String {
    format!("{err:#}")
}

pub struct AppState {
    launcher: Mutex<Option<Launcher>>,
    /// Rendered profile icons, keyed by everything that changes them.
    icons: Mutex<HashMap<String, String>>,
    app_icons: Mutex<HashMap<String, Option<String>>>,
    /// A link waiting for the user to pick a profile (`--choose-route`).
    route: Mutex<Option<String>>,
    opened_for_route: bool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl AppState {
    /// Run `f` with a launcher whose configuration is fresh from disk.
    fn with<R>(&self, f: impl FnOnce(&mut Launcher) -> anyhow::Result<R>) -> Res<R> {
        let mut g = lock(&self.launcher);
        if g.is_none() {
            *g = Some(Launcher::open().map_err(e)?);
        }
        let l = g.as_mut().unwrap();
        l.reload().map_err(e)?;
        f(l).map_err(e)
    }
}

// ── state shown by the window ──

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    #[serde(flatten)]
    profile: Profile,
    data_dir: String,
    running: bool,
    /// Running, but without its isolation variables (an update restarted it).
    lost_isolation: bool,
    shortcut_state: shortcuts::ShortcutState,
    shortcut_name: String,
    icon_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    profile: String,
    /// Seconds left before the marker expires.
    seconds_left: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppView {
    provider: ProviderKind,
    name: &'static str,
    install: Option<AppInstall>,
    icon_url: Option<String>,
    routing: scheme::SchemeStatus,
    routing_enabled: bool,
    routing_wanted: bool,
    pending: Option<PendingView>,
    unknown_running: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    version: &'static str,
    platform: &'static str,
    profiles: Vec<ProfileView>,
    apps: Vec<AppView>,
    settings: Settings,
    profiles_dir: String,
    config_file: String,
    cli: String,
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

impl AppState {
    fn profile_icon(&self, inst: Option<&AppInstall>, p: &Profile) -> Option<String> {
        let key = format!("{}|{}|{}|{:?}", p.id, p.color, p.isolated, inst.and_then(|i| i.icon.as_ref()));
        if let Some(v) = lock(&self.icons).get(&key) {
            return Some(v.clone());
        }
        let v = icon::data_url(inst, p, 96)?;
        lock(&self.icons).insert(key, v.clone());
        Some(v)
    }

    fn app_icon(&self, inst: &AppInstall) -> Option<String> {
        let key = format!("{:?}", inst.icon);
        if let Some(v) = lock(&self.app_icons).get(&key) {
            return v.clone();
        }
        let v = icon::app_icon_data_url(inst, 128);
        lock(&self.app_icons).insert(key, v.clone());
        v
    }
}

#[tauri::command]
pub async fn get_state(st: State<'_, AppState>) -> Res<StateView> {
    st.with(|l| {
        let snap = l.snapshot();
        let mut apps = Vec::new();
        let mut installs = HashMap::new();
        for provider in ProviderKind::ALL {
            let inst = l.install(provider);
            installs.insert(provider, inst.clone());
            let pending = l.pending_login(provider).map(|p| PendingView {
                profile: p.profile,
                seconds_left: (router::PENDING_TTL_SECS - (chrono_now() - p.created_at.timestamp())).max(0),
            });
            apps.push(AppView {
                provider,
                name: provider.display_name(),
                icon_url: inst.as_ref().and_then(|i| st.app_icon(i)),
                routing: scheme::status(provider, inst.as_ref(), &l.paths),
                routing_enabled: l.routing_enabled(provider),
                routing_wanted: l.routing_wanted(provider),
                install: inst,
                pending,
                unknown_running: snap.apps.iter().filter(|a| a.provider == provider && a.profile_id.is_none()).count(),
            });
        }
        let profiles = l
            .config
            .profiles
            .iter()
            .map(|p| {
                let root = p.data_root(&l.paths);
                let running = snap.for_profile(&p.id);
                let expected = p.provider.isolation_env(&root);
                let lost = p.isolated
                    && running.iter().any(|a| {
                        a.environ
                            .as_ref()
                            .is_some_and(|env| expected.iter().any(|(k, v)| env.get(k).map(String::as_str) != Some(v.as_str())))
                    });
                let inst = installs.get(&p.provider).cloned().flatten();
                ProfileView {
                    data_dir: if p.isolated { root.display().to_string() } else { String::new() },
                    running: !running.is_empty(),
                    lost_isolation: lost,
                    shortcut_state: shortcuts::state(p),
                    shortcut_name: shortcuts::display_name(p),
                    icon_url: st.profile_icon(inst.as_ref(), p),
                    profile: p.clone(),
                }
            })
            .collect();
        Ok(StateView {
            version: env!("CARGO_PKG_VERSION"),
            platform: platform(),
            profiles,
            apps,
            settings: l.config.settings.clone(),
            profiles_dir: l.paths.profiles_dir.display().to_string(),
            config_file: l.paths.config_file().display().to_string(),
            cli: vl_core::paths::self_exe().display().to_string(),
        })
    })
}

/// The click-through test reports here (only with the `selftest` feature).
/// Returns whether anyone is listening; the test doesn't run otherwise.
#[tauri::command]
fn selftest_report(line: String) -> bool {
    #[cfg(feature = "selftest")]
    if let Some(path) = std::env::var_os("VL_SELFTEST_OUT") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
        return true;
    }
    let _ = line;
    false
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ── profiles ──

#[tauri::command]
pub async fn launch(st: State<'_, AppState>, id: String) -> Res<()> {
    st.with(|l| l.launch(&id, None))
}

#[tauri::command]
pub async fn login(st: State<'_, AppState>, id: String) -> Res<()> {
    st.with(|l| l.login(&id))
}

#[tauri::command]
pub async fn cancel_login(st: State<'_, AppState>, provider: ProviderKind) -> Res<()> {
    st.with(|l| l.cancel_login(provider))
}

#[tauri::command]
pub async fn quit(st: State<'_, AppState>, id: String) -> Res<bool> {
    st.with(|l| l.quit(&id))
}

/// Hand a sign-in link the user pasted to a profile.
#[tauri::command]
pub async fn open_link(st: State<'_, AppState>, id: String, url: String) -> Res<()> {
    st.with(|l| {
        let url = url.trim();
        let parsed = url::Url::parse(url).map_err(|_| anyhow::anyhow!("That isn't a link."))?;
        let p = l.config.find(&id).cloned().ok_or_else(|| anyhow::anyhow!("no such profile"))?;
        if !p.provider.schemes().contains(&parsed.scheme()) {
            anyhow::bail!(
                "That isn't a {} link. It should start with {}://",
                p.provider.display_name(),
                p.provider.schemes()[0]
            );
        }
        router::PendingStore::new(&l.paths).log(&format!("{} → {} (pasted)", router::redact(url), p.id));
        l.deliver(&p.id, url, true)
    })
}

#[tauri::command]
pub async fn create_profile(st: State<'_, AppState>, input: ProfileInput) -> Res<Profile> {
    st.with(|l| l.create_profile(input))
}

#[tauri::command]
pub async fn update_profile(st: State<'_, AppState>, id: String, input: ProfileInput) -> Res<Profile> {
    st.with(|l| l.update_profile(&id, input))
}

#[tauri::command]
pub async fn delete_profile(st: State<'_, AppState>, id: String, delete_data: bool) -> Res<()> {
    st.with(|l| l.delete_profile(&id, delete_data))
}

#[tauri::command]
pub async fn reorder(st: State<'_, AppState>, ids: Vec<String>) -> Res<()> {
    st.with(|l| l.reorder(&ids))
}

/// Size of a profile's data folder; walked without holding the lock.
#[tauri::command]
pub async fn data_size(st: State<'_, AppState>, id: String) -> Res<Option<u64>> {
    let root = st.with(|l| Ok(l.config.find(&id).filter(|p| p.isolated).map(|p| p.data_root(&l.paths))))?;
    Ok(tauri::async_runtime::spawn_blocking(move || root.map(|r| vl_core::dir_size(&r, 0)))
        .await
        .unwrap_or(None))
}

// ── settings & apps ──

#[tauri::command]
pub async fn set_settings(st: State<'_, AppState>, settings: Settings) -> Res<Vec<String>> {
    st.with(|l| l.set_settings(settings))
}

#[tauri::command]
pub async fn app_candidates(st: State<'_, AppState>, provider: ProviderKind) -> Res<Vec<AppInstall>> {
    st.with(|l| Ok(l.candidates(provider)))
}

#[tauri::command]
pub async fn set_app(st: State<'_, AppState>, provider: ProviderKind, path: Option<PathBuf>) -> Res<()> {
    st.with(|l| {
        l.set_app_override(provider, path)?;
        // The icon and routing follow the chosen app.
        let _ = l.apply_routing();
        let _ = l.sync_shortcuts();
        Ok(())
    })
}

#[tauri::command]
pub async fn refresh_apps(st: State<'_, AppState>) -> Res<()> {
    st.with(|l| {
        l.refresh_installs();
        Ok(())
    })
}

// ── diagnostics ──

#[tauri::command]
pub async fn run_doctor(st: State<'_, AppState>) -> Res<Vec<doctor::Check>> {
    st.with(|l| Ok(doctor::run(l)))
}

/// Apply a fix. Restarts wait between launches, so they run on their own
/// launcher in the background and report back with a `fix-done` event.
#[tauri::command]
pub async fn apply_fix(app: AppHandle, st: State<'_, AppState>, fix: doctor::Fix) -> Res<bool> {
    if let doctor::Fix::Restart { .. } = &fix {
        std::thread::spawn(move || {
            let res = Launcher::open().and_then(|mut l| doctor::apply(&mut l, &fix)).map_err(e);
            let _ = app.emit("fix-done", res.err());
        });
        return Ok(true);
    }
    st.with(|l| doctor::apply(l, &fix)).map(|_| false)
}

#[tauri::command]
pub async fn route_log(st: State<'_, AppState>) -> Res<Vec<String>> {
    st.with(|l| Ok(router::PendingStore::new(&l.paths).recent_log(60)))
}

// ── choosing a profile for a link ──

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequest {
    url: String,
    redacted: String,
    provider: ProviderKind,
    is_login: bool,
    candidates: Vec<String>,
}

fn route_request(l: &mut Launcher, url: &str) -> anyhow::Result<RouteRequest> {
    let parsed = url::Url::parse(url)?;
    let provider = ProviderKind::from_scheme(parsed.scheme()).ok_or_else(|| anyhow::anyhow!("not a supported link"))?;
    let res = l.decide_route(provider, &parsed);
    let candidates = match res.decision {
        Decision::Ask { candidates } => candidates,
        Decision::To { profile, .. } => vec![profile],
    };
    Ok(RouteRequest { url: url.to_string(), redacted: res.redacted, provider, is_login: res.is_login, candidates })
}

/// The link the window was opened for, if any (taken once).
#[tauri::command]
pub async fn take_route(st: State<'_, AppState>) -> Res<Option<RouteRequest>> {
    let Some(url) = lock(&st.route).take() else { return Ok(None) };
    st.with(|l| route_request(l, &url)).map(Some)
}

#[tauri::command]
pub async fn deliver_choice(st: State<'_, AppState>, id: String, url: String) -> Res<()> {
    st.with(|l| l.deliver_choice(&id, &url))
}

// ── misc ──

/// Show a file or folder in the system file manager.
#[tauri::command]
pub async fn reveal(path: String) -> Res<()> {
    let p = PathBuf::from(&path);
    let target = if p.exists() { p } else { p.parent().map(PathBuf::from).unwrap_or(p) };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(&target).spawn();
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(format!("/select,{}", target.display())).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = {
        let dir = if target.is_dir() { target.clone() } else { target.parent().map(PathBuf::from).unwrap_or(target.clone()) };
        std::process::Command::new("xdg-open").arg(dir).spawn()
    };
    r.map(|_| ()).map_err(|err| err.to_string())
}

/// Open the project's web page (fixed addresses only).
#[tauri::command]
pub async fn open_website(page: String) -> Res<()> {
    let url = match page.as_str() {
        "issues" => "https://github.com/Gloridust/VibeLauncher/issues",
        "releases" => "https://github.com/Gloridust/VibeLauncher/releases",
        _ => "https://github.com/Gloridust/VibeLauncher",
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.map(|_| ()).map_err(|err| err.to_string())
}

/// Whether the window was opened only to choose a profile for a link.
#[tauri::command]
pub fn opened_for_route(st: State<'_, AppState>) -> bool {
    st.opened_for_route
}

fn route_arg(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "--choose-route")?;
    args.get(i + 1).cloned()
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn run(args: Vec<String>) {
    let state = AppState {
        launcher: Mutex::new(None),
        icons: Mutex::new(HashMap::new()),
        app_icons: Mutex::new(HashMap::new()),
        route: Mutex::new(route_arg(&args)),
        opened_for_route: route_arg(&args).is_some(),
    };
    // A small window needs a small runtime: the default spins up a worker per
    // CPU, and each one ends up with its own malloc arena.
    if let Ok(rt) = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        tauri::async_runtime::set(rt.handle().clone());
        std::mem::forget(rt);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(url) = route_arg(&argv) {
                *lock(&app.state::<AppState>().route) = Some(url);
                let _ = app.emit("route-requested", ());
            }
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            // Give the native frame the chosen theme before the window first
            // appears: on Windows a later change from the page doesn't always
            // repaint the title bar, which then stays light over a dark window.
            if let (Ok(l), Some(w)) = (Launcher::open(), app.get_webview_window("main")) {
                let theme = match l.config.settings.theme.as_str() {
                    "dark" => Some(tauri::Theme::Dark),
                    "light" => Some(tauri::Theme::Light),
                    _ => None,
                };
                if theme.is_some() {
                    let _ = w.set_theme(theme);
                }
            }
            // The page shows the window once it has drawn; this is the safety
            // net if it never does.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1500));
                if let Some(w) = handle.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            launch,
            login,
            cancel_login,
            quit,
            open_link,
            create_profile,
            update_profile,
            delete_profile,
            reorder,
            data_size,
            set_settings,
            app_candidates,
            set_app,
            refresh_apps,
            run_doctor,
            apply_fix,
            route_log,
            take_route,
            deliver_choice,
            reveal,
            open_website,
            opened_for_route,
            selftest_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeLauncher");
}
