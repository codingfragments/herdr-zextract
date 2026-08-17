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
//! Not ported (out of scope for this phase, see PLANNING.md §11 Phase 5):
//! `ui`, `colors`, `grab`, `limits`, `types`, `actions` blocks - those
//! configure UI/action-template/scrollback-depth surfaces this port
//! hasn't built yet. Live reload is also out of scope - config is read
//! once per invocation, matching the original's snapshot-once model.

use std::collections::HashSet;

use serde::Deserialize;

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
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Built-in type tags or custom pattern names to skip entirely.
    pub disabled: HashSet<String>,
    /// Whether `secret`'s entropy-fallback pass runs, on top of the
    /// curated-format regexes (which always run regardless).
    pub secret_entropy_filter: bool,
    pub custom: Vec<CustomPattern>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            disabled: HashSet::new(),
            secret_entropy_filter: true,
            custom: Vec::new(),
        }
    }
}

impl Config {
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
            },
            Err(e) => {
                eprintln!("herdr-zextract: failed to parse {}: {e}", path.display());
                Self::default()
            }
        }
    }
}
