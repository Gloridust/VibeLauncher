//! Deciding which profile gets an incoming `claude://` / `codex://` URL.
//!
//! The decision is a pure function ([`decide`]) so every rule is unit-tested;
//! [`PendingStore`] keeps the "I'm about to log in to profile X" markers.

use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::paths::{write_atomic, Paths};
use crate::provider::ProviderKind;

/// How long a pending-login marker stays valid.
pub const PENDING_TTL_SECS: i64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub provider: ProviderKind,
    pub profile: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Decision {
    /// Hand the URL to this profile.
    To { profile: String, reason: Reason },
    /// Ambiguous login callback: let the user choose.
    Ask { candidates: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    PendingLogin,
    OnlyRunning,
    MostRecent,
    Default,
}

impl Reason {
    pub fn describe(self) -> &'static str {
        match self {
            Reason::PendingLogin => "a login was started from this profile",
            Reason::OnlyRunning => "it is the only one running",
            Reason::MostRecent => "it was used most recently",
            Reason::Default => "no other profile applies",
        }
    }
}

/// What the router knows about one profile of the URL's provider.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub running: bool,
    pub last_launched_at: Option<DateTime<Utc>>,
}

pub fn decide(
    provider: ProviderKind,
    is_login: bool,
    pending: &[Pending],
    candidates: &[Candidate],
    now: DateTime<Utc>,
) -> Decision {
    let exists = |id: &str| candidates.iter().any(|c| c.id == id);
    // 1. A fresh pending-login marker wins.
    if let Some(p) = pending
        .iter()
        .filter(|p| p.provider == provider && now - p.created_at <= Duration::seconds(PENDING_TTL_SECS) && p.created_at <= now + Duration::seconds(5))
        .max_by_key(|p| p.created_at)
    {
        if exists(&p.profile) {
            return Decision::To { profile: p.profile.clone(), reason: Reason::PendingLogin };
        }
    }
    let running: Vec<&Candidate> = candidates.iter().filter(|c| c.running).collect();
    // 2. Exactly one instance running: it must be the one.
    if running.len() == 1 {
        return Decision::To { profile: running[0].id.clone(), reason: Reason::OnlyRunning };
    }
    let most_recent = |set: &[&Candidate]| -> Option<String> {
        set.iter()
            .max_by_key(|c| c.last_launched_at)
            .filter(|c| c.last_launched_at.is_some())
            .map(|c| c.id.clone())
    };
    // 3. Several running.
    if running.len() > 1 {
        if is_login {
            return Decision::Ask { candidates: running.iter().map(|c| c.id.clone()).collect() };
        }
        let id = most_recent(&running).unwrap_or_else(|| running[0].id.clone());
        return Decision::To { profile: id, reason: Reason::MostRecent };
    }
    // 4. Nothing running.
    let all: Vec<&Candidate> = candidates.iter().collect();
    if is_login && candidates.len() > 1 {
        return Decision::Ask { candidates: candidates.iter().map(|c| c.id.clone()).collect() };
    }
    if let Some(id) = most_recent(&all) {
        return Decision::To { profile: id, reason: Reason::MostRecent };
    }
    Decision::To { profile: provider.default_profile_id().into(), reason: Reason::Default }
}

pub struct PendingStore {
    file: PathBuf,
    log: PathBuf,
}

impl PendingStore {
    pub fn new(paths: &Paths) -> Self {
        Self { file: paths.state_dir.join("pending-login.json"), log: paths.state_dir.join("router.log") }
    }

