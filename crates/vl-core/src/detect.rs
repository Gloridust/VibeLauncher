//! Whether an app the user picked can run separate profiles.
//!
//! VibeLauncher keeps profiles apart by giving each its own
//! `--user-data-dir`. Electron and Chromium apps honour that flag (Claude
//! and ChatGPT profiles work the same way); other apps have no such switch,
//! so only those two kinds are accepted. They are recognised by files every
//! build ships next to its program. That can't prove an app doesn't choose
//! its own folder in code, so `doctor` also checks, after a profile's first
//! launch, that its folder was actually used.

use std::path::{Path, PathBuf};

use crate::install::which;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Electron,
    Chromium,
}

impl Engine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Electron => "electron",
            Self::Chromium => "chromium",
        }
    }
}

/// An app that can run separate profiles.
#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    pub name: String,
    /// The executable, the AppImage, or on macOS the `.app`.
    pub program: PathBuf,
    pub base_args: Vec<String>,
    pub engine: Engine,
    pub icon: Option<PathBuf>,
    /// An icon read from inside an AppImage, to be saved by the caller (the
    /// file itself is only there while the image is mounted).
    pub icon_png: Option<Vec<u8>>,
    pub desktop_file: Option<PathBuf>,
}

/// Why a picked file can't be used. [`Refusal::code`] is what the window
/// translates; the English text is for the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Missing,
    Sandboxed,
    Script,
    AppImageUnreadable,
    NotChromium,
    VibeLauncher,
    /// Claude or ChatGPT: already in the list.
    BuiltIn,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Sandboxed => "sandboxed",
            Self::Script => "script",
            Self::AppImageUnreadable => "appimage",
            Self::NotChromium => "not-chromium",
            Self::VibeLauncher => "vibelauncher",
            Self::BuiltIn => "built-in",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Missing => "That file doesn't exist (any more).",
            Self::Sandboxed => {
                "Flatpak and Snap apps run in a sandbox that keeps their data where the sandbox decides, so VibeLauncher can't give them a folder per profile."
            }
            Self::Script => {
                "This is a launcher script, and nothing next to it shows what it starts. Choose the program it runs, or the app's menu entry."
            }
            Self::AppImageUnreadable => {
                "VibeLauncher couldn't look inside this AppImage (it needs FUSE to mount it). Extract it, or install the app another way, and choose that."
            }
            Self::NotChromium => {
                "Only Electron and Chromium apps can run separate profiles: they keep everything in one folder that VibeLauncher can choose. This app isn't one of them."
            }
            Self::VibeLauncher => "That's VibeLauncher itself.",
            Self::BuiltIn => "VibeLauncher already knows this app: choose it from the list instead.",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Refusal {}

/// Look at what the user picked: an executable, an AppImage, a `.desktop`
/// entry, or a macOS `.app`.
pub fn detect(path: &Path) -> Result<Detected, Refusal> {
    if !path.exists() {
        return Err(Refusal::Missing);
    }
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("desktop") => from_desktop(path),
        Some("app") if path.is_dir() => from_bundle(path),
        _ => from_program(path, None),
    }
}

fn from_desktop(file: &Path) -> Result<Detected, Refusal> {
    use crate::desktop_entry as de;
    let entry = de::read(file).ok_or(Refusal::Missing)?;
    if entry.main("X-VibeLauncher").is_some() {
        return Err(Refusal::VibeLauncher);
    }
    let mut args: Vec<String> =
        entry.exec_args("Desktop Entry").ok_or(Refusal::Missing)?.into_iter().filter(|a| !de::is_field_code(a)).collect();
    // `env A=b program …`: what matters is the program.
    if args.first().is_some_and(|a| Path::new(a).file_name().is_some_and(|n| n == "env")) {
        args.remove(0);
        while args.first().is_some_and(|a| a.contains('=') && !a.starts_with('-')) {
            args.remove(0);
        }
    }
    let (first, rest) = args.split_first().ok_or(Refusal::Missing)?;
    if matches!(Path::new(first).file_name().and_then(|n| n.to_str()), Some("flatpak" | "snap")) {
        return Err(Refusal::Sandboxed);
    }
    let program = which(first).ok_or(Refusal::Missing)?;
    let mut d = from_program(&program, entry.main("Name"))?;
    d.base_args = rest.to_vec();
    d.desktop_file = Some(file.to_path_buf());
    if let Some(icon) = entry.main("Icon").and_then(|n| crate::install::linux_icon(&n)) {
        d.icon = Some(icon);
    }
    Ok(d)
}

