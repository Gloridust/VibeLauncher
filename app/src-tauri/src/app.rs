//! The window's logic. A worker thread owns the [`Launcher`] (its calls read
//! the disk, list processes and start apps, so they stay off the UI thread);
//! the UI thread keeps what is on screen and turns it into the `App` global.
//!
//! The configuration is re-read before every call, because the
//! `vibelauncher` command (run by shortcuts and links) writes to it too.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use slint::{Color, ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};
use vl_core::doctor::{self, Check, Fix, Level};
use vl_core::{icon, router, scheme, shortcuts, AppInstall, Decision, Launcher, Profile, ProfileInput, ProviderKind, Settings};

use crate::i18n::{self, t, tf};
use crate::{update, App, AppRow, AppWindow, CheckRow, ChoiceRow, ProfileView, SegOption, SideRow, Theme, ToastItem, Tr};

type Pixels = SharedPixelBuffer<Rgba8Pixel>;

const POLL: Duration = Duration::from_millis(2500);
const DOCTOR_EVERY: Duration = Duration::from_secs(30);
const LOGIN_SECS: i64 = 300;
pub const COLORS: [&str; 10] = ["#D97757", "#E5484D", "#F59E0B", "#2F9E62", "#14B8A6", "#3B82F6", "#6E7BF2", "#A855F7", "#EC4899", "#78716C"];

// ───────────────────────── what the worker reads ─────────────────────────

struct PSnap {
    p: Profile,
    data_dir: String,
    running: bool,
    lost: bool,
    shortcut: shortcuts::ShortcutState,
    shortcut_name: String,
    icon: Option<Pixels>,
}

struct ASnap {
    provider: ProviderKind,
    name: &'static str,
    install: Option<AppInstall>,
    icon: Option<Pixels>,
    routing: scheme::SchemeStatus,
    routing_wanted: bool,
    /// The profile waiting for a sign-in link, and seconds left.
    pending: Option<(String, i64)>,
}

pub struct Snap {
    profiles: Vec<PSnap>,
    apps: Vec<ASnap>,
    settings: Settings,
    profiles_dir: String,
    config_file: String,
    cli: String,
}

impl Snap {
    fn app(&self, p: ProviderKind) -> &ASnap {
        self.apps.iter().find(|a| a.provider == p).expect("every provider")
    }
    fn profile(&self, id: &str) -> Option<&PSnap> {
        self.profiles.iter().find(|p| p.p.id == id)
    }
    /// Profiles shown in the sidebar: an app's group is hidden when the app
    /// isn't installed and has no separate profiles.
    fn visible(&self) -> Vec<&PSnap> {
        ProviderKind::ALL
            .iter()
            .flat_map(|&pv| {
                let app = self.app(pv);
                let rows: Vec<&PSnap> = self.profiles.iter().filter(|p| p.p.provider == pv).collect();
                if app.install.is_none() && !rows.iter().any(|p| p.p.isolated) {
                    vec![]
                } else {
                    rows
                }
            })
            .collect()
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn to_pixels(img: image::RgbaImage, size: u32) -> Pixels {
    let img = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
    SharedPixelBuffer::clone_from_slice(img.as_raw(), size, size)
}

// ───────────────────────── worker thread ─────────────────────────

type Job = Box<dyn FnOnce(&mut Worker) + Send>;

pub struct Worker {
    launcher: Option<Launcher>,
    icons: HashMap<String, Option<Pixels>>,
}

impl Worker {
    fn with<R>(&mut self, f: impl FnOnce(&mut Launcher) -> anyhow::Result<R>) -> Result<R, String> {
        if self.launcher.is_none() {
            self.launcher = Some(Launcher::open().map_err(|e| format!("{e:#}"))?);
        }
        let l = self.launcher.as_mut().unwrap();
        l.reload().map_err(|e| format!("{e:#}"))?;
        f(l).map_err(|e| format!("{e:#}"))
    }

    fn snapshot(&mut self) -> Result<Snap, String> {
        let mut icons = std::mem::take(&mut self.icons);
        let r = self.with(|l| {
            let snap = l.snapshot();
            let mut apps = Vec::new();
            let mut installs = HashMap::new();
            for provider in ProviderKind::ALL {
                let inst = l.install(provider);
                installs.insert(provider, inst.clone());
                let key = format!("app|{:?}", inst.as_ref().and_then(|i| i.icon.clone()));
                let app_icon = icons
                    .entry(key)
                    .or_insert_with(|| inst.as_ref().and_then(|i| icon::load_base(i.icon.as_deref()?)).map(|img| to_pixels(img, 64)))
                    .clone();
                apps.push(ASnap {
                    provider,
                    name: provider.display_name(),
                    icon: app_icon,
                    routing: scheme::status(provider, inst.as_ref(), &l.paths),
                    routing_wanted: l.routing_wanted(provider),
                    pending: l
                        .pending_login(provider)
                        .map(|p| (p.profile, (router::PENDING_TTL_SECS - (now_secs() - p.created_at.timestamp())).max(0))),
                    install: inst,
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
                    let key = format!("{}|{}|{}|{:?}", p.id, p.color, p.isolated, inst.as_ref().and_then(|i| i.icon.clone()));
                    let icon = icons.entry(key).or_insert_with(|| Some(to_pixels(icon::compose(inst.as_ref(), p), 128))).clone();
                    PSnap {
                        data_dir: if p.isolated { root.display().to_string() } else { String::new() },
                        running: !running.is_empty(),
                        lost,
                        shortcut: shortcuts::state(p),
                        shortcut_name: shortcuts::display_name(p),
                        icon,
                        p: p.clone(),
                    }
                })
                .collect();
            Ok(Snap {
                profiles,
                apps,
                settings: l.config.settings.clone(),
                profiles_dir: l.paths.profiles_dir.display().to_string(),
                config_file: l.paths.config_file().display().to_string(),
                cli: vl_core::paths::self_exe().display().to_string(),
            })
        });
        self.icons = icons;
        r
    }
}

fn spawn_worker() -> mpsc::Sender<Job> {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("launcher".into())
        .spawn(move || {
            let mut w = Worker { launcher: None, icons: HashMap::new() };
            for job in rx {
                job(&mut w);
            }
        })
        .expect("worker thread");
    tx
}

// ───────────────────────── UI-thread state ─────────────────────────

#[derive(Clone, PartialEq)]
enum View {
    /// Pick the first running (or first) profile, or the welcome page.
    Auto,
    Welcome,
    Profile(String),
    Diagnostics,
    Settings,
}

#[derive(Clone)]
struct RouteReq {
    url: String,
    redacted: String,
    provider: ProviderKind,
    is_login: bool,
    candidates: Vec<String>,
}

#[derive(Clone, PartialEq)]
enum LoginPhase {
    Starting,
    Waiting,
    Done,
    Expired,
    Error,
}

#[derive(Clone)]
enum Sheet {
    None,
    New { provider: ProviderKind },
    Edit { id: String },
    Delete { id: String },
    Login { id: String, phase: LoginPhase, deadline: Instant },
    Route(RouteReq),
}

#[derive(Clone, Copy, PartialEq)]
enum UpdPhase {
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Installing,
    Error,
}

struct Upd {
    phase: UpdPhase,
    found: Option<update::Found>,
    progress: Option<f32>,
    error: Option<String>,
}

pub struct Ctl {
    win: AppWindow,
    tx: mpsc::Sender<Job>,
    snap: Option<Rc<Snap>>,
    load_error: Option<String>,
    view: View,
    sheet: Sheet,
    checks: Option<Vec<Check>>,
    log: Option<Vec<String>>,
    collapsed: BTreeSet<ProviderKind>,
    toasts: Vec<(i32, String, bool)>,
    toast_seq: i32,
    opened_for_route: bool,
    pending_route: Option<String>,
    upd: Upd,
    auto_checked: bool,
    sizes: HashMap<String, Option<u64>>,
    busy: String,
    images: HashMap<usize, Image>,
    doctor_at: Instant,
    focused: bool,
    /// Settings being saved: shown (and built on by the next change) until
    /// the saves are done, so quick changes in a row don't undo each other.
    pending_settings: Option<Settings>,
    saves: u32,
    /// Theme shown regardless of the setting (headless screenshots).
    theme_override: Option<String>,
    // Keep timers alive.
    timers: Vec<Timer>,
}

thread_local! {
    static CTL: RefCell<Option<Ctl>> = const { RefCell::new(None) };
}

/// Run `f` with the controller (UI thread only).
fn ctl<R>(f: impl FnOnce(&mut Ctl) -> R) -> Option<R> {
    CTL.with(|c| c.borrow_mut().as_mut().map(f))
}

/// From any thread: run `f` with the controller on the UI thread.
fn later(f: impl FnOnce(&mut Ctl) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        ctl(f);
    });
}

impl Ctl {
    /// Run `job` on the worker; its result comes back to `done` here.
    fn work<R: Send + 'static>(&self, job: impl FnOnce(&mut Worker) -> R + Send + 'static, done: impl FnOnce(&mut Ctl, R) + Send + 'static) {
        let _ = self.tx.send(Box::new(move |w| {
            let r = job(w);
            later(move |c| done(c, r));
        }));
    }

