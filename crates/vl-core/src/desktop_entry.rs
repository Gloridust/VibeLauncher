//! Minimal reader/writer for freedesktop `.desktop` files.
//!
//! Kept platform-independent so it is unit-tested everywhere; only the Linux
//! platform layer uses it at runtime.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct DesktopEntry {
    /// Lines in original order, so rewriting keeps comments and unknown keys.
    pub lines: Vec<String>,
}

impl DesktopEntry {
    pub fn parse(text: &str) -> Self {
        Self { lines: text.lines().map(str::to_string).collect() }
    }

    pub fn to_text(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }

    /// Raw (still escaped) value of `key` in `group`.
    pub fn get_raw(&self, group: &str, key: &str) -> Option<&str> {
        let mut cur = "";
        for l in &self.lines {
            let t = l.trim();
            if t.starts_with('[') && t.ends_with(']') {
                cur = &t[1..t.len() - 1];
                continue;
            }
            if cur != group {
                continue;
            }
            if let Some((k, v)) = t.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim_start());
                }
            }
        }
        None
    }

    pub fn get(&self, group: &str, key: &str) -> Option<String> {
        self.get_raw(group, key).map(unescape_string)
    }

    pub fn main(&self, key: &str) -> Option<String> {
        self.get("Desktop Entry", key)
    }

    pub fn groups(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|l| {
                let t = l.trim();
                (t.starts_with('[') && t.ends_with(']')).then(|| t[1..t.len() - 1].to_string())
            })
            .collect()
    }

    /// Set (or add) `key` in `group`. `value` is the raw, already escaped value.
    pub fn set_raw(&mut self, group: &str, key: &str, value: &str) {
        let mut cur = String::new();
        let mut group_end = None;
        for i in 0..self.lines.len() {
            let t = self.lines[i].trim().to_string();
            if t.starts_with('[') && t.ends_with(']') {
                if cur == group {
                    break;
                }
                cur = t[1..t.len() - 1].to_string();
                continue;
            }
            if cur != group {
                continue;
            }
            if !t.is_empty() {
                group_end = Some(i);
            }
            if let Some((k, _)) = t.split_once('=') {
                if k.trim() == key {
                    self.lines[i] = format!("{key}={value}");
                    return;
                }
            }
        }
        if cur == group || self.groups().iter().any(|g| g == group) {
            let at = group_end.map(|i| i + 1).unwrap_or_else(|| {
                self.lines.iter().position(|l| l.trim() == format!("[{group}]")).map(|i| i + 1).unwrap_or(self.lines.len())
            });
            self.lines.insert(at, format!("{key}={value}"));
        } else {
            if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                self.lines.push(String::new());
            }
            self.lines.push(format!("[{group}]"));
            self.lines.push(format!("{key}={value}"));
        }
    }

    pub fn remove(&mut self, group: &str, key: &str) {
        let mut cur = String::new();
        self.lines.retain(|l| {
            let t = l.trim();
            if t.starts_with('[') && t.ends_with(']') {
                cur = t[1..t.len() - 1].to_string();
                return true;
            }
            !(cur == group && t.split_once('=').is_some_and(|(k, _)| k.trim() == key))
        });
    }

    /// `Exec` of the main group split into arguments.
    pub fn exec_args(&self, group: &str) -> Option<Vec<String>> {
        self.get(group, "Exec").map(|v| split_exec(&v))
    }

    /// Whether the main group lists `mime` in `MimeType`.
    pub fn handles_mime(&self, mime: &str) -> bool {
        self.main("MimeType")
            .map(|m| m.split(';').any(|x| x.trim().eq_ignore_ascii_case(mime)))
            .unwrap_or(false)
    }
}