    pub fn load(&self) -> Vec<Pending> {
        std::fs::read(&self.file)
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<Pending>>(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, v: &[Pending]) -> Result<()> {
        write_atomic(&self.file, &serde_json::to_vec_pretty(v)?)
    }

    pub fn mark(&self, provider: ProviderKind, profile: &str) -> Result<()> {
        let now = Utc::now();
        let mut v: Vec<Pending> = self
            .load()
            .into_iter()
            .filter(|p| p.provider != provider && now - p.created_at <= Duration::seconds(PENDING_TTL_SECS))
            .collect();
        v.push(Pending { provider, profile: profile.into(), created_at: now });
        self.save(&v)
    }

    /// Remove the marker for `provider` (it is single-use).
    pub fn consume(&self, provider: ProviderKind) -> Result<()> {
        let v: Vec<Pending> = self.load().into_iter().filter(|p| p.provider != provider).collect();
        self.save(&v)
    }

    pub fn active(&self, provider: ProviderKind) -> Option<Pending> {
        let now = Utc::now();
        self.load()
            .into_iter()
            .filter(|p| p.provider == provider && now - p.created_at <= Duration::seconds(PENDING_TTL_SECS))
            .max_by_key(|p| p.created_at)
    }

    /// Append one line to the router log. The query string is dropped: login
    /// callbacks carry one-time codes there.
    pub fn log(&self, line: &str) {
        use std::io::Write;
        if let Ok(meta) = std::fs::metadata(&self.log) {
            if meta.len() > 256 * 1024 {
                let _ = std::fs::rename(&self.log, self.log.with_extension("log.1"));
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.log) {
            let _ = writeln!(f, "{} {}", Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), line);
        }
    }

    pub fn recent_log(&self, max: usize) -> Vec<String> {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(String::from).collect();
        lines[lines.len().saturating_sub(max)..].to_vec()
    }
}

/// `scheme://host/path` without query or fragment, safe to log.
pub fn redact(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut u) => {
            let had_query = u.query().is_some();
            u.set_query(None);
            u.set_fragment(None);
            let mut s = u.to_string();
            if had_query {
                s.push_str("?…");
            }
            s
        }
        Err(_) => "<not a URL>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, running: bool, mins_ago: Option<i64>) -> Candidate {
        Candidate { id: id.into(), running, last_launched_at: mins_ago.map(|m| Utc::now() - Duration::minutes(m)) }
    }
    const P: ProviderKind = ProviderKind::Claude;

    #[test]
    fn pending_wins() {
        let now = Utc::now();
        let pend = vec![Pending { provider: P, profile: "work".into(), created_at: now - Duration::seconds(30) }];
        let cands = [c("claude-default", true, Some(1)), c("work", true, Some(50))];
        assert_eq!(decide(P, true, &pend, &cands, now), Decision::To { profile: "work".into(), reason: Reason::PendingLogin });
    }

    #[test]
    fn expired_pending_ignored() {
        let now = Utc::now();
        let pend = vec![Pending { provider: P, profile: "work".into(), created_at: now - Duration::seconds(400) }];
        let cands = [c("claude-default", true, Some(1)), c("work", false, None)];
        assert_eq!(decide(P, true, &pend, &cands, now), Decision::To { profile: "claude-default".into(), reason: Reason::OnlyRunning });
    }

    #[test]
    fn other_provider_pending_ignored() {
        let now = Utc::now();
        let pend = vec![Pending { provider: ProviderKind::ChatGpt, profile: "gpt".into(), created_at: now }];
        let cands = [c("claude-default", false, None), c("work", false, Some(3))];
        assert_eq!(decide(P, false, &pend, &cands, now), Decision::To { profile: "work".into(), reason: Reason::MostRecent });
    }

    #[test]
    fn ambiguous_login_asks() {
        let now = Utc::now();
        let cands = [c("claude-default", true, Some(1)), c("work", true, Some(2))];
        assert_eq!(decide(P, true, &[], &cands, now), Decision::Ask { candidates: vec!["claude-default".into(), "work".into()] });
        // Non-login links go to the most recent.
        assert_eq!(decide(P, false, &[], &cands, now), Decision::To { profile: "claude-default".into(), reason: Reason::MostRecent });
    }

    #[test]
    fn nothing_known_goes_default() {
        let now = Utc::now();
        let cands = [c("claude-default", false, None)];
        assert_eq!(decide(P, false, &[], &cands, now), Decision::To { profile: "claude-default".into(), reason: Reason::Default });
    }

    #[test]
    fn redacts_query() {
        assert_eq!(redact("claude://claude.ai/login/x?code=SECRET"), "claude://claude.ai/login/x?…");
    }

    #[test]
    fn marker_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        paths.ensure().unwrap();
        let s = PendingStore::new(&paths);
        s.mark(P, "work").unwrap();
        assert_eq!(s.active(P).unwrap().profile, "work");
        s.consume(P).unwrap();
        assert!(s.active(P).is_none());
    }
}