    /// Run a launcher call; report its error as a toast, then refresh.
    fn act(&mut self, busy: &str, f: impl FnOnce(&mut Launcher) -> anyhow::Result<()> + Send + 'static, ok: Option<String>) {
        if !busy.is_empty() {
            self.busy = busy.into();
            self.render();
        }
        self.work(
            move |w| w.with(f),
            move |c, r| {
                c.busy.clear();
                match r {
                    Ok(()) => {
                        if let Some(ok) = ok {
                            c.toast(ok, false);
                        }
                    }
                    Err(e) => c.toast(e, true),
                }
                c.refresh();
            },
        );
    }

    fn refresh(&self) {
        self.work(
            |w| w.snapshot(),
            |c, r| {
                match r {
                    Ok(mut s) => {
                        if let Some(p) = &c.pending_settings {
                            s.settings = p.clone();
                        }
                        c.snap = Some(Rc::new(s));
                        c.load_error = None;
                    }
                    Err(e) => c.load_error = Some(e),
                }
                c.after_refresh();
            },
        );
    }

    fn after_refresh(&mut self) {
        if let Some(s) = self.snap.clone() {
            i18n::set_language(&s.settings.language);
            if !self.auto_checked {
                self.auto_checked = true;
                if s.settings.check_updates {
                    self.check_update(false);
                }
            }
            if let Some(url) = self.pending_route.take() {
                self.open_route(url);
            }
            // A sign-in that was waiting is done once its marker is used.
            if let Sheet::Login { id, phase: LoginPhase::Waiting, deadline } = &self.sheet {
                let started = deadline.checked_duration_since(Instant::now()).is_none_or(|left| left < Duration::from_secs(295));
                let still = s.profile(id).and_then(|p| s.app(p.p.provider).pending.as_ref()).is_some_and(|(pid, _)| pid == id);
                if started && !still {
                    let (id, deadline) = (id.clone(), *deadline);
                    self.sheet = Sheet::Login { id, phase: LoginPhase::Done, deadline };
                }
            }
        }
        self.render();
    }

    fn run_doctor(&mut self) {
        self.doctor_at = Instant::now();
        self.work(
            |w| w.with(|l| Ok(doctor::run(l))),
            |c, r| {
                if let Ok(checks) = r {
                    c.checks = Some(checks);
                }
                c.load_log();
                c.render();
            },
        );
    }

    fn load_log(&self) {
        self.work(
            |w| w.with(|l| Ok(router::PendingStore::new(&l.paths).recent_log(60))),
            |c, r| {
                c.log = Some(r.unwrap_or_default());
                c.render();
            },
        );
    }

    fn toast(&mut self, text: String, error: bool) {
        self.toast_seq += 1;
        let id = self.toast_seq;
        if self.toasts.len() >= 3 {
            self.toasts.remove(0);
        }
        self.toasts.push((id, text, error));
        self.render_toasts();
        Timer::single_shot(Duration::from_millis(if error { 6000 } else { 2800 }), move || {
            ctl(|c| {
                c.toasts.retain(|t| t.0 != id);
                c.render_toasts();
            });
        });
    }

    fn render_toasts(&self) {
        let items: Vec<ToastItem> = self.toasts.iter().map(|(id, text, error)| ToastItem { id: *id, text: text.into(), error: *error }).collect();
        self.win.global::<App>().set_toasts(ModelRc::new(VecModel::from(items)));
    }

    // ───────── what is on screen ─────────

    fn image(&mut self, px: &Option<Pixels>) -> Image {
        let Some(px) = px else { return Image::default() };
        let key = px.as_bytes().as_ptr() as usize;
        self.images.entry(key).or_insert_with(|| Image::from_rgba8(px.clone())).clone()
    }

    fn current_view(&self) -> View {
        let Some(s) = &self.snap else { return View::Welcome };
        match &self.view {
            View::Profile(id) if s.profile(id).is_some() => self.view.clone(),
            View::Profile(_) | View::Auto => {
                if !s.settings.onboarded && !self.opened_for_route {
                    return View::Welcome;
                }
                let vis = s.visible();
                vis.iter().find(|p| p.running).or(vis.first()).map_or(View::Welcome, |p| View::Profile(p.p.id.clone()))
            }
            v => v.clone(),
        }
    }

    fn render(&mut self) {
        let win = self.win.clone_strong();
        let g = win.global::<App>();
        let tr = win.global::<Tr>();
        tr.set_lang(i18n::lang().into());
        g.set_version(env!("CARGO_PKG_VERSION").into());
        let Some(s) = self.snap.clone() else {
            g.set_loaded(false);
            g.set_load_error(self.load_error.clone().unwrap_or_default().into());
            return;
        };
        g.set_loaded(true);
        let theme = self.theme_override.clone().unwrap_or_else(|| s.settings.theme.clone());
        win.global::<Theme>().set_mode(theme.as_str().into());

        let view = self.current_view();
        g.set_page(
            match view {
                View::Profile(_) => "profile",
                View::Diagnostics => "diagnostics",
                View::Settings => "settings",
                _ => "welcome",
            }
            .into(),
        );

        // sidebar
        let mut rows = Vec::new();
        for pv in ProviderKind::ALL {
            let app = s.app(pv);
            let list: Vec<&PSnap> = s.profiles.iter().filter(|p| p.p.provider == pv).collect();
            if app.install.is_none() && !list.iter().any(|p| p.p.isolated) {
                continue;
            }
            let collapsed = self.collapsed.contains(&pv);
            rows.push(SideRow {
                header: true,
                provider: pv.id().into(),
                label: app.name.into(),
                note: if app.install.is_none() { t("nav.notInstalled").into() } else { SharedString::new() },
                collapsed,
                ..Default::default()
            });
            if collapsed {
                continue;
            }
            for p in list {
                rows.push(SideRow {
                    header: false,
                    provider: pv.id().into(),
                    label: p.p.name.as_str().into(),
                    id: p.p.id.as_str().into(),
                    icon: self.image(&p.icon),
                    color: color(&p.p.color),
                    running: p.running,
                    lost: p.lost,
                    pending: app.pending.as_ref().is_some_and(|(id, _)| *id == p.p.id),
                    ..Default::default()
                });
            }
        }
        g.set_side_rows(ModelRc::new(VecModel::from(rows)));
        let (warn, err) = self.checks.as_ref().map_or((0, 0), |cs| {
            (cs.iter().filter(|c| c.level == Level::Warning).count(), cs.iter().filter(|c| c.level == Level::Error).count())
        });
        g.set_issues((warn + err) as i32);
        g.set_issues_red(err > 0);
        g.set_update_badge(self.upd.phase == UpdPhase::Available);
        g.set_busy(self.busy.as_str().into());

        // pages
        if let View::Profile(id) = &view {
            if let Some(p) = s.profile(id) {
                if p.p.isolated && !self.sizes.contains_key(id) {
                    self.load_size(id.clone());
                }
                let pv = self.profile_view(&s, p);
                g.set_profile(pv);
            }
        }
        let apps: Vec<AppRow> = s.apps.iter().map(|a| self.app_row(&s, a)).collect();
        g.set_any_installed(s.apps.iter().any(|a| a.install.is_some()));
        g.set_apps(ModelRc::new(VecModel::from(apps)));
        self.render_checks();
        self.render_settings(&s);
        self.render_sheet(&s);
    }