fn from_program(program: &Path, name: Option<String>) -> Result<Detected, Refusal> {
    let real = program.canonicalize().unwrap_or_else(|_| program.to_path_buf());
    if real == crate::paths::self_exe() || real.file_stem().is_some_and(|s| s.to_string_lossy().starts_with("vibelauncher")) {
        return Err(Refusal::VibeLauncher);
    }
    if is_sandboxed(&real) {
        return Err(Refusal::Sandboxed);
    }
    let name = name.unwrap_or_else(|| title_case(&program.file_stem().unwrap_or_default().to_string_lossy()));
    let mut d = Detected {
        name,
        program: program.to_path_buf(),
        base_args: vec![],
        engine: Engine::Electron,
        icon: None,
        icon_png: None,
        desktop_file: None,
    };
    if is_appimage(&real) {
        let (engine, name, icon) = appimage::inspect(&real).ok_or(Refusal::AppImageUnreadable)?;
        d.engine = engine.ok_or(Refusal::NotChromium)?;
        if let Some(n) = name {
            d.name = n;
        }
        d.icon_png = icon;
        return Ok(d);
    }
    let dir = real.parent().ok_or(Refusal::Missing)?;
    // A wrapper script next to the real binary (Chrome's `google-chrome`)
    // passes its arguments on; the folder tells what it runs.
    match engine_in_dir(dir).or_else(|| versioned_dirs(dir).iter().find_map(|d| engine_in_dir(d))) {
        Some(e) => {
            d.engine = e;
            d.icon = ["resources/icon.png", "product_logo_256.png", "product_logo_128.png"]
                .iter()
                .map(|n| dir.join(n))
                .find(|p| p.is_file())
                .or_else(|| crate::install::linux_icon(&program.file_stem().unwrap_or_default().to_string_lossy()));
            Ok(d)
        }
        None if is_script(&real) => Err(Refusal::Script),
        None => Err(Refusal::NotChromium),
    }
}

fn from_bundle(bundle: &Path) -> Result<Detected, Refusal> {
    if bundle.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("VibeLauncher")) {
        return Err(Refusal::VibeLauncher);
    }
    let engine = bundle_engine(bundle).ok_or(Refusal::NotChromium)?;
    #[cfg(target_os = "macos")]
    let (name, icon) = {
        use crate::install::macos::plist_value;
        let name = plist_value(bundle, "CFBundleDisplayName").or_else(|| plist_value(bundle, "CFBundleName"));
        let icon = plist_value(bundle, "CFBundleIconFile").map(|i| {
            let mut p = bundle.join("Contents/Resources").join(i);
            if p.extension().is_none() {
                p.set_extension("icns");
            }
            p
        });
        (name, icon.filter(|p| p.is_file()))
    };
    #[cfg(not(target_os = "macos"))]
    let (name, icon): (Option<String>, Option<PathBuf>) = (None, None);
    Ok(Detected {
        name: name.unwrap_or_else(|| bundle.file_stem().unwrap_or_default().to_string_lossy().into_owned()),
        program: bundle.to_path_buf(),
        base_args: vec![],
        engine,
        icon,
        icon_png: None,
        desktop_file: None,
    })
}

