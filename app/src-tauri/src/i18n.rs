//! Interface text in English and Simplified Chinese.
//!
//! The tables are the ones the previous (web) window used, kept as JSON.
//! Diagnostics come from `vl-core` with a stable `code`; they are translated
//! here, falling back to the English text the check carries.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

use chrono::{DateTime, Local, Utc};
use vl_core::doctor::Check;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tables {
    en: HashMap<String, String>,
    zh: HashMap<String, String>,
    checks_en: HashMap<String, (String, String)>,
    checks_zh: HashMap<String, (String, String)>,
}

static TABLES: LazyLock<Tables> = LazyLock::new(|| serde_json::from_str(include_str!("i18n.json")).expect("i18n.json"));
static ZH: AtomicBool = AtomicBool::new(false);

/// Resolve the setting ("auto" | "en" | "zh") and use it. Returns "en" or "zh".
pub fn set_language(pref: &str) -> &'static str {
    let zh = match pref {
        "zh" => true,
        "en" => false,
        _ => sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("zh")),
    };
    ZH.store(zh, Ordering::Relaxed);
    lang()
}

pub fn lang() -> &'static str {
    if is_zh() {
        "zh"
    } else {
        "en"
    }
}

pub fn is_zh() -> bool {
    ZH.load(Ordering::Relaxed)
}

/// Text for `key` in the given language ("en" | "zh").
pub fn t_in(lang: &str, key: &str) -> String {
    let t = &*TABLES;
    let table = if lang == "zh" { &t.zh } else { &t.en };
    table.get(key).or_else(|| t.en.get(key)).cloned().unwrap_or_else(|| key.to_string())
}

pub fn t(key: &str) -> String {
    t_in(lang(), key)
}

pub fn fill(s: &str, params: &[(&str, &str)]) -> String {
    let mut out = s.to_string();
    for (k, v) in params {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

pub fn tf(key: &str, params: &[(&str, &str)]) -> String {
    fill(&t(key), params)
}

/// Title and detail of a diagnostic, in the current language.
pub fn check_text(c: &Check) -> (String, String) {
    let mut code = c.code;
    if code == "updated" && c.params.get("defaultRunning").map(String::as_str) == Some("true") {
        code = "updated-running";
    }
    let t = &*TABLES;
    let table = if is_zh() { &t.checks_zh } else { &t.checks_en };
    let Some((title, detail)) = table.get(code) else {
        return (c.title.clone(), c.detail.clone());
    };
    let params: Vec<(&str, &str)> = c.params.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut detail = fill(detail, &params);
    let zh = is_zh();
    if code == "routing-ok" {
        if let Some(h) = c.params.get("handler") {
            detail += &if zh { format!(" 当前处理程序：{h}。") } else { format!(" Current handler: {h}.") };
        }
        if c.params.get("experimental").map(String::as_str) == Some("true") {
            detail += if zh { "（在这个系统上为实验性功能。）" } else { " (Experimental on this system.)" };
        }
    }
    (fill(title, &params).trim().to_string(), detail)
}

pub fn relative_time(when: Option<DateTime<Utc>>) -> String {
    let Some(when) = when else { return t("detail.never") };
    let s = (Utc::now() - when).num_seconds();
    let n = |v: i64| v.to_string();
    if s < 60 {
        t("time.justNow")
    } else if s < 3600 {
        tf("time.min", &[("n", &n(s / 60))])
    } else if s < 86400 {
        tf("time.hour", &[("n", &n(s / 3600))])
    } else if s < 86400 * 30 {
        tf("time.day", &[("n", &n(s / 86400))])
    } else {
        let d = when.with_timezone(&Local);
        if is_zh() {
            d.format("%Y/%-m/%-d").to_string()
        } else {
            d.format("%b %-d, %Y").to_string()
        }
    }
}

/// The link log is written in English by the command, each line starting
/// with a UTC timestamp: show local time, and translate its fixed phrases.
pub fn localize_log_line(line: &str) -> String {
    let Some((stamp, rest)) = line.split_once(char::is_whitespace) else { return line.to_string() };
    let Ok(when) = DateTime::parse_from_rfc3339(stamp) else { return line.to_string() };
    let mut rest = rest.trim_start().to_string();
    if is_zh() {
        rest = translate_log(&rest);
    }
    let local = when.with_timezone(&Local);
    let time = local.format("%H:%M:%S").to_string();
    let when = if local.date_naive() == Local::now().date_naive() {
        time
    } else if is_zh() {
        format!("{} {time}", local.format("%-m月%-d日"))
    } else {
        format!("{} {time}", local.format("%b %-d"))
    };
    format!("{when}  {rest}")
}

fn translate_log(s: &str) -> String {
    if let Some(name) = s.strip_prefix("login started for ") {
        return format!("{name} 开始登录");
    }
    let mut s = s.to_string();
    if let Some(rest) = s.strip_prefix("error: ") {
        s = format!("出错：{rest}");
    }
    const ENDINGS: [(&str, &str); 7] = [
        ("→ asking the user", "→ 询问用户"),
        ("(a login was started from this profile)", "（登录是从这个配置发起的）"),
        ("(it is the only one running)", "（只有它在运行）"),
        ("(it was used most recently)", "（最近用过它）"),
        ("(no other profile applies)", "（没有别的配置适用）"),
        ("(chosen by you)", "（由你选择）"),
        ("(pasted)", "（手动粘贴）"),
    ];
    for (en, zh) in ENDINGS {
        if let Some(head) = s.strip_suffix(en) {
            return format!("{head}{zh}");
        }
    }
    s
}

pub fn format_bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if v >= 100.0 || i == 0 {
        format!("{} {}", v.round(), U[i])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_have_every_key() {
        let t = &*TABLES;
        for k in t.en.keys() {
            assert!(t.zh.contains_key(k), "zh is missing {k}");
        }
        for k in t.checks_en.keys() {
            assert!(t.checks_zh.contains_key(k), "zh checks are missing {k}");
        }
    }

    #[test]
    fn fills_parameters() {
        assert_eq!(fill("{a} and {b}", &[("a", "x"), ("b", "y")]), "x and y");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
    }

    #[test]
    fn translates_log_lines() {
        set_language("zh");
        assert_eq!(translate_log("claude://x → work (pasted)"), "claude://x → work （手动粘贴）");
        assert_eq!(translate_log("login started for Work"), "Work 开始登录");
        set_language("en");
    }
}