    fn profile_view(&mut self, s: &Snap, p: &PSnap) -> ProfileView {
        let app = s.app(p.p.provider);
        let order: Vec<&str> = s.profiles.iter().filter(|x| x.p.provider == p.p.provider).map(|x| x.p.id.as_str()).collect();
        let pos = order.iter().position(|x| *x == p.p.id).unwrap_or(0);
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(windows) {
            "windows"
        } else {
            "linux"
        };
        ProfileView {
            id: p.p.id.as_str().into(),
            name: p.p.name.as_str().into(),
            provider: p.p.provider.id().into(),
            app_name: app.name.into(),
            app_installed: app.install.is_some(),
            color: color(&p.p.color),
            icon: self.image(&p.icon),
            isolated: p.p.isolated,
            running: p.running,
            lost: p.lost,
            pending: app.pending.as_ref().is_some_and(|(id, _)| *id == p.p.id),
            first_run: p.p.isolated && p.p.last_launched_at.is_none() && !p.running,
            data_dir: p.data_dir.as_str().into(),
            size: match self.sizes.get(&p.p.id) {
                Some(Some(n)) => i18n::format_bytes(*n).into(),
                _ => "…".into(),
            },
            shortcut: p.p.shortcut,
            shortcut_hint: tf(&format!("detail.shortcutHint.{platform}"), &[("name", &p.shortcut_name)]).into(),
            shortcut_broken: p.p.shortcut && (!p.shortcut.exists || p.shortcut.stale),
            last_opened: i18n::relative_time(p.p.last_launched_at).into(),
            env_text: p.p.env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n").into(),
            args_text: p.p.extra_args.join(" ").into(),
            can_move_up: pos > 0,
            can_move_down: pos + 1 < order.len(),
        }
    }

    fn app_row(&mut self, s: &Snap, a: &ASnap) -> AppRow {
        let scheme = if a.provider == ProviderKind::Claude { "claude" } else { "codex" };
        let r = &a.routing;
        let on = match a.provider {
            ProviderKind::Claude => s.settings.route_claude,
            ProviderKind::ChatGpt => s.settings.route_chatgpt,
        };
        let status = if !r.supported {
            t("settings.routeUnsupported")
        } else if !on {
            t("settings.routeOff")
        } else if r.claimed {
            t("settings.routeOn")
        } else if a.routing_wanted {
            t("settings.routeOff")
        } else {
            t("settings.routeIdle")
        };
        let inst = a.install.as_ref();
        AppRow {
            provider: a.provider.id().into(),
            name: a.name.into(),
            icon: self.image(&a.icon),
            installed: inst.is_some(),
            version: inst.and_then(|i| i.version.clone()).unwrap_or_default().into(),
            path: inst.map(|i| i.bundle.as_ref().unwrap_or(&i.program).display().to_string()).unwrap_or_default().into(),
            manual: s.settings.app_overrides.contains_key(a.provider.id()),
            found: match inst {
                Some(i) => format!("{}{}", t("onb.found"), i.version.as_ref().map(|v| format!(" · {v}")).unwrap_or_default()),
                None => t("onb.missing"),
            }
            .into(),
            route_title: tf("settings.route", &[("scheme", scheme)]).into(),
            route_status: r.conflict.clone().unwrap_or(status).into(),
            route_warn: r.conflict.is_some(),
            route_on: on && r.supported,
            route_supported: r.supported,
            route_experimental: r.experimental && r.supported,
        }
    }

    fn render_checks(&self) {
        let g = self.win.global::<App>();
        g.set_checks_loaded(self.checks.is_some());
        let mut checks = self.checks.clone().unwrap_or_default();
        let order = |l: Level| match l {
            Level::Error => 0,
            Level::Warning => 1,
            Level::Info => 2,
            Level::Ok => 3,
        };
        checks.sort_by_key(|c| order(c.level));
        let issues = checks.iter().filter(|c| matches!(c.level, Level::Warning | Level::Error)).count();
        let has_err = checks.iter().any(|c| c.level == Level::Error);
        g.set_summary(
            if self.checks.is_none() {
                t("diag.checking")
            } else if issues == 0 {
                t("diag.allGood")
            } else if issues == 1 {
                t("diag.issue")
            } else {
                tf("diag.issues", &[("n", &issues.to_string())])
            }
            .into(),
        );
        g.set_summary_level(if issues == 0 { "ok" } else if has_err { "err" } else { "warn" }.into());
        let rows: Vec<CheckRow> = checks
            .iter()
            .map(|c| {
                let (title, detail) = i18n::check_text(c);
                CheckRow {
                    id: c.id.as_str().into(),
                    level: format!("{:?}", c.level).to_lowercase().into(),
                    title: title.into(),
                    detail: detail.into(),
                    has_fix: c.fix.is_some() && c.level != Level::Ok,
                    fix_label: if matches!(c.fix, Some(Fix::ChooseApp { .. })) { t("action.choose") } else { t("diag.fix") }.into(),
                }
            })
            .collect();
        g.set_checks(ModelRc::new(VecModel::from(rows)));
        match &self.log {
            None => g.set_log_state(0),
            Some(l) if l.is_empty() => g.set_log_state(1),
            Some(l) => {
                g.set_log_state(2);
                g.set_log_text(l.iter().rev().map(|x| i18n::localize_log_line(x)).collect::<Vec<_>>().join("\n").into());
            }
        }
    }

    fn render_settings(&self, s: &Snap) {
        let g = self.win.global::<App>();
        g.set_language(s.settings.language.as_str().into());
        g.set_theme(s.settings.theme.as_str().into());
        g.set_stagger(s.settings.stagger_seconds as i32);
        g.set_check_updates(s.settings.check_updates);
        g.set_beta_updates(s.settings.beta_updates.unwrap_or_else(update::beta_by_default));
        g.set_cli(s.cli.as_str().into());
        g.set_config_file(s.config_file.as_str().into());

        let u = &self.upd;
        let kind = update::install_kind();
        let pct = u.progress.map(|p| format!("{}%", (p * 100.0).round())).unwrap_or_default();
        g.set_update_sub(
            match u.phase {
                UpdPhase::Checking => t("update.checking"),
                UpdPhase::Current => t("update.current"),
                UpdPhase::Available => tf("update.available", &[("version", u.found.as_ref().map_or("", |f| f.version.as_str()))]),
                UpdPhase::Downloading => tf("update.downloading", &[("pct", &pct)]),
                UpdPhase::Installing => t("update.installing"),
                UpdPhase::Error => tf("update.failed", &[("error", u.error.as_deref().unwrap_or(""))]),
                UpdPhase::Idle => t("update.idle"),
            }
            .into(),
        );
        g.set_update_sub_warn(u.phase == UpdPhase::Error);
        g.set_update_hint(
            if u.phase == UpdPhase::Available {
                match kind {
                    None => t("update.manualOnly"),
                    Some("deb" | "rpm") => t("update.password"),
                    _ => String::new(),
                }
            } else {
                String::new()
            }
            .into(),
        );
        g.set_update_phase(
            match u.phase {
                UpdPhase::Downloading => "downloading",
                UpdPhase::Installing => "installing",
                _ => "",
            }
            .into(),
        );
        g.set_update_progress(u.progress.unwrap_or(-1.0));
        let has = u.found.is_some() && !matches!(u.phase, UpdPhase::Checking | UpdPhase::Current);
        g.set_update_button(if has && kind.is_some() { "install" } else if has { "download" } else { "check" }.into());
        g.set_update_busy(matches!(u.phase, UpdPhase::Checking | UpdPhase::Downloading | UpdPhase::Installing));
        g.set_update_whats_new(matches!(u.phase, UpdPhase::Available | UpdPhase::Downloading | UpdPhase::Installing));
    }