/// What a folder holding an app's program shows it to be.
pub fn engine_in_dir(dir: &Path) -> Option<Engine> {
    let res = dir.join("resources");
    if res.join("app.asar").is_file() || res.join("app").join("package.json").is_file() {
        return Some(Engine::Electron);
    }
    let chromium = ["v8_context_snapshot.bin", "snapshot_blob.bin", "chrome_100_percent.pak", "resources.pak"];
    chromium.iter().any(|f| dir.join(f).is_file()).then_some(Engine::Chromium)
}

/// A macOS bundle's engine, from its frameworks.
fn bundle_engine(bundle: &Path) -> Option<Engine> {
    let fw = bundle.join("Contents/Frameworks");
    if fw.join("Electron Framework.framework").is_dir() {
        return Some(Engine::Electron);
    }
    let rd = std::fs::read_dir(&fw).ok()?;
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "framework"))
        .any(|f| has_file(&f, 5, &|n| n.starts_with("v8_context_snapshot") || n == "chrome_100_percent.pak"))
        .then_some(Engine::Chromium)
}

fn has_file(dir: &Path, depth: u32, want: &dyn Fn(&str) -> bool) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.flatten().any(|e| {
        let p = e.path();
        // Frameworks link `Current` to a version folder; don't walk it twice.
        let link = e.file_type().is_ok_and(|t| t.is_symlink());
        if p.is_file() {
            want(&e.file_name().to_string_lossy())
        } else {
            !link && depth > 0 && p.is_dir() && has_file(&p, depth - 1, want)
        }
    })
}

/// Windows (Squirrel) installs keep the app in `app-<version>` folders next
/// to a small stub that starts the newest; newest first.
pub fn versioned_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<(Option<semver::Version>, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let ver = n.strip_prefix("app-")?;
            Some((semver::Version::parse(ver).ok(), e.path()))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// The programs a stub starts (`app-*/<same name>`), so a running copy is
/// recognised as this app.
pub fn versioned_exes(program: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (program.parent(), program.file_name()) else { return vec![] };
    versioned_dirs(dir).into_iter().map(|d| d.join(name)).filter(|p| p.is_file()).collect()
}

fn is_sandboxed(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.starts_with("/snap/") || s.contains("/flatpak/") || s.starts_with("/var/lib/snapd/")
}

fn is_script(p: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 2];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == b"#!"
}

