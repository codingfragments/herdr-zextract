//! User config, loaded once at startup from
//! `$HERDR_PLUGIN_CONFIG_DIR/config.toml`. Ports the built-in-tuning and
//! custom-pattern surface of the original plugin's `doc/config-reference.md`
//! `patterns { }` block.
//!
//! One deliberate format change: the original uses KDL, matching
//! Zellij's own config format. Herdr's own config (`config.toml`,
//! `herdr-plugin.toml`) is TOML, so this port uses TOML too for
//! consistency with the host it's actually running under.
//!
//! Also carries `[profiles.<name>]` (Phase 7): per-keybind grab scope,
//! pattern allowlist, and query pre-fill, selected at launch by
//! `ZEXTRACT_PROFILE` - see `doc/config-reference.md`.
//!
//! Not ported (out of scope for this phase, see PLANNING.md §11 Phase 5):
//! `ui`, `colors`, `limits`, `types`, `actions` blocks - those configure
//! UI/action-template surfaces this port hasn't built yet. Live reload
//! is also out of scope - config is read once per invocation, matching
//! the original's snapshot-once model.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Starter config written by [`write_default`], mirroring the
/// original plugin's `Ctrl-W` "write starter config" banner action.
/// Sourced directly from `config.example.toml` at the repo root (the
/// copy-pasteable template mentioned in the README and
/// `doc/config-reference.md`) so the two can never drift apart -
/// there is exactly one place this text lives.
pub const DEFAULT_CONFIG_TOML: &str = include_str!("../config.example.toml");

#[derive(Debug, Clone, Deserialize)]
pub struct CustomPattern {
    pub name: String,
    pub regex: String,
    #[serde(rename = "type", default = "default_custom_type")]
    pub ty: String,
    pub template: Option<String>,
}