    fn render_sheet(&mut self, s: &Snap) {
        let win = self.win.clone_strong();
        let g = win.global::<App>();
        let sheet = self.sheet.clone();
        g.set_sheet(
            match &sheet {
                Sheet::None => "",
                Sheet::New { .. } => "new",
                Sheet::Edit { .. } => "edit",
                Sheet::Delete { .. } => "delete",
                Sheet::Login { .. } => "login",
                Sheet::Route(_) => "route",
            }
            .into(),
        );
        match sheet {
            Sheet::New { provider } => self.render_profile_sheet(s, None, provider),
            Sheet::Edit { id } => {
                if let Some(p) = s.profile(&id) {
                    self.render_profile_sheet(s, Some(&p.p), p.p.provider)
                }
            }
            Sheet::Login { id, phase, deadline } => {
                let Some(p) = s.profile(&id) else { return };
                let app = s.app(p.p.provider);
                let name = app.name;
                g.set_login_title(tf("login.title", &[("name", &p.p.name)]).into());
                let steps: Vec<SharedString> = ["login.step1", "login.step2", "login.step3"].iter().map(|k| tf(k, &[("app", name)]).into()).collect();
                g.set_login_steps(ModelRc::new(VecModel::from(steps)));
                g.set_login_phase(
                    match phase {
                        LoginPhase::Starting => "starting",
                        LoginPhase::Waiting => "waiting",
                        LoginPhase::Done => "done",
                        LoginPhase::Expired => "expired",
                        LoginPhase::Error => "error",
                    }
                    .into(),
                );
                let left = deadline.saturating_duration_since(Instant::now()).as_secs() as i64;
                g.set_login_wait(tf("login.waiting", &[("time", &format!("{}:{:02}", left / 60, left % 60))]).into());
                g.set_login_left((left as f32 / LOGIN_SECS as f32).clamp(0.0, 1.0));
                let routed = app.routing.claimed && !app.routing.stale;
                g.set_login_routing_hint(if routed { String::new() } else { tf("login.routingOff", &[("app", name)]) }.into());
                g.set_login_paste_body(tf("login.pasteBody", &[("app", name)]).into());
                let scheme = if p.p.provider == ProviderKind::Claude { "claude" } else { "codex" };
                g.set_login_placeholder(tf("login.pastePlaceholder", &[("scheme", scheme)]).into());
            }
            Sheet::Route(req) => {
                let app = s.app(req.provider);
                g.set_route_title(if req.is_login { t("route.loginTitle") } else { t("route.title") }.into());
                g.set_route_sub(tf("route.body", &[("app", app.name), ("link", &req.redacted)]).into());
                let mut cands: Vec<&PSnap> = req.candidates.iter().filter_map(|id| s.profile(id)).collect();
                cands.sort_by_key(|p| !p.running);
                let rows: Vec<ChoiceRow> = cands
                    .into_iter()
                    .map(|p| ChoiceRow {
                        id: p.p.id.as_str().into(),
                        name: p.p.name.as_str().into(),
                        icon: self.image(&p.icon),
                        color: color(&p.p.color),
                        running: p.running,
                        sub: if p.running {
                            t("status.running")
                        } else {
                            format!("{} · {}", t("detail.lastOpened"), i18n::relative_time(p.p.last_launched_at))
                        }
                        .into(),
                    })
                    .collect();
                g.set_route_choices(ModelRc::new(VecModel::from(rows)));
            }
            _ => {}
        }
    }

    fn render_profile_sheet(&mut self, s: &Snap, ed: Option<&Profile>, provider: ProviderKind) {
        let win = self.win.clone_strong();
        let g = win.global::<App>();
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(windows) {
            "windows"
        } else {
            "linux"
        };
        let is_default = ed.is_some_and(|p| !p.isolated);
        g.set_ps_editing(ed.is_some());
        g.set_ps_is_default(is_default);
        g.set_ps_provider(provider.id().into());
        let opts: Vec<SegOption> = ProviderKind::ALL
            .iter()
            .map(|&pv| {
                let a = s.app(pv);
                SegOption {
                    value: pv.id().into(),
                    label: a.name.into(),
                    icon: self.image(&a.icon),
                    has_icon: a.icon.is_some(),
                    disabled: ed.is_some() || a.install.is_none(),
                }
            })
            .collect();
        g.set_ps_providers(ModelRc::new(VecModel::from(opts)));
        g.set_ps_colors(ModelRc::new(VecModel::from(COLORS.iter().map(|c| color(c)).collect::<Vec<_>>())));
        let name = g.get_ps_name().to_string();
        let shown = if is_default {
            format!("{} (VibeLauncher)", ed.map_or("", |p| p.name.as_str()))
        } else {
            format!("{} · {}", s.app(provider).name, if name.trim().is_empty() { "…" } else { name.trim() })
        };
        g.set_ps_shortcut_hint(tf(&format!("detail.shortcutHint.{platform}"), &[("name", &shown)]).into());
        let sep = if cfg!(windows) { "\\" } else { "/" };
        let id = ed.map(|p| p.id.clone()).unwrap_or_else(|| slug(&name));
        g.set_ps_default_path(format!("{}{sep}{id}", s.profiles_dir).into());
    }

    // ───────── sheets ─────────

