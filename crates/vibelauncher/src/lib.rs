//! The `vibelauncher` command — the small program every shortcut and link handler runs.
//!
//! It links no GUI toolkit, so opening a profile costs a few milliseconds and a
//! few megabytes. Running it with no arguments opens the manager window
//! (`vibelauncher-gui`).

use std::process::ExitCode;

use anyhow::{anyhow, bail, Result};
use vl_core::{doctor, router, scheme, Decision, Launcher, ProfileInput, ProviderKind};

const HELP: &str = "\
VibeLauncher — run Claude and ChatGPT desktop with separate profiles.

USAGE
  vibelauncher                         Open the VibeLauncher window
  vibelauncher list [--json]           List profiles and whether they are running
  vibelauncher launch <profile> [URL]  Open a profile (and hand it a link)
  vibelauncher login <profile>         Open a profile to sign in; the sign-in link
                                       will come back to it (for 5 minutes)
  vibelauncher quit <profile>          Ask a profile's app to quit
  vibelauncher open-url [URL]          Route a claude:// or codex:// link
  vibelauncher create <name> --app claude|chatgpt [--color #RRGGBB]
                      [--data <dir>] [--shortcut]
  vibelauncher delete <profile> [--delete-data]
  vibelauncher doctor [--fix] [--json] Find and fix common problems
  vibelauncher shortcuts sync          Recreate launcher shortcuts
  vibelauncher scheme status|claim|release [claude|chatgpt]
  vibelauncher log                     Show recent link-routing decisions
  vibelauncher --version

<profile> is a profile id or name. Data and settings live in the paths shown by
`vibelauncher list --json`; set VIBELAUNCHER_HOME to keep everything elsewhere.
";

/// Tiny argument reader: positional values plus `--flag` / `--key value`.
struct Args {
    pos: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: &[String], valued: &[&str]) -> Result<Self> {
        let mut pos = Vec::new();
        let mut flags = Vec::new();
        let mut it = raw.iter();
        while let Some(a) = it.next() {
            if a == "--" {
                pos.extend(it.by_ref().cloned());
                break;
            }
            if let Some(name) = a.strip_prefix("--") {
                let (k, v) = match name.split_once('=') {
                    Some((k, v)) => (k.to_string(), Some(v.to_string())),
                    None if valued.contains(&name) => (
                        name.to_string(),
                        Some(
                            it.next()
                                .cloned()
                                .ok_or_else(|| anyhow!("--{name} needs a value"))?,
                        ),
                    ),
                    None => (name.to_string(), None),
                };
                flags.push((k, v));
            } else {
                pos.push(a.clone());
            }
        }
        Ok(Self { pos, flags })
    }
    fn has(&self, k: &str) -> bool {
        self.flags.iter().any(|(f, _)| f == k)
    }
    fn val(&self, k: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(f, _)| f == k)
            .and_then(|(_, v)| v.as_deref())
    }
    fn need(&self, i: usize, what: &str) -> Result<&str> {
        self.pos
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| anyhow!("missing {what} (see `vibelauncher --help`)"))
    }
}

/// Commands the GUI binary hands over to this module instead of opening a
/// window (the AppImage has only one executable, and shortcuts run it).
pub fn is_command(first: &str) -> bool {
    const CMDS: &[&str] = &[
        "-h",
        "--help",
        "help",
        "-V",
        "--version",
        "version",
        "list",
        "ls",
        "ps",
        "launch",
        "open",
        "login",
        "quit",
        "open-url",
        "route",
        "create",
        "new",
        "delete",
        "rm",
        "doctor",
        "shortcuts",
        "scheme",
        "log",
        "internal-reassert",
    ];
    CMDS.contains(&first) || first.contains("://")
}

/// Entry point shared by `vibelauncher` and `vibelauncher-gui`.
pub fn main_with(args: Vec<String>) -> ExitCode {
    #[cfg(windows)]
    unsafe {
        // ATTACH_PARENT_PROCESS: print to the terminal we were started from.
        windows_sys::Win32::System::Console::AttachConsole(u32::MAX);
    }
    match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("vibelauncher: {e:#}");
            ExitCode::from(1)
        }
    }
}

