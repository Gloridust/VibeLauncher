//! End-to-end test of the real `vibelauncher` command on the current OS,
//! against a stand-in app (`vl-fake-app`) that records how it was started.
//!
//! It touches per-user system state (link handlers, shortcuts), so it only
//! runs when asked:
//!
//! - `VL_SMOKE=sandbox` keeps VibeLauncher's files in a temporary folder (and,
//!   on Linux, the XDG folders too). Safe on a developer machine.
//! - `VL_SMOKE=ci` uses the real per-user locations and also checks that the
//!   OS itself delivers a `claude://` link through VibeLauncher. For CI runners.
//!
//! cargo test -p vibelauncher --features smoke-test --test smoke -- --nocapture

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const CMD: &str = env!("CARGO_BIN_EXE_vibelauncher");
const FAKE: &str = env!("CARGO_BIN_EXE_vl-fake-app");
#[cfg(target_os = "macos")]
const LSREGISTER: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

struct Smoke {
    _tmp: tempfile::TempDir,
    vars: Vec<(String, String)>,
    /// Folder holding the stand-in executable and its log.
    app_dir: PathBuf,
    /// What Settings → "Choose…" would store for Claude.
    app_path: PathBuf,
    /// Whether to test that the OS routes a link through VibeLauncher.
    os_dispatch: bool,
}