    fn open_profile_sheet(&mut self, ed: Option<String>, provider: Option<ProviderKind>) {
        let Some(s) = self.snap.clone() else { return };
        let g = self.win.global::<App>();
        let p = ed.as_ref().and_then(|id| s.profile(id)).map(|p| p.p.clone());
        let installed: Vec<ProviderKind> = ProviderKind::ALL.into_iter().filter(|&pv| s.app(pv).install.is_some()).collect();
        let provider = p.as_ref().map(|p| p.provider).or(provider).or(installed.first().copied()).unwrap_or(ProviderKind::Claude);
        let used: BTreeSet<String> = s.profiles.iter().map(|p| p.p.color.to_lowercase()).collect();
        let col = p
            .as_ref()
            .map(|p| p.color.clone())
            .or_else(|| COLORS[1..].iter().find(|c| !used.contains(&c.to_lowercase())).map(|c| c.to_string()))
            .unwrap_or(COLORS[5].into());
        g.set_ps_name(p.as_ref().map(|p| p.name.clone()).unwrap_or_default().into());
        g.set_ps_color(color(&col));
        g.set_ps_shortcut(p.as_ref().is_none_or(|p| p.shortcut));
        g.set_ps_advanced(p.as_ref().is_some_and(|p| p.data_root.is_some() || !p.env.is_empty() || !p.extra_args.is_empty()));
        g.set_ps_data_root(p.as_ref().and_then(|p| p.data_root.as_ref()).map(|d| d.display().to_string()).unwrap_or_default().into());
        g.set_ps_env(p.as_ref().map(|p| p.env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n")).unwrap_or_default().into());
        g.set_ps_args(p.as_ref().map(|p| p.extra_args.join("\n")).unwrap_or_default().into());
        g.set_ps_error("".into());
        g.set_ps_saving(false);
        self.sheet = match ed {
            Some(id) => Sheet::Edit { id },
            None => Sheet::New { provider },
        };
        self.render();
    }

    fn save_profile(&mut self) {
        let g = self.win.global::<App>();
        if g.get_ps_saving() {
            return;
        }
        let name = g.get_ps_name().trim().to_string();
        if name.is_empty() {
            g.set_ps_error(t("sheet.namePlaceholder").into());
            return;
        }
        let env = match parse_env(&g.get_ps_env()) {
            Ok(e) => e,
            Err(line) => {
                g.set_ps_error(tf("sheet.badEnv", &[("n", &line.to_string())]).into());
                return;
            }
        };
        let (editing, provider) = match &self.sheet {
            Sheet::Edit { id } => (Some(id.clone()), self.snap.as_ref().and_then(|s| s.profile(id)).map(|p| p.p.provider)),
            Sheet::New { provider } => (None, Some(*provider)),
            _ => return,
        };
        let Some(provider) = provider else { return };
        let icon = editing.as_ref().and_then(|id| self.snap.as_ref()?.profile(id)).map(|p| p.p.icon.clone());
        let data_root = g.get_ps_data_root().trim().to_string();
        let input = ProfileInput {
            name: name.clone(),
            provider,
            color: hex(g.get_ps_color()),
            icon,
            data_root: (!data_root.is_empty()).then(|| PathBuf::from(data_root)),
            env,
            extra_args: g.get_ps_args().lines().map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect(),
            shortcut: g.get_ps_shortcut(),
        };
        g.set_ps_saving(true);
        g.set_ps_error("".into());
        self.work(
            move |w| {
                w.with(move |l| match &editing {
                    Some(id) => l.update_profile(id, input).map(|p| (p, true)),
                    None => l.create_profile(input).map(|p| (p, false)),
                })
            },
            |c, r| {
                let g = c.win.global::<App>();
                g.set_ps_saving(false);
                match r {
                    Ok((p, edited)) => {
                        c.sheet = Sheet::None;
                        if !edited {
                            c.view = View::Profile(p.id.clone());
                            c.run_doctor();
                        }
                        c.toast(if edited { t("sheet.saved") } else { tf("sheet.created", &[("name", &p.name)]) }, false);
                        c.refresh();
                    }
                    Err(e) => g.set_ps_error(e.into()),
                }
            },
        );
    }

    fn open_delete(&mut self, id: String) {
        let Some(s) = self.snap.clone() else { return };
        let Some(p) = s.profile(&id) else { return };
        let g = self.win.global::<App>();
        g.set_del_title(tf("delete.title", &[("name", &p.p.name)]).into());
        g.set_del_sub(t(if p.running { "delete.bodyRunning" } else { "delete.body" }).into());
        g.set_del_hint(tf("delete.dataHint", &[("path", &p.data_dir)]).into());
        g.set_del_confirm(t(if p.running { "delete.quitConfirm" } else { "delete.confirm" }).into());
        g.set_del_with_data(false);
        g.set_del_busy(false);
        g.set_del_error("".into());
        self.sheet = Sheet::Delete { id };
        self.render();
    }

    fn confirm_delete(&mut self) {
        let Sheet::Delete { id } = self.sheet.clone() else { return };
        let g = self.win.global::<App>();
        if g.get_del_busy() {
            return;
        }
        let with_data = g.get_del_with_data();
        let running = self.snap.as_ref().and_then(|s| s.profile(&id)).is_some_and(|p| p.running);
        let name = self.snap.as_ref().and_then(|s| s.profile(&id)).map(|p| p.p.name.clone()).unwrap_or_default();
        g.set_del_busy(true);
        self.work(
            move |w| {
                if running {
                    // Ask it to quit, then wait (up to 15 s) until it has.
                    w.with(|l| l.quit(&id).map(|_| ()))?;
                    let until = Instant::now() + Duration::from_secs(15);
                    while Instant::now() < until {
                        std::thread::sleep(Duration::from_millis(400));
                        let still = w.with(|l| Ok(!l.snapshot().for_profile(&id).is_empty()))?;
                        if !still {
                            break;
                        }
                    }
                }
                w.with(|l| l.delete_profile(&id, with_data))
            },
            move |c, r| {
                let g = c.win.global::<App>();
                g.set_del_busy(false);
                match r {
                    Ok(()) => {
                        c.sheet = Sheet::None;
                        c.view = View::Auto;
                        c.toast(tf("delete.done", &[("name", &name)]), false);
                        c.run_doctor();
                        c.refresh();
                    }
                    Err(e) => g.set_del_error(e.into()),
                }
            },
        );
    }

    fn open_login(&mut self, id: String) {
        let Some(s) = self.snap.clone() else { return };
        let Some(p) = s.profile(&id) else { return };
        let g = self.win.global::<App>();
        g.set_login_link("".into());
        g.set_login_error("".into());
        g.set_login_sending(false);
        // Opened from the "waiting" banner: don't start a second sign-in.
        if let Some((pid, left)) = &s.app(p.p.provider).pending {
            if *pid == id {
                self.sheet = Sheet::Login { id, phase: LoginPhase::Waiting, deadline: Instant::now() + Duration::from_secs(*left as u64) };
                self.render();
                return;
            }
        }
        self.start_login(id);
    }

    fn start_login(&mut self, id: String) {
        self.sheet = Sheet::Login { id: id.clone(), phase: LoginPhase::Starting, deadline: Instant::now() + Duration::from_secs(LOGIN_SECS as u64) };
        self.win.global::<App>().set_login_error("".into());
        self.render();
        let id2 = id.clone();
        self.work(
            move |w| w.with(|l| l.login(&id2)),
            move |c, r| {
                if !matches!(&c.sheet, Sheet::Login { id: cur, .. } if *cur == id) {
                    return;
                }
                match r {
                    Ok(()) => c.sheet = Sheet::Login { id, phase: LoginPhase::Waiting, deadline: Instant::now() + Duration::from_secs(LOGIN_SECS as u64) },
                    Err(e) => {
                        c.win.global::<App>().set_login_error(e.into());
                        c.sheet = Sheet::Login { id, phase: LoginPhase::Error, deadline: Instant::now() };
                    }
                }
                c.refresh();
            },
        );
    }

    fn send_link(&mut self) {
        let Sheet::Login { id, .. } = self.sheet.clone() else { return };
        let g = self.win.global::<App>();
        let link = g.get_login_link().trim().to_string();
        if link.is_empty() || g.get_login_sending() {
            return;
        }
        g.set_login_sending(true);
        g.set_login_error("".into());
        self.work(
            move |w| {
                w.with(|l| {
                    let parsed = url::Url::parse(&link).map_err(|_| anyhow::anyhow!("That isn't a link."))?;
                    let p = l.config.find(&id).cloned().ok_or_else(|| anyhow::anyhow!("no such profile"))?;
                    if !p.provider.schemes().contains(&parsed.scheme()) {
                        anyhow::bail!("That isn't a {} link. It should start with {}://", p.provider.display_name(), p.provider.schemes()[0]);
                    }
                    router::PendingStore::new(&l.paths).log(&format!("{} → {} (pasted)", router::redact(&link), p.id));
                    l.deliver(&p.id, &link, true)
                })
            },
            |c, r| {
                let g = c.win.global::<App>();
                g.set_login_sending(false);
                match r {
                    Ok(()) => {
                        if let Sheet::Login { id, deadline, .. } = c.sheet.clone() {
                            c.sheet = Sheet::Login { id, phase: LoginPhase::Done, deadline };
                        }
                        c.refresh();
                    }
                    Err(e) => g.set_login_error(e.into()),
                }
            },
        );
    }

    fn tick_login(&mut self) {
        if let Sheet::Login { id, phase: LoginPhase::Waiting, deadline } = self.sheet.clone() {
            if Instant::now() >= deadline {
                self.sheet = Sheet::Login { id, phase: LoginPhase::Expired, deadline };
            }
            if let Some(s) = self.snap.clone() {
                self.render_sheet(&s);
            }
        }
    }

    fn open_route(&mut self, url: String) {
        self.work(
            move |w| {
                w.with(|l| {
                    let parsed = url::Url::parse(&url)?;
                    let provider = ProviderKind::from_scheme(parsed.scheme()).ok_or_else(|| anyhow::anyhow!("not a supported link"))?;
                    let res = l.decide_route(provider, &parsed);
                    let candidates = match res.decision {
                        Decision::Ask { candidates } => candidates,
                        Decision::To { profile, .. } => vec![profile],
                    };
                    Ok(RouteReq { url, redacted: res.redacted, provider, is_login: res.is_login, candidates })
                })
            },
            |c, r| match r {
                Ok(req) => {
                    let g = c.win.global::<App>();
                    g.set_route_busy("".into());
                    g.set_route_error("".into());
                    c.sheet = Sheet::Route(req);
                    c.render();
                    c.bring_to_front();
                }
                Err(e) => c.toast(e, true),
            },
        );
    }

    fn pick_route(&mut self, id: String) {
        let Sheet::Route(req) = self.sheet.clone() else { return };
        let g = self.win.global::<App>();
        g.set_route_busy(id.as_str().into());
        let name = self.snap.as_ref().and_then(|s| s.profile(&id)).map(|p| p.p.name.clone()).unwrap_or_default();
        self.work(
            move |w| w.with(|l| l.deliver_choice(&id, &req.url)),
            move |c, r| match r {
                Ok(()) => {
                    c.toast(tf("route.delivered", &[("name", &name)]), false);
                    c.close_sheet();
                    c.refresh();
                }
                Err(e) => {
                    let g = c.win.global::<App>();
                    g.set_route_busy("".into());
                    g.set_route_error(e.into());
                }
            },
        );
    }

    fn close_sheet(&mut self) {
        let was_route = matches!(self.sheet, Sheet::Route(_));
        self.sheet = Sheet::None;
        self.render();
        // Opened only to ask which profile gets a link: go away again.
        if was_route && self.opened_for_route {
            self.opened_for_route = false;
            let _ = self.win.hide();
            let _ = slint::quit_event_loop();
        }
    }

    // ───────── settings ─────────

    fn save_settings(&mut self, change: impl FnOnce(&mut Settings)) {
        let Some(base) = self.pending_settings.clone().or_else(|| self.snap.as_ref().map(|s| s.settings.clone())) else { return };
        let mut next = base;
        change(&mut next);
        i18n::set_language(&next.language);
        // Show the change right away; the refresh after the save confirms it.
        self.pending_settings = Some(next.clone());
        self.saves += 1;
        if let Some(snap) = self.snap.as_mut().and_then(Rc::get_mut) {
            snap.settings = next.clone();
        }
        self.render();
        self.work(
            move |w| w.with(|l| l.set_settings(next)),
            |c, r| {
                c.saves -= 1;
                if c.saves == 0 {
                    c.pending_settings = None;
                }
                match r {
                    Ok(problems) => problems.into_iter().for_each(|p| c.toast(p, true)),
                    Err(e) => c.toast(e, true),
                }
                c.refresh();
            },
        );
    }

    fn choose_app(&mut self, provider: ProviderKind) {
        let title = tf("settings.chooseApp", &[("app", provider.display_name())]);
        pick(false, title, move |path| {
            let Some(path) = path else { return };
            ctl(|c| {
                c.win.global::<App>().set_apps_busy(provider.id().into());
                c.work(
                    move |w| {
                        w.with(|l| {
                            l.set_app_override(provider, Some(path))?;
                            // The icon and routing follow the chosen app.
                            let _ = l.apply_routing();
                            let _ = l.sync_shortcuts();
                            Ok(())
                        })
                    },
                    |c, r| {
                        c.win.global::<App>().set_apps_busy("".into());
                        if let Err(e) = r {
                            c.toast(e, true);
                        }
                        c.refresh();
                    },
                );
            });
        });
    }

    /// Whether update checks include prereleases.
    fn beta_updates(&self) -> bool {
        let settings = self.pending_settings.as_ref().or(self.snap.as_ref().map(|s| &s.settings));
        settings.and_then(|s| s.beta_updates).unwrap_or_else(update::beta_by_default)
    }

    fn check_update(&mut self, manual: bool) {
        self.upd.phase = UpdPhase::Checking;
        self.upd.error = None;
        self.render();
        let beta = self.beta_updates();
        std::thread::spawn(move || {
            let r = update::check(beta);
            later(move |c| {
                match r {
                    Ok(found) => {
                        c.upd.phase = if found.is_some() { UpdPhase::Available } else { UpdPhase::Current };
                        c.upd.found = found;
                    }
                    Err(e) => {
                        c.upd.found = None;
                        if manual {
                            c.upd.error = Some(e);
                            c.upd.phase = UpdPhase::Error;
                        } else {
                            c.upd.phase = UpdPhase::Idle;
                        }
                    }
                }
                c.render();
            });
        });
    }

    fn install_update(&mut self) {
        let Some(found) = self.upd.found.clone() else { return };
        let Some(kind) = update::install_kind() else {
            open_website("releases");
            return;
        };
        self.upd.phase = UpdPhase::Downloading;
        self.upd.progress = None;
        self.upd.error = None;
        self.render();
        std::thread::spawn(move || {
            let mut last = Instant::now() - Duration::from_secs(1);
            let r = update::download(&found, |done, total| {
                if last.elapsed() >= Duration::from_millis(100) {
                    last = Instant::now();
                    let p = total.map(|t| (done as f32 / t as f32).min(1.0));
                    later(move |c| {
                        c.upd.progress = p;
                        c.render();
                    });
                }
            })
            .and_then(|bytes| update::verify(&bytes, &found).map(|_| bytes));
            let r = r.and_then(|bytes| {
                later(|c| {
                    c.upd.phase = UpdPhase::Installing;
                    c.render();
                });
                update::install(kind, &bytes, &found.version)
            });
            match r {
                Ok(()) => update::restart(kind),
                Err(e) => later(move |c| {
                    if e == update::CANCELLED {
                        c.upd.phase = UpdPhase::Available;
                    } else {
                        c.upd.error = Some(e);
                        c.upd.phase = UpdPhase::Error;
                    }
                    c.render();
                }),
            }
        });
    }

    fn apply_fix(&mut self, id: String) {
        let Some(check) = self.checks.as_ref().and_then(|cs| cs.iter().find(|c| c.id == id)).cloned() else { return };
        let Some(fix) = check.fix else { return };
        if let Fix::ChooseApp { .. } = fix {
            self.view = View::Settings;
            self.render();
            return;
        }
        let restart = matches!(fix, Fix::Restart { .. });
        self.win.global::<App>().set_fixing(id.as_str().into());
        self.work(
            move |w| {
                if restart {
                    // Restarts wait between launches: run them on their own
                    // launcher and report when they're done.
                    std::thread::spawn(move || {
                        let r = Launcher::open().and_then(|mut l| doctor::apply(&mut l, &fix)).map_err(|e| format!("{e:#}"));
                        later(move |c| {
                            if let Err(e) = r {
                                c.toast(e, true);
                            }
                            c.refresh();
                            c.run_doctor();
                        });
                    });
                    Ok(())
                } else {
                    w.with(|l| doctor::apply(l, &fix))
                }
            },
            move |c, r| {
                c.win.global::<App>().set_fixing("".into());
                match r {
                    Ok(()) => {
                        c.toast(t(if restart { "diag.restartStarted" } else { "diag.fixed" }), false);
                        if !restart {
                            c.run_doctor();
                        }
                    }
                    Err(e) => c.toast(e, true),
                }
                c.refresh();
            },
        );
    }

    fn select(&mut self, view: View) {
        if let View::Profile(id) = &view {
            if self.current_view() != view {
                self.win.global::<App>().set_detail_advanced(false);
                self.load_size(id.clone());
            }
        }
        if view == View::Diagnostics {
            self.load_log();
        }
        self.view = view;
        self.render();
    }

    fn load_size(&mut self, id: String) {
        let Some(s) = &self.snap else { return };
        let Some(root) = s.profile(&id).filter(|p| p.p.isolated).map(|p| PathBuf::from(&p.data_dir)) else { return };
        // `None` while counting; the page shows "…" until it is `Some`.
        self.sizes.insert(id.clone(), None);
        std::thread::spawn(move || {
            let size = vl_core::dir_size(&root, 0);
            later(move |c| {
                c.sizes.insert(id, Some(size));
                c.render();
            });
        });
    }

    fn step(&mut self, by: i32) {
        let Some(s) = self.snap.clone() else { return };
        let ids: Vec<String> = s.visible().iter().map(|p| p.p.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let at = match self.current_view() {
            View::Profile(id) => ids.iter().position(|x| *x == id).map_or(-1, |i| i as i32),
            _ => -1,
        };
        let next = (at + by).clamp(0, ids.len() as i32 - 1) as usize;
        self.select(View::Profile(ids[next].clone()));
    }

    fn bring_to_front(&self) {
        use slint::winit_030::WinitWindowAccessor;
        let _ = self.win.show();
        self.win.window().with_winit_window(|w| {
            w.set_minimized(false);
            w.focus_window();
        });
    }

    fn finish_onboarding(&mut self) {
        if self.snap.as_ref().is_some_and(|s| !s.settings.onboarded) {
            self.save_settings(|s| s.onboarded = true);
        }
    }
}

// ───────────────────────── helpers ─────────────────────────

fn color(hex: &str) -> Color {
    let [r, g, b] = icon::parse_hex(hex);
    Color::from_rgb_u8(r, g, b)
}

fn hex(c: Color) -> String {
    let wanted = format!("#{:02X}{:02X}{:02X}", c.red(), c.green(), c.blue());
    // Keep the palette's spelling when it is one of ours.
    COLORS.iter().find(|x| x.eq_ignore_ascii_case(&wanted)).map(|x| x.to_string()).unwrap_or(wanted)
}

fn slug(name: &str) -> String {
    let s: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "profile".into()
    } else {
        s
    }
}

/// `NAME=value` lines; blank lines and `#` comments are skipped. The error is
/// the (1-based) line that isn't one.
fn parse_env(text: &str) -> Result<std::collections::BTreeMap<String, String>, usize> {
    let mut env = std::collections::BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { return Err(i + 1) };
        let k = k.trim();
        let ok = k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            return Err(i + 1);
        }
        env.insert(k.to_string(), v.to_string());
    }
    Ok(env)
}