fn is_appimage(p: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 11];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head[..4] == b"\x7fELF" && &head[8..11] == b"AI\x02"
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Mount an AppImage (it does so itself with `--appimage-mount`, while it
/// keeps running) to see what's inside.
mod appimage {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    /// What's inside: the engine, the app's name and its icon (PNG bytes).
    type Found = (Option<Engine>, Option<String>, Option<Vec<u8>>);

    pub fn inspect(image: &Path) -> Option<Found> {
        let mut child = Command::new(image)
            .arg("--appimage-mount")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(out).read_line(&mut line);
            let _ = tx.send(line);
        });
        let found = rx.recv_timeout(Duration::from_secs(15)).ok().and_then(|line| {
            let root = PathBuf::from(line.trim());
            root.is_dir().then(|| look(&root))
        });
        let _ = child.kill();
        let _ = child.wait();
        found
    }

    fn look(root: &Path) -> Found {
        // The program sits at the top or in a usr/lib|bin|share folder.
        let mut dirs = vec![root.to_path_buf()];
        for sub in ["usr/bin", "usr/lib", "usr/share", "opt"] {
            let d = root.join(sub);
            dirs.push(d.clone());
            if let Ok(rd) = std::fs::read_dir(&d) {
                dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
        }
        let engine = dirs.iter().find_map(|d| engine_in_dir(d));
        let entry = std::fs::read_dir(root)
            .ok()
            .and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "desktop")))
            .and_then(|f| crate::desktop_entry::read(&f));
        let name = entry.as_ref().and_then(|e| e.main("Name"));
        let icon = std::fs::read(root.join(".DirIcon")).ok().filter(|b| b.starts_with(b"\x89PNG"));
        (engine, name, icon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn recognises_electron_and_chromium_folders() {
        let t = tempfile::tempdir().unwrap();
        let electron = t.path().join("obsidian");
        touch(&electron.join("obsidian"));
        touch(&electron.join("resources/app.asar"));
        let d = detect(&electron.join("obsidian")).unwrap();
        assert_eq!((d.engine, d.name.as_str()), (Engine::Electron, "Obsidian"));

        let chrome = t.path().join("chrome");
        touch(&chrome.join("chrome"));
        touch(&chrome.join("v8_context_snapshot.bin"));
        assert_eq!(detect(&chrome.join("chrome")).unwrap().engine, Engine::Chromium);

        let native = t.path().join("gimp");
        touch(&native.join("gimp"));
        assert_eq!(detect(&native.join("gimp")), Err(Refusal::NotChromium));
        assert_eq!(detect(&t.path().join("nope")), Err(Refusal::Missing));
    }

    #[test]
    fn a_script_needs_something_to_go_by() {
        let t = tempfile::tempdir().unwrap();
        let script = t.path().join("bin/start");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "#!/bin/sh\nexec /somewhere/app \"$@\"\n").unwrap();
        assert_eq!(detect(&script), Err(Refusal::Script));
        // A wrapper sitting next to Chromium's files is fine.
        touch(&t.path().join("bin/resources.pak"));
        assert_eq!(detect(&script).unwrap().engine, Engine::Chromium);
    }

    #[test]
    fn squirrel_stub_finds_its_versions() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Discord");
        touch(&root.join("Discord.exe"));
        touch(&root.join("app-1.0.9/Discord.exe"));
        touch(&root.join("app-1.0.10/Discord.exe"));
        touch(&root.join("app-1.0.10/resources/app.asar"));
        let d = detect(&root.join("Discord.exe")).unwrap();
        assert_eq!(d.engine, Engine::Electron);
        assert_eq!(versioned_exes(&root.join("Discord.exe"))[0], root.join("app-1.0.10/Discord.exe"));
    }

    #[test]
    fn bundles_by_framework() {
        let t = tempfile::tempdir().unwrap();
        let e = t.path().join("Slack.app");
        std::fs::create_dir_all(e.join("Contents/Frameworks/Electron Framework.framework")).unwrap();
        assert_eq!(detect(&e).unwrap().engine, Engine::Electron);
        let c = t.path().join("Brave Browser.app");
        touch(&c.join("Contents/Frameworks/Brave Browser Framework.framework/Versions/1.2/Resources/v8_context_snapshot.arm64.bin"));
        let d = detect(&c).unwrap();
        assert_eq!((d.engine, d.name.as_str()), (Engine::Chromium, "Brave Browser"));
        let n = t.path().join("Notes.app");
        std::fs::create_dir_all(n.join("Contents/MacOS")).unwrap();
        assert_eq!(detect(&n), Err(Refusal::NotChromium));
    }

    #[cfg(unix)]
    #[test]
    fn desktop_entries_and_sandboxes() {
        let t = tempfile::tempdir().unwrap();
        let app = t.path().join("opt/Obsidian");
        touch(&app.join("obsidian"));
        touch(&app.join("resources/app.asar"));
        let entry = t.path().join("obsidian.desktop");
        std::fs::write(
            &entry,
            format!("[Desktop Entry]\nType=Application\nName=Obsidian Notes\nExec=env A=1 {} --no-sandbox %U\n", app.join("obsidian").display()),
        )
        .unwrap();
        let d = detect(&entry).unwrap();
        assert_eq!(d.name, "Obsidian Notes");
        assert_eq!(d.base_args, ["--no-sandbox"]);
        assert_eq!(d.desktop_file.as_deref(), Some(entry.as_path()));
        let flat = t.path().join("flat.desktop");
        std::fs::write(&flat, "[Desktop Entry]\nType=Application\nName=X\nExec=/usr/bin/flatpak run md.obsidian.Obsidian\n").unwrap();
        assert_eq!(detect(&flat), Err(Refusal::Sandboxed));
    }
}