pub fn run(raw: Vec<String>) -> Result<ExitCode> {
    let Some(cmd) = raw.first().cloned() else {
        open_gui(&[])?;
        return Ok(ExitCode::SUCCESS);
    };
    let rest = &raw[1..];
    match cmd.as_str() {
        "-h" | "--help" | "help" => print!("{HELP}"),
        "-V" | "--version" | "version" => println!("vibelauncher {}", env!("CARGO_PKG_VERSION")),
        "list" | "ls" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            list(&mut l, a.has("json"))?;
        }
        "ps" => {
            let mut l = Launcher::open()?;
            println!("{}", serde_json::to_string_pretty(&l.snapshot().apps)?);
        }
        "launch" | "open" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            l.launch(a.need(0, "profile")?, a.pos.get(1).map(String::as_str))?;
        }
        "login" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            let id = a.need(0, "profile")?;
            l.login(id)?;
            println!("Opened {id}. Sign in within 5 minutes; the sign-in link will come back to this profile.");
        }
        "quit" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            if !l.quit(a.need(0, "profile")?)? {
                println!("not running");
            }
        }
        "open-url" | "route" => return open_url(rest),
        "create" | "new" => {
            let a = Args::parse(rest, &["app", "color", "data", "icon"])?;
            let provider = a
                .val("app")
                .and_then(ProviderKind::parse)
                .ok_or_else(|| anyhow!("--app claude|chatgpt is required"))?;
            let mut l = Launcher::open()?;
            let p = l.create_profile(ProfileInput {
                name: a.need(0, "name")?.into(),
                provider,
                color: a.val("color").unwrap_or("#6E7BF2").into(),
                icon: a.val("icon").map(String::from),
                data_root: a.val("data").map(Into::into),
                env: Default::default(),
                extra_args: vec![],
                shortcut: a.has("shortcut"),
            })?;
            println!(
                "Created {} ({}). Data: {}",
                p.name,
                p.id,
                p.data_root(&l.paths).display()
            );
        }
        "delete" | "rm" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            l.delete_profile(a.need(0, "profile")?, a.has("delete-data"))?;
        }
        "doctor" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            return doctor_cmd(&mut l, a.has("fix"), a.has("json"));
        }
        "shortcuts" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            match a.pos.first().map(String::as_str) {
                Some("sync") | None => {
                    let errs = l.sync_shortcuts();
                    for e in &errs {
                        eprintln!("{e}");
                    }
                    if !errs.is_empty() {
                        return Ok(ExitCode::from(1));
                    }
                }
                Some(o) => bail!("unknown `shortcuts {o}`"),
            }
        }
        "scheme" => {
            let a = Args::parse(rest, &[])?;
            let mut l = Launcher::open()?;
            let providers: Vec<ProviderKind> = match a.pos.get(1) {
                Some(p) => vec![ProviderKind::parse(p).ok_or_else(|| anyhow!("unknown app {p}"))?],
                None => ProviderKind::ALL.to_vec(),
            };
            for p in providers {
                let inst = l.install(p);
                match a.pos.first().map(String::as_str).unwrap_or("status") {
                    "status" => println!(
                        "{}",
                        serde_json::to_string_pretty(&scheme::status(p, inst.as_ref(), &l.paths))?
                    ),
                    "claim" => scheme::claim(
                        p,
                        &inst.ok_or_else(|| anyhow!("{} not found", p.display_name()))?,
                        &l.paths,
                    )?,
                    "release" => scheme::release(p, inst.as_ref(), &l.paths)?,
                    o => bail!("unknown `scheme {o}`"),
                }
            }
        }
        "log" => {
            let l = Launcher::open()?;
            for line in router::PendingStore::new(&l.paths).recent_log(40) {
                println!("{line}");
            }
        }
        // Started in the background after a launch on macOS / Windows.
        "internal-reassert" => {
            let a = Args::parse(rest, &[])?;
            let p = ProviderKind::parse(a.need(0, "app")?).ok_or_else(|| anyhow!("unknown app"))?;
            let mut l = Launcher::open()?;
            l.reassert_after_launch(p);
        }
        other => {
            // `vibelauncher claude://…` (macOS / Windows hand us the URL alone).
            if other.contains("://") {
                return open_url(&raw);
            }
            bail!("unknown command `{other}` (see `vibelauncher --help`)");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Exit status matters here: non-zero tells the Linux link handler to fall
/// back to opening the app directly, so only fail when nothing was opened.
fn open_url(rest: &[String]) -> Result<ExitCode> {
    let a = Args::parse(rest, &["provider"])?;
    let provider = a.val("provider").and_then(ProviderKind::parse);
    let url = a.pos.first().map(String::as_str);
    let mut l = Launcher::open()?;
    let res = match l.open_url(provider, url) {
        Ok(r) => r,
        Err(e) => {
            router::PendingStore::new(&l.paths).log(&format!("error: {e:#}"));
            return Err(e);
        }
    };
    if let Decision::Ask { candidates } = &res.decision {
        let url = url.unwrap_or_default();
        if open_gui(&["--choose-route".into(), url.into()]).is_err() {
            // No GUI available: prefer a running profile, else the first.
            let snap = l.snapshot();
            let pick = candidates
                .iter()
                .find(|c| snap.is_running(c))
                .or(candidates.first())
                .cloned();
            match pick {
                Some(id) => l.deliver_choice(&id, url)?,
                None => bail!("no profile to open the link"),
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn open_gui(args: &[String]) -> Result<()> {
    let gui = vl_core::paths::gui_exe().ok_or_else(|| {
        anyhow!("the VibeLauncher window (vibelauncher-gui) isn't installed next to this program")
    })?;
    vl_core::launch::keep_std_handles_private();
    let mut c = std::process::Command::new(gui);
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            c.pre_exec(|| {
                libc_setsid();
                Ok(())
            });
        }
    }
    c.spawn()?;
    Ok(())
}

#[cfg(unix)]
fn libc_setsid() {
    extern "C" {
        fn setsid() -> i32;
    }
    unsafe {
        setsid();
    }
}

fn list(l: &mut Launcher, json: bool) -> Result<()> {
    let snap = l.snapshot();
    if json {
        let v: Vec<serde_json::Value> = l
            .config
            .profiles
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id, "name": p.name, "app": p.provider, "isolated": p.isolated,
                    "running": snap.is_running(&p.id),
                    "dataRoot": p.isolated.then(|| p.data_root(&l.paths)),
                    "lastLaunchedAt": p.last_launched_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "config": l.paths.config_file(), "profiles": v,
            }))?
        );
        return Ok(());
    }
    for provider in ProviderKind::ALL {
        let inst = l.install(provider);
        println!(
            "{} {}",
            provider.display_name(),
            match &inst {
                Some(i) => i.version.clone().unwrap_or_default(),
                None => "(not found)".into(),
            }
        );
        for p in l.config.profiles.iter().filter(|p| p.provider == provider) {
            let dot = if snap.is_running(&p.id) { "●" } else { "○" };
            let kind = if p.isolated { "" } else { "  (default)" };
            println!("  {dot} {:<20} {}{kind}", p.id, p.name);
        }
    }
    Ok(())
}

fn doctor_cmd(l: &mut Launcher, fix: bool, json: bool) -> Result<ExitCode> {
    let checks = doctor::run(l);
    if json {
        println!("{}", serde_json::to_string_pretty(&checks)?);
    } else {
        for c in &checks {
            let mark = match c.level {
                doctor::Level::Ok => "✓",
                doctor::Level::Info => "i",
                doctor::Level::Warning => "!",
                doctor::Level::Error => "✗",
            };
            println!("{mark} {}\n    {}", c.title, c.detail);
        }
    }
    let mut failed = checks.iter().any(|c| c.level == doctor::Level::Error);
    if fix {
        for c in checks.iter().filter(|c| c.level >= doctor::Level::Warning) {
            if let Some(f) = &c.fix {
                print!("Fixing: {} … ", c.title);
                match doctor::apply(l, f) {
                    Ok(()) => println!("done"),
                    Err(e) => {
                        println!("failed: {e:#}");
                        failed = true;
                    }
                }
            }
        }
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