fn quote(s: &str) -> String {
    if s.chars().any(|c| c.is_whitespace() || "\"'$`\\".contains(c)) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`"))
    } else {
        s.to_string()
    }
}

/// Show a file or folder in the system file manager.
fn reveal(path: &str) -> Result<(), String> {
    let p = PathBuf::from(path);
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
    r.map(|_| ()).map_err(|e| e.to_string())
}

/// Open the project's web page (fixed addresses only).
fn open_website(page: &str) {
    let url = match page {
        "issues" => "https://github.com/Gloridust/VibeLauncher/issues",
        "releases" => "https://github.com/Gloridust/VibeLauncher/releases",
        "slint" => "https://slint.dev",
        _ => "https://github.com/Gloridust/VibeLauncher",
    };
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Ask for a file or folder. The answer comes back on the UI thread.
fn pick(directory: bool, title: String, done: impl FnOnce(Option<PathBuf>) + Send + 'static) {
    // macOS needs its panels on the main thread; elsewhere a thread keeps the
    // window responsive while the (portal) dialog is open.
    #[cfg(target_os = "macos")]
    {
        let d = rfd::FileDialog::new().set_title(&title);
        let r = if directory { d.pick_folder() } else { d.pick_file() };
        done(r);
    }
    #[cfg(not(target_os = "macos"))]
    std::thread::spawn(move || {
        let d = rfd::FileDialog::new().set_title(&title);
        let r = if directory { d.pick_folder() } else { d.pick_file() };
        let _ = slint::invoke_from_event_loop(move || done(r));
    });
}

fn copy_text(text: &str) -> bool {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).is_ok()
}

fn paste_text() -> Option<String> {
    arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok()
}

fn provider(s: &str) -> Option<ProviderKind> {
    ProviderKind::parse(s)
}

// ───────────────────────── wiring ─────────────────────────

fn route_arg(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "--choose-route")?;
    args.get(i + 1).cloned()
}

/// Show this theme whatever the setting says (headless screenshots).
#[cfg(feature = "selftest")]
pub fn force_theme(mode: &str) {
    let mode = mode.to_string();
    ctl(move |c| {
        c.theme_override = Some(mode);
        c.render();
    });
}

/// A later start handed us its arguments.
pub fn on_second_start(args: Vec<String>) {
    later(move |c| {
        if let Some(url) = route_arg(&args) {
            if c.snap.is_some() {
                c.open_route(url);
            } else {
                c.pending_route = Some(url);
            }
        }
        c.bring_to_front();
    });
}

pub fn install(win: &AppWindow, args: &[String]) {
    let tx = spawn_worker();
    let opened_for_route = route_arg(args).is_some();
    // Before the first snapshot: the window shows the right language and
    // theme from its first frame.
    if let Ok(l) = Launcher::open() {
        i18n::set_language(&l.config.settings.language);
        win.global::<Theme>().set_mode(l.config.settings.theme.as_str().into());
    }
    win.global::<Theme>().set_mac(cfg!(target_os = "macos"));
    win.global::<Theme>().set_windows(cfg!(windows));
    let tr = win.global::<Tr>();
    tr.set_lang(i18n::lang().into());
    tr.on_t(|lang, key| i18n::t_in(&lang, &key).into());
    tr.on_t1(|lang, key, k, v| i18n::fill(&i18n::t_in(&lang, &key), &[(&k, &v)]).into());

    let c = Ctl {
        win: win.clone_strong(),
        tx,
        snap: None,
        load_error: None,
        view: View::Auto,
        sheet: Sheet::None,
        checks: None,
        log: None,
        collapsed: BTreeSet::new(),
        toasts: Vec::new(),
        toast_seq: 0,
        opened_for_route,
        pending_route: route_arg(args),
        upd: Upd { phase: UpdPhase::Idle, found: None, progress: None, error: None },
        auto_checked: false,
        sizes: HashMap::new(),
        busy: String::new(),
        images: HashMap::new(),
        doctor_at: Instant::now(),
        focused: true,
        theme_override: None,
        pending_settings: None,
        saves: 0,
        timers: Vec::new(),
    };
    CTL.with(|x| *x.borrow_mut() = Some(c));
    wire(win);
    ctl(|c| {
        c.refresh();
        c.run_doctor();
        // Poll while the window is in front.
        let poll = Timer::default();
        poll.start(TimerMode::Repeated, POLL, || {
            ctl(|c| {
                if c.focused {
                    c.refresh();
                    if c.doctor_at.elapsed() > DOCTOR_EVERY {
                        c.run_doctor();
                    }
                }
            });
        });
        let second = Timer::default();
        second.start(TimerMode::Repeated, Duration::from_secs(1), || {
            ctl(|c| c.tick_login());
        });
        c.timers.push(poll);
        c.timers.push(second);
    });
}

fn wire(win: &AppWindow) {
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    let g = win.global::<App>();

    // Refresh everything when the window comes back to the front; follow
    // maximising for the title bar buttons.
    win.window().on_winit_window_event(|w, ev| {
        match ev {
            winit::event::WindowEvent::Focused(f) => {
                let f = *f;
                ctl(|c| {
                    let gained = f && !c.focused;
                    c.focused = f;
                    if gained {
                        c.work(
                            |w| {
                                let _ = w.with(|l| {
                                    l.refresh_installs();
                                    Ok(())
                                });
                            },
                            |c, _| {
                                c.refresh();
                                c.run_doctor();
                            },
                        );
                    }
                });
            }
            winit::event::WindowEvent::Resized(_) => {
                let max = w.with_winit_window(|w| w.is_maximized()).unwrap_or(false);
                ctl(|c| c.win.global::<App>().set_maximized(max));
            }
            _ => {}
        }
        EventResult::Propagate
    });

    let weak = win.as_weak();
    let winit_do = move |f: &dyn Fn(&winit::window::Window)| {
        if let Some(w) = weak.upgrade() {
            w.window().with_winit_window(|ww| f(ww));
        }
    };
    let wd = winit_do.clone();
    g.on_win_minimize(move || wd(&|w| w.set_minimized(true)));
    let wd = winit_do.clone();
    g.on_win_maximize(move || wd(&|w| w.set_maximized(!w.is_maximized())));
    g.on_win_close(|| {
        let _ = slint::quit_event_loop();
    });
    g.on_select(|kind, id| {
        ctl(|c| match kind.as_str() {
            "profile" => c.select(View::Profile(id.to_string())),
            "diagnostics" => c.select(View::Diagnostics),
            "settings" => c.select(View::Settings),
            "next" => c.step(1),
            "prev" => c.step(-1),
            _ => c.select(View::Auto),
        });
    });
    g.on_new_profile(|pv| {
        ctl(|c| c.open_profile_sheet(None, provider(&pv)));
    });
    g.on_toggle_section(|pv| {
        ctl(|c| {
            if let Some(pv) = provider(&pv) {
                if !c.collapsed.remove(&pv) {
                    c.collapsed.insert(pv);
                }
                c.render();
            }
        });
    });
    g.on_launch(|id| {
        let id = id.to_string();
        ctl(|c| c.act("open", move |l| l.launch(&id, None), None));
    });
    g.on_sign_in(|id| {
        ctl(|c| c.open_login(id.to_string()));
    });
    g.on_quit(|id| {
        let id = id.to_string();
        ctl(|c| c.act("quit", move |l| l.quit(&id).map(|_| ()), None));
    });
    g.on_restart(|id| {
        let id = id.to_string();
        ctl(|c| {
            c.busy = "restart".into();
            c.render();
            std::thread::spawn(move || {
                let r = Launcher::open().and_then(|mut l| doctor::apply(&mut l, &Fix::Restart { profiles: vec![id] })).map_err(|e| format!("{e:#}"));
                later(move |c| {
                    c.busy.clear();
                    if let Err(e) = r {
                        c.toast(e, true);
                    }
                    c.refresh();
                    c.run_doctor();
                });
            });
            c.toast(t("diag.restartStarted"), false);
        });
    });
    g.on_cancel_login(|| {
        ctl(|c| {
            let pv = match &c.sheet {
                Sheet::Login { id, .. } => c.snap.as_ref().and_then(|s| s.profile(id)).map(|p| p.p.provider),
                _ => None,
            }
            .or_else(|| match c.current_view() {
                View::Profile(id) => c.snap.as_ref().and_then(|s| s.profile(&id)).map(|p| p.p.provider),
                _ => None,
            });
            if let Some(pv) = pv {
                if matches!(c.sheet, Sheet::Login { .. }) {
                    c.sheet = Sheet::None;
                }
                c.act("", move |l| l.cancel_login(pv), None);
            }
        });
    });
    g.on_edit(|id| {
        ctl(|c| c.open_profile_sheet(Some(id.to_string()), None));
    });
    g.on_delete(|id| {
        ctl(|c| c.open_delete(id.to_string()));
    });
    g.on_show_data(|id| {
        ctl(|c| {
            if let Some(p) = c.snap.as_ref().and_then(|s| s.profile(&id)) {
                if let Err(e) = reveal(&p.data_dir) {
                    c.toast(e, true);
                }
            }
        });
    });
    g.on_copy_command(|id| {
        ctl(|c| {
            let Some(s) = &c.snap else { return };
            let cmd = format!("{} launch {}", quote(&s.cli), id);
            if copy_text(&cmd) {
                c.toast(t("copied"), false);
            } else {
                c.toast(cmd, false);
            }
        });
    });
    g.on_move_profile(|id, by| {
        ctl(|c| {
            let Some(s) = c.snap.clone() else { return };
            let Some(p) = s.profile(&id) else { return };
            let same: Vec<String> = s.profiles.iter().filter(|x| x.p.provider == p.p.provider).map(|x| x.p.id.clone()).collect();
            let Some(i) = same.iter().position(|x| *x == p.p.id) else { return };
            let j = i as i32 + by;
            if j < 0 || j >= same.len() as i32 {
                return;
            }
            let other = same[j as usize].clone();
            let mut ids: Vec<String> = s.profiles.iter().map(|x| x.p.id.clone()).collect();
            let a = ids.iter().position(|x| *x == p.p.id).unwrap();
            let b = ids.iter().position(|x| *x == other).unwrap();
            ids.swap(a, b);
            c.act("", move |l| l.reorder(&ids), None);
        });
    });
    g.on_set_shortcut(|id, on| {
        let id = id.to_string();
        ctl(|c| {
            c.act(
                "",
                move |l| {
                    let p = l.config.find(&id).cloned().ok_or_else(|| anyhow::anyhow!("no such profile"))?;
                    l.update_profile(
                        &id,
                        ProfileInput {
                            name: p.name,
                            provider: p.provider,
                            color: p.color,
                            icon: Some(p.icon),
                            data_root: p.data_root,
                            env: p.env,
                            extra_args: p.extra_args,
                            shortcut: on,
                        },
                    )
                    .map(|_| ())
                },
                None,
            )
        });
    });
    g.on_repair_shortcuts(|| {
        ctl(|c| c.act("", |l| doctor::apply(l, &Fix::SyncShortcuts), None));
    });
    g.on_recheck(|| {
        ctl(|c| {
            if c.snap.is_none() {
                c.refresh();
                return;
            }
            c.win.global::<App>().set_checking(true);
            c.doctor_at = Instant::now();
            c.work(
                |w| w.with(|l| Ok(doctor::run(l))),
                |c, r| {
                    c.win.global::<App>().set_checking(false);
                    if let Ok(checks) = r {
                        c.checks = Some(checks);
                    }
                    c.load_log();
                    c.render();
                },
            );
        });
    });
    g.on_fix(|id| {
        ctl(|c| c.apply_fix(id.to_string()));
    });
    g.on_set_language(|v| {
        ctl(|c| c.save_settings(|s| s.language = v.to_string()));
    });
    g.on_set_theme(|v| {
        ctl(|c| c.save_settings(|s| s.theme = v.to_string()));
    });
    g.on_find_apps(|| {
        ctl(|c| {
            c.win.global::<App>().set_apps_busy("refresh".into());
            c.work(
                |w| {
                    w.with(|l| {
                        l.refresh_installs();
                        Ok(())
                    })
                },
                |c, r| {
                    c.win.global::<App>().set_apps_busy("".into());
                    if let Err(e) = r {
                        c.toast(e, true);
                    }
                    c.refresh();
                },
            );
        });
    });
    g.on_choose_app(|pv| {
        if let Some(pv) = provider(&pv) {
            ctl(|c| c.choose_app(pv));
        }
    });
    g.on_reset_app(|pv| {
        if let Some(pv) = provider(&pv) {
            ctl(|c| {
                c.act(
                    "",
                    move |l| {
                        l.set_app_override(pv, None)?;
                        let _ = l.apply_routing();
                        let _ = l.sync_shortcuts();
                        Ok(())
                    },
                    None,
                )
            });
        }
    });
    g.on_set_route(|pv, on| {
        ctl(|c| {
            c.save_settings(|s| match provider(&pv) {
                Some(ProviderKind::Claude) => s.route_claude = on,
                Some(ProviderKind::ChatGpt) => s.route_chatgpt = on,
                None => {}
            })
        });
    });
    g.on_set_stagger(|n| {
        ctl(|c| c.save_settings(|s| s.stagger_seconds = n.clamp(0, 30) as u32));
    });
    g.on_set_check_updates(|on| {
        ctl(|c| c.save_settings(|s| s.check_updates = on));
    });
    g.on_set_beta_updates(|on| {
        ctl(|c| {
            c.save_settings(|s| s.beta_updates = Some(on));
            // Show what the new channel offers, unless an update is underway.
            if !matches!(c.upd.phase, UpdPhase::Checking | UpdPhase::Downloading | UpdPhase::Installing) {
                c.check_update(false);
            }
        });
    });
    g.on_update_action(|| {
        ctl(|c| {
            if c.upd.found.is_some() && !matches!(c.upd.phase, UpdPhase::Checking | UpdPhase::Current) {
                c.install_update();
            } else {
                c.check_update(true);
            }
        });
    });
    g.on_open_website(|page| open_website(&page));
    g.on_reveal_config(|| {
        ctl(|c| {
            if let Some(s) = &c.snap {
                if let Err(e) = reveal(&s.config_file) {
                    c.toast(e, true);
                }
            }
        });
    });
    g.on_welcome_skip(|| {
        ctl(|c| {
            c.finish_onboarding();
            c.view = View::Auto;
        });
    });
    g.on_welcome_start(|| {
        ctl(|c| {
            c.finish_onboarding();
            c.open_profile_sheet(None, None);
        });
    });

    g.on_sheet_close(|| {
        ctl(|c| c.close_sheet());
    });
    g.on_ps_name_edited(|_| {
        ctl(|c| {
            if let Some(s) = c.snap.clone() {
                c.render_sheet(&s);
            }
        });
    });
    g.on_ps_choose_provider(|pv| {
        ctl(|c| {
            if let (Sheet::New { .. }, Some(pv)) = (&c.sheet, provider(&pv)) {
                c.sheet = Sheet::New { provider: pv };
                c.render();
            }
        });
    });
    g.on_ps_choose_color(|col| {
        ctl(|c| c.win.global::<App>().set_ps_color(col));
    });
    g.on_ps_pick_folder(|| {
        pick(true, t("sheet.dataFolder"), |path| {
            if let Some(p) = path {
                ctl(|c| c.win.global::<App>().set_ps_data_root(p.display().to_string().into()));
            }
        });
    });
    g.on_ps_save(|| {
        ctl(|c| c.save_profile());
    });
    g.on_del_confirm_clicked(|| {
        ctl(|c| c.confirm_delete());
    });
    g.on_login_again(|| {
        ctl(|c| {
            if let Sheet::Login { id, .. } = c.sheet.clone() {
                c.start_login(id);
            }
        });
    });
    g.on_login_send(|| {
        ctl(|c| c.send_link());
    });
    g.on_login_paste(|| {
        ctl(|c| {
            if let Some(text) = paste_text() {
                c.win.global::<App>().set_login_link(text.trim().into());
            }
        });
    });
    g.on_route_pick(|id| {
        ctl(|c| c.pick_route(id.to_string()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_lines() {
        let e = parse_env("A=1\n# note\n\nB_2 = x=y").unwrap();
        assert_eq!(e.get("A").map(String::as_str), Some("1"));
        assert_eq!(e.get("B_2").map(String::as_str), Some(" x=y"));
        assert_eq!(parse_env("A=1\nnot a pair"), Err(2));
        assert_eq!(parse_env("1A=x"), Err(1));
    }

    #[test]
    fn slugs_and_quotes() {
        assert_eq!(slug("My Work!"), "my-work");
        assert_eq!(slug("工作"), "profile");
        assert_eq!(quote("/usr/bin/vibelauncher"), "/usr/bin/vibelauncher");
        assert_eq!(quote("/a b/c"), "\"/a b/c\"");
    }

    #[test]
    fn colors_round_trip() {
        assert_eq!(hex(color("#d97757")), "#D97757");
        assert_eq!(hex(color("#123456")), "#123456");
    }
}