fn default_custom_type() -> String {
    "url".to_string()
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PatternsSection {
    #[serde(default)]
    disable: Vec<String>,
    #[serde(default)]
    secret: SecretSection,
    #[serde(default)]
    custom: Vec<CustomPattern>,
}

#[derive(Debug, Clone, Deserialize)]
struct SecretSection {
    #[serde(default = "default_true")]
    entropy_filter: bool,
}

impl Default for SecretSection {
    fn default() -> Self {
        Self {
            entropy_filter: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    patterns: PatternsSection,
    #[serde(default)]
    profiles: std::collections::HashMap<String, Profile>,
}

/// Per-keybind override bundle, selected at runtime by `ZEXTRACT_PROFILE`
/// and defined by name under `[profiles.<name>]` in the user's own
/// `config.toml` — the launcher action in `herdr-plugin.toml` only ever
/// picks a profile *name*, never the grab/pattern values themselves, so
/// tuning a keybind's behavior never requires touching plugin packaging.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Profile {
    /// Grab profile name (`quick`/`deep`/`viewport`/`full`/`tab-scan`).
    /// `None` defers to this profile's built-in default, if any, else
    /// `grab::resolve`'s own fallback (`quick`).
    pub grab: Option<String>,
    /// Allowlist of type tags to extract at all, overriding `[patterns]`
    /// `disable` entirely for this invocation. `None` means no
    /// restriction (the config file's own `disable` list still applies).
    pub patterns: Option<Vec<String>>,
    /// Type tags to pre-fill the picker query with as `#tag` filters.
    pub type_filter: Option<Vec<String>>,
}

/// Built-in defaults for the four named profiles the plugin ships
/// actions for, so they work with zero user config (matching the
/// README's "works with zero config" claim) — a `[profiles.<name>]`
/// block in the user's own config.toml overrides these entirely, not
/// merges with them.
fn builtin_profile(name: &str) -> Option<Profile> {
    match name {
        "open" => Some(Profile::default()),
        "tab" => Some(Profile {
            grab: Some("tab-scan".to_string()),
            ..Profile::default()
        }),
        "url" => Some(Profile {
            patterns: Some(vec![
                "url".to_string(),
                "ipv4".to_string(),
                "ipv6".to_string(),
            ]),
            ..Profile::default()
        }),
        "url-tab" => Some(Profile {
            grab: Some("tab-scan".to_string()),
            patterns: Some(vec![
                "url".to_string(),
                "ipv4".to_string(),
                "ipv6".to_string(),
            ]),
            ..Profile::default()
        }),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Built-in type tags or custom pattern names to skip entirely.
    pub disabled: HashSet<String>,
    /// Whether `secret`'s entropy-fallback pass runs, on top of the
    /// curated-format regexes (which always run regardless).
    pub secret_entropy_filter: bool,
    pub custom: Vec<CustomPattern>,
    pub profiles: std::collections::HashMap<String, Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            disabled: HashSet::new(),
            secret_entropy_filter: true,
            custom: Vec::new(),
            profiles: std::collections::HashMap::new(),
        }
    }
}

impl Config {
    /// Resolve `name` (from `ZEXTRACT_PROFILE`) to a [`Profile`]: the
    /// user's own `[profiles.<name>]` config wins if present, else one
    /// of the four built-in named defaults, else an all-defaults
    /// profile (plain `quick` grab, no pattern restriction) — an
    /// unrecognized custom profile name degrades gracefully rather than
    /// failing the launch.
    pub fn resolve_profile(&self, name: &str) -> Profile {
        self.profiles
            .get(name)
            .cloned()
            .or_else(|| builtin_profile(name))
            .unwrap_or_default()
    }

    /// Allowlist mode, ported from the original's per-keybind
    /// `patterns` override: only the given tags (built-in or custom
    /// pattern names) run at all, overriding any `disable` list from
    /// the config file entirely - not layered on top of it.
    pub fn restrict_to(&mut self, allowed: &HashSet<String>) {
        let mut disabled: HashSet<String> = crate::matcher::TYPE_PRIORITY
            .iter()
            .map(|t| t.tag().to_string())
            .collect();
        disabled.extend(self.custom.iter().map(|cp| cp.name.clone()));
        disabled.retain(|tag| !allowed.contains(tag));
        self.disabled = disabled;
    }

    /// Load from `$HERDR_PLUGIN_CONFIG_DIR/config.toml`. Missing file,
    /// unset env var, or a parse error all fall back to
    /// `Config::default()` (all built-ins on, no custom patterns) - a
    /// broken config must never crash the plugin. Parse errors are
    /// reported on stderr so they're visible without blocking startup.
    pub fn load() -> Self {
        let Ok(dir) = std::env::var("HERDR_PLUGIN_CONFIG_DIR") else {
            return Self::default();
        };
        let path = std::path::Path::new(&dir).join("config.toml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str::<RawConfig>(&text) {
            Ok(raw) => Self {
                disabled: raw.patterns.disable.into_iter().collect(),
                secret_entropy_filter: raw.patterns.secret.entropy_filter,
                custom: raw.patterns.custom,
                profiles: raw.profiles,
            },
            Err(e) => {
                eprintln!("herdr-zextract: failed to parse {}: {e}", path.display());
                Self::default()
            }
        }
    }
}

fn config_path() -> Option<PathBuf> {
    std::env::var("HERDR_PLUGIN_CONFIG_DIR")
        .ok()
        .map(|dir| Path::new(&dir).join("config.toml"))
}

/// True only when `$HERDR_PLUGIN_CONFIG_DIR` is set but `config.toml`
/// doesn't exist there yet - the case [`write_default`] is for. A set
/// env var pointing at a config that exists but fails to parse is a
/// different situation (the user has a config, just a broken one) and
/// intentionally doesn't trigger this.
pub fn is_missing() -> bool {
    config_path().is_some_and(|p| !p.exists())
}

/// Write [`DEFAULT_CONFIG_TOML`] to `$HERDR_PLUGIN_CONFIG_DIR/config.toml`,
/// creating the directory if needed. Refuses to overwrite an existing
/// file (`create_new`) - this is only ever offered when [`is_missing`]
/// was true, but a defensive double-check costs nothing.
pub fn write_default() -> Result<PathBuf, String> {
    let path = config_path().ok_or("HERDR_PLUGIN_CONFIG_DIR is not set")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create {parent:?}: {e}"))?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, DEFAULT_CONFIG_TOML.as_bytes()))
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restrict_to_disables_everything_not_allowed() {
        let mut config = Config::default();
        let allowed: HashSet<String> = ["url".to_string(), "ipv4".to_string()].into();
        config.restrict_to(&allowed);
        assert!(!config.disabled.contains("url"));
        assert!(!config.disabled.contains("ipv4"));
        assert!(config.disabled.contains("file"));
        assert!(config.disabled.contains("secret"));
    }

    #[test]
    fn restrict_to_overrides_prior_disable_list() {
        let mut config = Config {
            disabled: ["url".to_string()].into(),
            ..Config::default()
        };
        let allowed: HashSet<String> = ["url".to_string()].into();
        config.restrict_to(&allowed);
        // "url" is now explicitly allowed, so the prior disable of it
        // must not survive - restrict_to replaces, doesn't layer.
        assert!(!config.disabled.contains("url"));
    }

    #[test]
    fn restrict_to_keeps_allowed_custom_patterns() {
        let mut config = Config {
            custom: vec![CustomPattern {
                name: "jira".to_string(),
                regex: "x".to_string(),
                ty: "url".to_string(),
                template: None,
            }],
            ..Config::default()
        };
        let allowed: HashSet<String> = ["jira".to_string()].into();
        config.restrict_to(&allowed);
        assert!(!config.disabled.contains("jira"));
        assert!(config.disabled.contains("url"));
    }

    #[test]
    fn resolve_profile_uses_builtin_defaults_with_zero_config() {
        let config = Config::default();
        assert_eq!(config.resolve_profile("open").grab, None);
        assert_eq!(
            config.resolve_profile("tab").grab,
            Some("tab-scan".to_string())
        );
        assert_eq!(
            config.resolve_profile("url").patterns,
            Some(vec![
                "url".to_string(),
                "ipv4".to_string(),
                "ipv6".to_string()
            ])
        );
        assert_eq!(
            config.resolve_profile("url-tab").grab,
            Some("tab-scan".to_string())
        );
    }

    #[test]
    fn resolve_profile_unknown_name_degrades_to_plain_default() {
        let config = Config::default();
        let profile = config.resolve_profile("custom0");
        assert_eq!(profile.grab, None);
        assert_eq!(profile.patterns, None);
        assert_eq!(profile.type_filter, None);
    }

    #[test]
    fn resolve_profile_user_config_overrides_builtin_entirely() {
        let mut config = Config::default();
        config.profiles.insert(
            "tab".to_string(),
            Profile {
                grab: Some("deep".to_string()),
                ..Profile::default()
            },
        );
        let profile = config.resolve_profile("tab");
        // The built-in "tab" default is grab="tab-scan" - a user
        // override must replace it outright, not merge with it.
        assert_eq!(profile.grab, Some("deep".to_string()));
    }

    #[test]
    fn resolve_profile_user_config_defines_custom_slot() {
        let mut config = Config::default();
        config.profiles.insert(
            "custom0".to_string(),
            Profile {
                grab: Some("deep".to_string()),
                patterns: Some(vec!["secret".to_string()]),
                type_filter: None,
            },
        );
        let profile = config.resolve_profile("custom0");
        assert_eq!(profile.grab, Some("deep".to_string()));
        assert_eq!(profile.patterns, Some(vec!["secret".to_string()]));
    }
}
