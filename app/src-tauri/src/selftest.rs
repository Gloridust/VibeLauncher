//! Checking the window without a screen (`--features selftest`), on Slint's
//! software renderer in memory with the real window logic:
//!
//! - `VL_SHOT=<dir>` saves each page and sheet as a PNG, in light and dark.
//!   It only reads; nothing is launched or changed.
//! - `VL_SELFTEST_OUT=<file>` clicks through the window the way a person
//!   would — create, open, quit, sign in with a pasted link, rename, run
//!   Diagnostics, switch to Chinese and dark, delete — and writes one line per
//!   step. The CI script around it (`.github/scripts/window-check.sh`) runs
//!   it against stand-in apps and checks the results with the command.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{EventLoopProxy, Platform, WindowAdapter};
use slint::ComponentHandle;

use crate::{App, AppWindow};

type Task = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Queue {
    tasks: Mutex<VecDeque<Task>>,
    wake: Condvar,
}

struct Proxy(Arc<Queue>);
impl EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        Ok(())
    }
    fn invoke_from_event_loop(&self, event: Box<dyn FnOnce() + Send>) -> Result<(), slint::EventLoopError> {
        self.0.tasks.lock().unwrap().push_back(event);
        self.0.wake.notify_one();
        Ok(())
    }
}

struct Headless {
    window: Rc<MinimalSoftwareWindow>,
    queue: Arc<Queue>,
}

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(Proxy(self.queue.clone())))
    }
}

/// Run queued tasks and timers for `d`.
fn pump(q: &Queue, d: Duration) {
    let until = Instant::now() + d;
    while Instant::now() < until {
        slint::platform::update_timers_and_animations();
        let task = {
            let g = q.tasks.lock().unwrap();
            let (mut g, _) = q.wake.wait_timeout(g, Duration::from_millis(10)).unwrap();
            g.pop_front()
        };
        if let Some(t) = task {
            t();
        }
    }
}

fn save(win: &MinimalSoftwareWindow, w: u32, h: u32, path: PathBuf) {
    win.request_redraw();
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    win.draw_if_needed(|r| {
        r.render(&mut buf, w as usize);
    });
    let raw: Vec<u8> = buf.iter().flat_map(|p| [p.red, p.green, p.blue, p.alpha]).collect();
    if let Some(img) = image::RgbaImage::from_raw(w, h, raw) {
        let _ = img.save(&path);
        println!("{}", path.display());
    }
}

fn setup() -> (Rc<MinimalSoftwareWindow>, Arc<Queue>, u32, u32) {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let queue = Arc::new(Queue::default());
    slint::platform::set_platform(Box::new(Headless { window: window.clone(), queue: queue.clone() })).unwrap();
    let (w, h) = (
        std::env::var("VL_SHOT_W").ok().and_then(|v| v.parse().ok()).unwrap_or(980u32),
        std::env::var("VL_SHOT_H").ok().and_then(|v| v.parse().ok()).unwrap_or(660u32),
    );
    window.set_size(slint::PhysicalSize::new(w, h));
    (window, queue, w, h)
}

pub fn run(dir: PathBuf) -> std::process::ExitCode {
    let _ = std::fs::create_dir_all(&dir);
    let (window, queue, w, h) = setup();
    let ui = AppWindow::new().unwrap();
    crate::app::install(&ui, &[]);
    ui.show().unwrap();
    let g = ui.global::<App>();
    let until = Instant::now() + Duration::from_secs(15);
    while !g.get_loaded() && Instant::now() < until {
        pump(&queue, Duration::from_millis(100));
    }
    pump(&queue, Duration::from_millis(1500));

    let first_profile = (0..slint::Model::row_count(&g.get_side_rows()))
        .filter_map(|i| slint::Model::row_data(&g.get_side_rows(), i))
        .find(|r| !r.header)
        .map(|r| r.id.to_string());

    for mode in ["light", "dark"] {
        crate::app::force_theme(mode);
        let shot = |name: &str| {
            pump(&queue, Duration::from_millis(400));
            save(&window, w, h, dir.join(format!("{name}-{mode}.png")));
        };
        if let Some(id) = &first_profile {
            g.invoke_select("profile".into(), id.as_str().into());
            shot("profile");
            g.set_detail_advanced(true);
            shot("profile-advanced");
            g.set_detail_advanced(false);
        }
        g.invoke_select("diagnostics".into(), "".into());
        shot("diagnostics");
        g.invoke_select("settings".into(), "".into());
        shot("settings");
        g.invoke_new_profile("".into());
        shot("sheet-new");
        g.invoke_sheet_close();
        if let Some(id) = &first_profile {
            g.invoke_select("profile".into(), id.as_str().into());
            g.invoke_delete(id.as_str().into());
            shot("sheet-delete");
            g.invoke_sheet_close();
        }
        g.set_page("welcome".into());
        shot("welcome");
    }
    std::process::ExitCode::SUCCESS
}