impl Drop for Smoke {
    /// Stop any stand-in still running, also when the test failed.
    fn drop(&mut self) {
        let _ = std::fs::write(self.app_dir.join("fake-app-stop"), "");
        std::thread::sleep(Duration::from_millis(500));
        #[cfg(target_os = "macos")]
        if let Some(parent) = self.app_path.parent().filter(|p| p.to_string_lossy().contains("/Applications/VLSmoke-")) {
            let _ = Command::new(LSREGISTER).arg("-u").arg(&self.app_path).status();
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

fn canonical(p: &Path) -> PathBuf {
    let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    // Strip Windows' \\?\ prefix so the path looks like the ones users pick.
    let s = c.to_string_lossy().into_owned();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
}

impl Smoke {
    fn setup(mode: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = canonical(tmp.path());
        let mut vars = Vec::new();
        if mode == "sandbox" {
            vars.push(("VIBELAUNCHER_HOME".into(), root.join("home").display().to_string()));
        }
        let (app_dir, app_path) = make_fake_app(&root, &mut vars, mode);
        let os_dispatch = mode == "ci" || cfg!(target_os = "linux");
        Self { _tmp: tmp, vars, app_dir, app_path, os_dispatch }
    }

    fn cmd(&self, args: &[&str]) -> Output {
        let mut c = Command::new(CMD);
        c.args(args);
        for (k, v) in &self.vars {
            c.env(k, v);
        }
        let out = c.output().expect("run vibelauncher");
        eprintln!(
            "$ vibelauncher {}\n{}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.cmd(args);
        assert!(out.status.success(), "`vibelauncher {}` failed", args.join(" "));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.ok(args)).expect("valid JSON")
    }

    fn profile(&self, name: &str) -> Value {
        self.json(&["list", "--json"])["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("no profile {name}"))
            .clone()
    }

    fn launches(&self) -> Vec<Value> {
        std::fs::read_to_string(self.app_dir.join("fake-app-log.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// Wait for the stand-in to be started in a way `pred` accepts.
    fn wait_launch(&self, what: &str, since: usize, pred: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            if let Some(v) = self.launches().into_iter().skip(since).find(|v| pred(v)) {
                return v;
            }
            assert!(Instant::now() < deadline, "timed out waiting for: {what}\nlaunches: {:#?}", self.launches());
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    fn wait_running(&self, name: &str, want: bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.profile(name)["running"] != want {
            if Instant::now() >= deadline {
                self.dump_processes();
                panic!("{name} running should become {want}");
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// What VibeLauncher and the OS each see, for a failed wait.
    fn dump_processes(&self) {
        let mut ps = Command::new(CMD);
        ps.arg("ps").env("VIBELAUNCHER_DEBUG_PROCS", "1");
        for (k, v) in &self.vars {
            ps.env(k, v);
        }
        if let Ok(o) = ps.output() {
            eprintln!("$ vibelauncher ps\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        }
        eprintln!("stand-in launches: {:#?}", self.launches());
        if let Ok(rd) = std::fs::read_dir(&self.app_dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("fake-app-alive-") || n.starts_with("fake-app-panic-") {
                    eprintln!("{n}: {}", std::fs::read_to_string(e.path()).unwrap_or_default());
                }
            }
        }
        // Control: a copy started straight from the test, then scanned again.
        #[cfg(target_os = "windows")]
        if let Some(udd) = self.launches().first().and_then(udd_of) {
            if let Ok(mut c) = Command::new(self.app_dir.join(exe_name())).arg(format!("--user-data-dir={udd}")).spawn() {
                std::thread::sleep(Duration::from_secs(3));
                let mut ps = Command::new(CMD);
                ps.arg("ps").env("VIBELAUNCHER_DEBUG_PROCS", "1");
                for (k, v) in &self.vars {
                    ps.env(k, v);
                }
                if let Ok(o) = ps.output() {
                    eprintln!(
                        "control copy pid {} alive={:?}\n$ vibelauncher ps\n{}{}",
                        c.id(),
                        c.try_wait().map(|s| s.is_none()),
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                }
                let _ = c.kill();
            }
        }
        let os = if cfg!(target_os = "windows") {
            Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'Claude*' } | Format-List ProcessId,ParentProcessId,ExecutablePath,CommandLine",
                ])
                .output()
        } else {
            Command::new("ps").args(["-eo", "pid,ppid,args"]).output()
        };
        if let Ok(o) = os {
            let text = String::from_utf8_lossy(&o.stdout);
            let app = self.app_dir.display().to_string();
            let lines: Vec<&str> = if cfg!(target_os = "windows") {
                text.lines().collect()
            } else {
                text.lines().filter(|l| l.contains(&app)).collect()
            };
            eprintln!("OS process list:\n{}", lines.join("\n"));
        }
    }

    fn checks(&self) -> Vec<Value> {
        let v = self.json(&["doctor", "--json"]);
        v.as_array().cloned().or_else(|| v["checks"].as_array().cloned()).expect("doctor --json gives checks")
    }
}

fn args_of(v: &Value) -> Vec<String> {
    v["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect()
}

fn udd_of(v: &Value) -> Option<String> {
    let a = args_of(v);
    a.iter()
        .find_map(|x| x.strip_prefix("--user-data-dir=").map(str::to_string))
        .or_else(|| a.iter().position(|x| x == "--user-data-dir").and_then(|i| a.get(i + 1).cloned()))
}

fn same_dir(a: &str, b: &Path) -> bool {
    canonical(Path::new(a)) == canonical(b)
}

#[cfg(target_os = "linux")]
fn make_fake_app(root: &Path, vars: &mut Vec<(String, String)>, mode: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("claude-desktop");
    std::fs::copy(FAKE, &exe).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The app's "system" desktop entry, in a data dir of its own so the
    // per-user shadow entry VibeLauncher writes is a separate file.
    let sys = root.join("sysdata");
    std::fs::create_dir_all(sys.join("applications")).unwrap();
    let entry = sys.join("applications/vl-smoke-claude.desktop");
    std::fs::write(
        &entry,
        format!(
            "[Desktop Entry]\nType=Application\nName=Claude\nExec={} %u\nMimeType=x-scheme-handler/claude;\n",
            exe.display()
        ),
    )
    .unwrap();
    let base = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    vars.push(("XDG_DATA_DIRS".into(), format!("{}:{base}", sys.display())));
    if mode == "sandbox" {
        for (k, d) in [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"), ("XDG_STATE_HOME", "state"), ("XDG_CACHE_HOME", "cache")] {
            let p = root.join("xdg").join(d);
            std::fs::create_dir_all(&p).unwrap();
            vars.push((k.into(), p.display().to_string()));
        }
    }
    (dir, entry)
}

#[cfg(target_os = "macos")]
fn make_fake_app(root: &Path, _vars: &mut Vec<(String, String)>, mode: &str) -> (PathBuf, PathBuf) {
    // In CI, where a real install would be: Launch Services may not name an
    // app inside a temporary folder as a scheme's default.
    let parent = if mode == "ci" {
        PathBuf::from(std::env::var("HOME").unwrap()).join("Applications").join(format!("VLSmoke-{}", std::process::id()))
    } else {
        root.to_path_buf()
    };
    let bundle = parent.join("VLSmokeClaude.app");
    let dir = bundle.join("Contents/MacOS");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(FAKE, dir.join("VLSmokeClaude")).unwrap();
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>VLSmokeClaude</string>
<key>CFBundleIdentifier</key><string>io.github.gloridust.vlsmoke.claude</string>
<key>CFBundleName</key><string>VLSmokeClaude</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>9.9.9</string>
<key>LSUIElement</key><true/>
<key>CFBundleURLTypes</key><array><dict>
<key>CFBundleURLName</key><string>VLSmokeClaude</string>
<key>CFBundleURLSchemes</key><array><string>claude</string></array>
</dict></array>
</dict></plist>
"#,
    )
    .unwrap();
    let _ = Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&bundle).status();
    // Like the real app, it declares claude:// and Launch Services knows it.
    let _ = Command::new(LSREGISTER).arg("-f").arg(&bundle).status();
    (dir, bundle)
}

#[cfg(target_os = "windows")]
fn make_fake_app(root: &Path, _vars: &mut Vec<(String, String)>, _mode: &str) -> (PathBuf, PathBuf) {
    let dir = root.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("Claude.exe");
    std::fs::copy(FAKE, &exe).unwrap();
    (dir, exe)
}

/// Ask the OS to open a link, the way a browser does after sign-in.
fn os_open(s: &Smoke, url: &str) -> bool {
    let mut c = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/open");
        c.arg(url);
        c
    } else {
        if Command::new("gio").arg("--version").output().is_err() {
            eprintln!("(gio not installed; skipping the system link test)");
            return false;
        }
        let mut c = Command::new("gio");
        c.args(["open", url]);
        c
    };
    for (k, v) in &s.vars {
        c.env(k, v);
    }
    let st = c.status().expect("open link");
    assert!(st.success(), "the OS refused to open {url}");
    true
}

#[test]
fn smoke() {
    let Ok(mode) = std::env::var("VL_SMOKE") else {
        eprintln!("skipped: set VL_SMOKE=sandbox (or ci) to run");
        return;
    };
    let s = Smoke::setup(&mode);

    // Point VibeLauncher at the stand-in, as Settings → Choose… does.
    let cfg_file = PathBuf::from(s.json(&["list", "--json"])["config"].as_str().unwrap());
    std::fs::create_dir_all(cfg_file.parent().unwrap()).unwrap();
    let path_lit = s.app_path.display().to_string().replace('\'', "");
    std::fs::write(&cfg_file, format!("schema = 1\n\n[settings]\nonboarded = true\nstaggerSeconds = 0\n\n[settings.appOverrides]\nclaude = '{path_lit}'\n"))
        .unwrap();
    let list = s.ok(&["list"]);
    assert!(list.lines().next().unwrap().starts_with("Claude") && !list.contains("Claude (not found)"), "Claude not found:\n{list}");
    // Before anything is claimed: who the system sends claude:// to (logged only).
    let _ = s.cmd(&["scheme", "status", "claude"]);

    // ── create + launch: own data folder and CLAUDE_CONFIG_DIR ──
    s.ok(&["create", "Smoke Work", "--app", "claude"]);
    s.ok(&["create", "Smoke Personal", "--app", "claude", "--shortcut"]);
    let work = s.profile("Smoke Work");
    let work_id = work["id"].as_str().unwrap().to_string();
    let work_root = PathBuf::from(work["dataRoot"].as_str().unwrap());
    let personal_id = s.profile("Smoke Personal")["id"].as_str().unwrap().to_string();

    let n = s.launches().len();
    let t = Instant::now();
    s.ok(&["launch", &work_id]);
    // The app must not hold on to our output (on Windows it inherits every
    // inheritable handle), or whoever reads it waits until the app quits.
    assert!(t.elapsed() < Duration::from_secs(30), "`launch` waited for the app ({:?})", t.elapsed());
    let l = s.wait_launch("Smoke Work to start", n, |v| udd_of(v).is_some());
    assert!(same_dir(&udd_of(&l).unwrap(), &work_root.join("electron")), "wrong data folder: {l}");
    let ccd = l["env"]["CLAUDE_CONFIG_DIR"].as_str().expect("CLAUDE_CONFIG_DIR set");
    assert!(same_dir(ccd, &work_root.join("claude-config")), "wrong CLAUDE_CONFIG_DIR: {ccd}");

    // ── the process is recognised as that profile ──
    s.wait_running("Smoke Work", true);
    assert_eq!(s.profile("Smoke Personal")["running"], false);
    assert!(
        !s.checks().iter().any(|c| c["code"] == "lost-env"),
        "a correctly started profile was reported as lost"
    );

    // ── quit stops every copy of the profile, not just the first ──
    let mut second = Command::new(s.app_dir.join(exe_name()))
        .arg(format!("--user-data-dir={}", work_root.join("electron").display()))
        .env("CLAUDE_CONFIG_DIR", work_root.join("claude-config"))
        .spawn()
        .expect("start a second copy");
    s.ok(&["quit", &work_id]);
    s.wait_running("Smoke Work", false);
    let _ = second.wait();

    // ── an update relaunch that loses CLAUDE_CONFIG_DIR is detected and fixed ──
    let udd = format!("--user-data-dir={}", work_root.join("electron").display());
    let mut stray = Command::new(s.app_dir.join(exe_name()));
    stray.arg(&udd).env_remove("CLAUDE_CONFIG_DIR");
    let mut stray = stray.spawn().expect("start stray instance");
    s.wait_running("Smoke Work", true);
    let lost = s.checks().into_iter().find(|c| c["code"] == "lost-env");
    assert!(lost.is_some(), "a profile running without CLAUDE_CONFIG_DIR wasn't detected");
    let n = s.launches().len();
    s.ok(&["doctor", "--fix"]);
    let _ = stray.wait();
    let l = s.wait_launch("Smoke Work to be restarted with its settings", n, |v| {
        udd_of(v).is_some() && v["env"]["CLAUDE_CONFIG_DIR"].is_string()
    });
    assert!(same_dir(&udd_of(&l).unwrap(), &work_root.join("electron")));
    s.wait_running("Smoke Work", true);
    assert!(!s.checks().iter().any(|c| c["code"] == "lost-env"), "still lost after the fix");

    // ── sign-in routing: the link goes to the profile signing in ──
    s.ok(&["login", &personal_id]);
    s.wait_running("Smoke Personal", true);
    let n = s.launches().len();
    let url = "claude://login/callback?code=SMOKESECRET1";
    s.ok(&["open-url", url]);
    let l = s.wait_launch("the sign-in link to reach Smoke Personal", n, |v| args_of(v).iter().any(|a| a == url));
    let personal_root = PathBuf::from(s.profile("Smoke Personal")["dataRoot"].as_str().unwrap());
    assert!(same_dir(&udd_of(&l).expect("link went to an isolated profile"), &personal_root.join("electron")));
    assert!(!s.ok(&["log"]).contains("SMOKESECRET1"), "the sign-in code was written to the log");

    // ── the system hands links to VibeLauncher ──
    s.ok(&["scheme", "claim", "claude"]);
    let status = s.json(&["scheme", "status", "claude"]);
    assert_eq!(status["claimed"], true, "not claimed: {status}");
    if s.os_dispatch {
        s.ok(&["login", &work_id]);
        let n = s.launches().len();
        let url = "claude://login/callback?code=SMOKESECRET2";
        if os_open(&s, url) {
            let l = s.wait_launch("the OS to deliver the link to Smoke Work", n, |v| args_of(v).iter().any(|a| a == url));
            assert!(same_dir(&udd_of(&l).expect("isolated"), &work_root.join("electron")));
            eprintln!("✓ the system delivered {url} to Smoke Work through VibeLauncher");
        }
    }
    s.ok(&["scheme", "release", "claude"]);
    let status = s.json(&["scheme", "status", "claude"]);
    assert_eq!(status["claimed"], false, "still claimed: {status}");
    // The stand-in declares claude:// like the real app, so it gets it back.
    #[cfg(target_os = "macos")]
    if status["handler"] != "io.github.gloridust.vlsmoke.claude" {
        let dump = Command::new(LSREGISTER)
            .arg("-dump")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        // Whole records (separated by dashed lines) that mention the stand-in.
        let mut known = Vec::new();
        let mut block = Vec::new();
        for l in dump.lines().chain(std::iter::once("----")) {
            if l.starts_with("----") {
                if block.iter().any(|b: &&str| b.contains("vlsmoke") || b.contains("vibelauncher.router")) {
                    known.push(block.iter().take(60).copied().collect::<Vec<_>>().join("\n"));
                }
                block.clear();
            } else {
                block.push(l);
            }
        }
        panic!("the app didn't get its links back: {status}\nLaunch Services knows:\n{}", known.join("\n----\n"));
    }

    // ── shortcuts ──
    let shortcut = shortcut_for(&s, "Smoke Personal", &personal_id);
    assert!(shortcut.exists(), "no shortcut at {}", shortcut.display());

    // ── clean up ──
    std::fs::write(s.app_dir.join("fake-app-stop"), "").unwrap();
    s.wait_running("Smoke Work", false);
    s.wait_running("Smoke Personal", false);
    s.ok(&["delete", &work_id, "--delete-data"]);
    s.ok(&["delete", &personal_id, "--delete-data"]);
    assert!(!shortcut.exists(), "shortcut left behind at {}", shortcut.display());
    assert!(!work_root.exists(), "data left behind at {}", work_root.display());
}

fn exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Claude.exe"
    } else if cfg!(target_os = "macos") {
        "VLSmokeClaude"
    } else {
        "claude-desktop"
    }
}

/// The profile's shortcut: an app-menu entry, an app in ~/Applications, or a
/// Start-menu link. Named after the profile, so look for it by name.
fn shortcut_for(s: &Smoke, name: &str, id: &str) -> PathBuf {
    let var = |k: &str| s.vars.iter().find(|(n, _)| n == k).map(|(_, v)| PathBuf::from(v));
    let home = || PathBuf::from(std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap());
    if cfg!(target_os = "linux") {
        return var("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local/share")).join(format!("applications/vibelauncher-{id}.desktop"));
    }
    let dir = if cfg!(target_os = "macos") {
        home().join("Applications/VibeLauncher")
    } else {
        PathBuf::from(std::env::var("APPDATA").unwrap()).join("Microsoft\\Windows\\Start Menu\\Programs\\VibeLauncher")
    };
    std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains(name)))
        .unwrap_or_else(|| dir.join(format!("(no shortcut for {name})")))
}
