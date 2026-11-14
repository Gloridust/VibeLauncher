//! Profiles and settings, stored in `profiles.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::paths::{write_atomic, Paths};
use crate::provider::ProviderKind;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Stable id: lowercase letters, digits and dashes. Used for directory
    /// names, shortcut file names and window classes.
    pub id: String,
    pub name: String,
    pub provider: ProviderKind,
    /// `#RRGGBB`
    pub color: String,
    /// A symbol name from the UI's icon set (e.g. `briefcase`).
    #[serde(default = "default_icon")]
    pub icon: String,
    /// `false` for the built-in default profile of each app: it runs the app
    /// exactly as installed, with no extra flags or variables.
    #[serde(default = "yes")]
    pub isolated: bool,
    /// Where this profile's data lives. `None` = `<profiles_dir>/<id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub shortcut: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_launched_at: Option<DateTime<Utc>>,
}

fn default_icon() -> String {
    "sparkle".into()
}
fn yes() -> bool {
    true
}

impl Profile {
    pub fn data_root(&self, paths: &Paths) -> PathBuf {
        self.data_root
            .clone()
            .unwrap_or_else(|| paths.profiles_dir.join(&self.id))
    }

    pub fn default_for(provider: ProviderKind) -> Self {
        Self {
            id: provider.default_profile_id().into(),
            name: provider.display_name().into(),
            provider,
            color: provider.brand_color().into(),
            icon: "house".into(),
            isolated: false,
            data_root: None,
            env: BTreeMap::new(),
            extra_args: vec![],
            shortcut: false,
            created_at: Utc::now(),
            last_launched_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Route `claude://` callbacks through VibeLauncher.
    pub route_claude: bool,
    /// Route `codex://` callbacks through VibeLauncher.
    pub route_chatgpt: bool,
    /// Seconds between launches when starting several profiles at once, so
    /// their update timers don't line up.
    pub stagger_seconds: u32,
    /// "auto" | "zh" | "en"
    pub language: String,
    /// "auto" | "light" | "dark"
    pub theme: String,
    /// Manually chosen app executables, by provider id.
    pub app_overrides: BTreeMap<String, PathBuf>,
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            route_claude: true,
            route_chatgpt: true,
            stagger_seconds: 20,
            language: "auto".into(),
            theme: "auto".into(),
            app_overrides: BTreeMap::new(),
            onboarded: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub schema: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default, rename = "profile")]
    pub profiles: Vec<Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            settings: Settings::default(),
            profiles: vec![],
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        let file = paths.config_file();
        let mut cfg = match std::fs::read_to_string(&file) {
            Ok(text) => toml::from_str::<Config>(&text)
                .with_context(|| format!("{} is not valid", file.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", file.display())),
        };
        if cfg.schema > SCHEMA {
            bail!(
                "{} was written by a newer VibeLauncher (schema {}); please update",
                file.display(),
                cfg.schema
            );
        }
        cfg.schema = SCHEMA;
        cfg.ensure_defaults();
        Ok(cfg)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.ensure()?;
        let text = toml::to_string_pretty(self)?;
        let header = "# VibeLauncher profiles. Edit with the app or `vibelauncher`; hand edits are fine too.\n\n";
        write_atomic(&paths.config_file(), format!("{header}{text}").as_bytes())
    }

    /// Every provider always has its default profile, listed first.
    pub fn ensure_defaults(&mut self) {
        for p in ProviderKind::ALL {
            if !self.profiles.iter().any(|x| x.id == p.default_profile_id()) {
                self.profiles.insert(0, Profile::default_for(p));
            }
        }
        for prof in &mut self.profiles {
            if prof.id == prof.provider.default_profile_id() {
                prof.isolated = false;
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.id == id)
    }

    /// Find by id, or by display name (case-insensitive) as a convenience for
    /// the CLI.
    pub fn find(&self, key: &str) -> Option<&Profile> {
        self.get(key).or_else(|| {
            let k = key.to_lowercase();
            self.profiles.iter().find(|p| p.name.to_lowercase() == k)
        })
    }

    pub fn unique_id(&self, name: &str, provider: ProviderKind) -> String {
        let base = slugify(name);
        let base = if base.is_empty() {
            format!("{}-profile", provider.id())
        } else {
            base
        };
        let mut id = base.clone();
        let mut n = 2;
        while self.get(&id).is_some()
            || ProviderKind::ALL
                .iter()
                .any(|p| p.default_profile_id() == id)
        {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    pub fn validate_profile(
        &self,
        prof: &Profile,
        paths: &Paths,
        replacing: Option<&str>,
    ) -> Result<()> {
        if !is_valid_id(&prof.id) {
            bail!(
                "profile id {:?} may only contain lowercase letters, digits and dashes",
                prof.id
            );
        }
        if prof.name.trim().is_empty() {
            bail!("profile name is empty");
        }
        if !is_hex_color(&prof.color) {
            bail!("color {:?} is not #RRGGBB", prof.color);
        }
        for key in prof.env.keys() {
            if key.is_empty() || key.contains('=') || key.contains('\0') {
                bail!("invalid environment variable name {key:?}");
            }
        }
        let root = prof.data_root(paths);
        for other in &self.profiles {
            if Some(other.id.as_str()) == replacing {
                continue;
            }
            if other.id == prof.id {
                bail!("a profile with id {:?} already exists", prof.id);
            }
            if other.isolated && prof.isolated && same_path(&other.data_root(paths), &root) {
                bail!("profile {:?} already uses {}", other.name, root.display());
            }
        }
        Ok(())
    }
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    out.chars()
        .take(32)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

pub fn is_hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())
}

fn same_path(a: &Path, b: &Path) -> bool {
    let ca = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let cb = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Work Account"), "work-account");
        assert_eq!(slugify("  工作 2 "), "2");
        assert_eq!(slugify("工作"), "");
        assert_eq!(slugify("a--b__c"), "a-b-c");
    }

    #[test]
    fn unique_ids() {
        let mut c = Config::default();
        c.ensure_defaults();
        assert_eq!(c.unique_id("工作", ProviderKind::Claude), "claude-profile");
        let mut p = Profile::default_for(ProviderKind::Claude);
        p.id = "work".into();
        p.isolated = true;
        c.profiles.push(p);
        assert_eq!(c.unique_id("Work", ProviderKind::Claude), "work-2");
        assert_eq!(
            c.unique_id("claude default", ProviderKind::Claude),
            "claude-default-2"
        );
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let mut c = Config::load(&paths).unwrap();
        assert_eq!(c.profiles.len(), ProviderKind::ALL.len());
        let mut p = Profile::default_for(ProviderKind::Claude);
        p.id = "work".into();
        p.name = "Work".into();
        p.isolated = true;
        p.env
            .insert("HTTPS_PROXY".into(), "http://127.0.0.1:7890".into());
        c.validate_profile(&p, &paths, None).unwrap();
        c.profiles.push(p);
        c.save(&paths).unwrap();
        let back = Config::load(&paths).unwrap();
        assert_eq!(back, c);
        // Duplicate data dir is rejected.
        let mut dup = back.get("work").unwrap().clone();
        dup.id = "work-b".into();
        dup.data_root = Some(paths.profiles_dir.join("work"));
        assert!(back.validate_profile(&dup, &paths, None).is_err());
    }
}