/// Undo string-level escapes (`\s`, `\n`, `\t`, `\r`, `\\`).
pub fn unescape_string(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn escape_string(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r")
}

/// Split an (already string-unescaped) Exec value into arguments.
pub fn split_exec(v: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_arg = false;
    let mut quoted = false;
    let mut it = v.chars().peekable();
    while let Some(c) = it.next() {
        if quoted {
            match c {
                '"' => quoted = false,
                '\\' => {
                    if let Some(&n) = it.peek() {
                        if matches!(n, '"' | '`' | '$' | '\\') {
                            cur.push(n);
                            it.next();
                            continue;
                        }
                    }
                    cur.push('\\');
                }
                _ => cur.push(c),
            }
        } else if c == '"' {
            quoted = true;
            in_arg = true;
        } else if c.is_whitespace() {
            if in_arg {
                args.push(std::mem::take(&mut cur));
                in_arg = false;
            }
        } else {
            cur.push(c);
            in_arg = true;
        }
    }
    if in_arg {
        args.push(cur);
    }
    args
}

/// Quote one argument for an Exec line (before string-level escaping).
pub fn quote_exec_arg(a: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')', '`',
    ];
    if !a.is_empty() && !a.contains(RESERVED) {
        return a.to_string();
    }
    let mut s = String::from('"');
    for c in a.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

/// Build a raw (escaped) Exec value from arguments. Field codes such as `%U`
/// must be passed as their own argument and are kept as-is.
pub fn build_exec(args: &[String]) -> String {
    let joined = args
        .iter()
        .map(|a| if is_field_code(a) { a.clone() } else { quote_exec_arg(a) })
        .collect::<Vec<_>>()
        .join(" ");
    escape_string(&joined)
}

pub fn is_field_code(a: &str) -> bool {
    a.len() == 2 && a.starts_with('%')
}

/// `$XDG_DATA_HOME` then each of `$XDG_DATA_DIRS`.
pub fn xdg_data_dirs() -> Vec<PathBuf> {
    let home = crate::paths::home_dir();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let mut v = vec![data_home];
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        let p = PathBuf::from(d);
        if !v.contains(&p) {
            v.push(p);
        }
    }
    // Flatpak and Snap exports are usually already in XDG_DATA_DIRS, but not
    // when we are started from a minimal environment.
    for extra in [
        home.join(".local/share/flatpak/exports/share"),
        PathBuf::from("/var/lib/flatpak/exports/share"),
        PathBuf::from("/var/lib/snapd/desktop"),
    ] {
        if !v.contains(&extra) {
            v.push(extra);
        }
    }
    v
}

pub fn user_applications_dir() -> PathBuf {
    xdg_data_dirs().remove(0).join("applications")
}

/// All `.desktop` files with this id, highest precedence first.
pub fn find_by_id(id: &str) -> Vec<PathBuf> {
    xdg_data_dirs()
        .into_iter()
        .map(|d| d.join("applications").join(id))
        .filter(|p| p.is_file())
        .collect()
}

/// Every desktop file in the system data dirs (not the user dir) declaring `mime`.
pub fn system_handlers_of(mime: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for d in xdg_data_dirs().into_iter().skip(1) {
        let Ok(rd) = std::fs::read_dir(d.join("applications")) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let de = DesktopEntry::parse(&text);
            if de.handles_mime(mime) && de.main("X-VibeLauncher").is_none() {
                let id = p.file_name().unwrap().to_string_lossy().into_owned();
                if !out.iter().any(|(i, _)| *i == id) {
                    out.push((id, p));
                }
            }
        }
    }
    out
}

pub fn read(path: &Path) -> Option<DesktopEntry> {
    std::fs::read_to_string(path).ok().map(|t| DesktopEntry::parse(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = "[Desktop Entry]\nName=Claude\nExec=claude-desktop %U\nIcon=claude-desktop\nMimeType=x-scheme-handler/claude;application/vnd.anthropic.mcpb;\nActions=NewChat;\n\n[Desktop Action NewChat]\nName=New Chat\nExec=claude-desktop \"claude://claude.ai/new?surface=chat&source=desktop_action\"\n";

    #[test]
    fn parse_and_split() {
        let d = DesktopEntry::parse(CLAUDE);
        assert_eq!(d.main("Name").as_deref(), Some("Claude"));
        assert!(d.handles_mime("x-scheme-handler/claude"));
        assert_eq!(d.exec_args("Desktop Entry").unwrap(), vec!["claude-desktop", "%U"]);
        assert_eq!(
            d.exec_args("Desktop Action NewChat").unwrap(),
            vec!["claude-desktop", "claude://claude.ai/new?surface=chat&source=desktop_action"]
        );
    }

    #[test]
    fn exec_roundtrip() {
        let args = vec![
            "/home/a b/vibe launcher".to_string(),
            "open-url".into(),
            "$HOME \"x\" \\y".into(),
            "%U".into(),
        ];
        let raw = build_exec(&args);
        assert_eq!(split_exec(&unescape_string(&raw)), args);
    }

    #[test]
    fn set_and_remove() {
        let mut d = DesktopEntry::parse(CLAUDE);
        d.set_raw("Desktop Entry", "Exec", "x %U");
        d.set_raw("Desktop Entry", "X-VibeLauncher", "true");
        d.remove("Desktop Entry", "Icon");
        assert_eq!(d.main("Exec").as_deref(), Some("x %U"));
        assert_eq!(d.main("X-VibeLauncher").as_deref(), Some("true"));
        assert!(d.main("Icon").is_none());
        // Action group untouched and still after the main group.
        assert!(d.get("Desktop Action NewChat", "Exec").unwrap().contains("claude://"));
        let t = d.to_text();
        assert!(t.find("X-VibeLauncher").unwrap() < t.find("[Desktop Action NewChat]").unwrap());
    }
}