// ───────────────────────── click-through ─────────────────────────

const NAME: &str = "UI Test";
const RENAMED: &str = "UI Test Renamed";
const LINK: &str = "claude://login/callback?code=UISECRET";
/// The profile made for the app added with "Other app…" (`VL_SELFTEST_PICK`).
const ADDED: &str = "Notes Work";

struct T<'a> {
    ui: &'a AppWindow,
    queue: &'a Queue,
}

impl T<'_> {
    fn app(&self) -> App<'_> {
        self.ui.global::<App>()
    }
    fn wait(&self, what: &str, ms: u64, mut cond: impl FnMut(&App) -> bool) -> Result<(), String> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            if cond(&self.app()) {
                pump(self.queue, Duration::from_millis(150));
                return Ok(());
            }
            if Instant::now() > end {
                return Err(format!("timed out waiting for {what}"));
            }
            pump(self.queue, Duration::from_millis(100));
        }
    }
    fn side_id(&self, name: &str) -> Option<String> {
        let rows = self.app().get_side_rows();
        (0..slint::Model::row_count(&rows)).filter_map(|i| slint::Model::row_data(&rows, i)).find(|r| !r.header && r.label == name).map(|r| r.id.to_string())
    }
    fn shown(&self) -> String {
        let g = self.app();
        if g.get_page() == "profile" {
            g.get_profile().name.to_string()
        } else {
            String::new()
        }
    }
}

fn step(t: &T, name: &str) -> Result<(), String> {
    let g = t.app();
    match name {
        "window shows the profiles" => {
            t.wait("the Work profile in the sidebar", 20_000, |_| t.side_id("Work").is_some())?;
            t.wait("the Client A profile in the sidebar", 20_000, |_| t.side_id("Client A").is_some())
        }
        "create a profile" => {
            g.invoke_new_profile("".into());
            t.wait("the new-profile sheet", 5_000, |g| g.get_sheet() == "new")?;
            g.set_ps_name(NAME.into());
            g.invoke_ps_name_edited(NAME.into());
            g.invoke_ps_choose_provider("claude".into());
            g.invoke_ps_save();
            t.wait("the new profile in the sidebar", 20_000, |_| t.side_id(NAME).is_some())?;
            t.wait("the new profile to be shown", 20_000, |_| t.shown() == NAME)
        }
        "open it" => {
            g.invoke_launch(g.get_profile().id);
            t.wait("it to be running", 20_000, |g| g.get_profile().running)
        }
        "quit it" => {
            g.invoke_quit(g.get_profile().id);
            t.wait("it to stop", 30_000, |g| !g.get_profile().running)
        }
        "sign in with a pasted link" => {
            g.invoke_sign_in(g.get_profile().id);
            t.wait("the sign-in to start", 20_000, |g| g.get_sheet() == "login" && g.get_login_phase() == "waiting")?;
            g.set_login_link(LINK.into());
            g.invoke_login_send();
            t.wait("the link to be delivered", 20_000, |g| g.get_login_phase() == "done")?;
            g.invoke_sheet_close();
            t.wait("the sheet to close", 5_000, |g| g.get_sheet() == "")
        }
        "quit after signing in" => {
            t.wait("it to be running", 20_000, |g| g.get_profile().running)?;
            g.invoke_quit(g.get_profile().id);
            t.wait("it to stop", 30_000, |g| !g.get_profile().running)
        }
        "rename it" => {
            g.invoke_edit(g.get_profile().id);
            t.wait("the edit sheet", 5_000, |g| g.get_sheet() == "edit")?;
            g.set_ps_name(RENAMED.into());
            g.invoke_ps_save();
            t.wait("the new name", 20_000, |_| t.shown() == RENAMED && t.side_id(RENAMED).is_some())
        }
        "diagnostics" => {
            g.invoke_select("diagnostics".into(), "".into());
            t.wait("the checks", 30_000, |g| g.get_page() == "diagnostics" && g.get_checks_loaded())?;
            g.invoke_recheck();
            t.wait("the checks again", 30_000, |g| g.get_checks_loaded() && !g.get_checking())
        }
        "settings show updates" => {
            g.invoke_select("settings".into(), "".into());
            // The automatic check may still be running; any settled state will do.
            t.wait("the Updates row", 40_000, |g| g.get_page() == "settings" && !g.get_update_busy() && !g.get_update_sub().is_empty())
        }
        "switch to Chinese and dark" => {
            g.invoke_set_language("zh".into());
            t.wait("the interface in Chinese", 10_000, |_| t.ui.global::<crate::Tr>().get_lang() == "zh")?;
            g.invoke_set_theme("dark".into());
            t.wait("the dark theme", 10_000, |g| g.get_theme() == "dark")
        }
        "delete it" => {
            let id = t.side_id(RENAMED).ok_or("the renamed profile isn't in the sidebar")?;
            g.invoke_select("profile".into(), id.as_str().into());
            t.wait("it to be shown", 5_000, |_| t.shown() == RENAMED)?;
            g.invoke_delete(id.into());
            t.wait("the delete sheet", 5_000, |g| g.get_sheet() == "delete")?;
            g.invoke_del_confirm_clicked();
            t.wait("it to be gone", 30_000, |g| t.side_id(RENAMED).is_none() && g.get_sheet() == "")
        }
        // Needs an Electron-looking stand-in in VL_SELFTEST_PICK (window-check.sh).
        "add another app" | "open its profile" | "remove the app" if std::env::var_os("VL_SELFTEST_PICK").is_none() => Ok(()),
        "add another app" => {
            g.invoke_new_profile("".into());
            t.wait("the new-profile sheet", 5_000, |g| g.get_sheet() == "new")?;
            g.set_ps_name(ADDED.into());
            g.invoke_ps_name_edited(ADDED.into());
            g.invoke_ps_other_app();
            t.wait("the app to be checked", 30_000, |g| !g.get_ps_app_checking() && (g.get_ps_provider() == "+new" || g.get_ps_app_note_warn()))?;
            if g.get_ps_app_note_warn() {
                return Err(format!("the app was turned away: {}", g.get_ps_app_note()));
            }
            g.invoke_ps_save();
            t.wait("its profile in the sidebar", 20_000, |_| t.side_id(ADDED).is_some())?;
            t.wait("its profile to be shown", 20_000, |_| t.shown() == ADDED)
        }
        "open its profile" => {
            if g.get_profile().can_sign_in {
                return Err("an added app's profile offers to route sign-in links".into());
            }
            g.invoke_launch(g.get_profile().id);
            t.wait("it to be running", 20_000, |g| g.get_profile().running)?;
            g.invoke_quit(g.get_profile().id);
            t.wait("it to stop", 30_000, |g| !g.get_profile().running)
        }
        "remove the app" => {
            let id = t.side_id(ADDED).ok_or("the added app's profile isn't in the sidebar")?;
            g.invoke_delete(id.into());
            t.wait("the delete sheet", 5_000, |g| g.get_sheet() == "delete")?;
            g.invoke_del_confirm_clicked();
            t.wait("the profile to be gone", 30_000, |g| t.side_id(ADDED).is_none() && g.get_sheet() == "")?;
            g.invoke_select("settings".into(), "".into());
            let custom = |g: &App| {
                let apps = g.get_apps();
                (0..slint::Model::row_count(&apps)).filter_map(|i| slint::Model::row_data(&apps, i)).find(|a| a.custom).map(|a| a.provider.to_string())
            };
            t.wait("the app in Settings", 10_000, |g| custom(g).is_some())?;
            g.invoke_remove_app(custom(&g).unwrap_or_default().into());
            t.wait("the app to be removed", 20_000, |g| custom(g).is_none())
        }
        _ => Err(format!("unknown step {name}")),
    }
}

pub fn click_through(out: PathBuf) -> std::process::ExitCode {
    use std::io::Write;
    let mut report = match std::fs::OpenOptions::new().create(true).append(true).open(&out) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{}: {e}", out.display());
            return std::process::ExitCode::FAILURE;
        }
    };
    let mut line = |s: &str| {
        let _ = writeln!(report, "{s}");
        println!("{s}");
    };
    let shots = std::env::var_os("VL_SHOT").map(PathBuf::from);
    if let Some(d) = &shots {
        let _ = std::fs::create_dir_all(d);
    }
    let (window, queue, w, h) = setup();
    let ui = AppWindow::new().unwrap();
    crate::app::install(&ui, &[]);
    ui.show().unwrap();
    let t = T { ui: &ui, queue: &queue };
    line("start");
    let mut ok = true;
    let steps = [
        "window shows the profiles",
        "create a profile",
        "open it",
        "quit it",
        "sign in with a pasted link",
        "quit after signing in",
        "rename it",
        "diagnostics",
        "settings show updates",
        "switch to Chinese and dark",
        "delete it",
        "add another app",
        "open its profile",
        "remove the app",
    ];
    for (i, name) in steps.iter().enumerate() {
        let r = step(&t, name);
        if let Some(d) = &shots {
            save(&window, w, h, d.join(format!("step-{:02}.png", i + 1)));
        }
        match r {
            Ok(()) => line(&format!("ok {name}")),
            Err(e) => {
                line(&format!("FAIL {name}: {e}"));
                let g = t.app();
                line(&format!("page: {} sheet: {} profile: {}", g.get_page(), g.get_sheet(), g.get_profile().name));
                ok = false;
                break;
            }
        }
    }
    line("end");
    if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
