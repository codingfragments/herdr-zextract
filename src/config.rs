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
//! Phase 8 closes out 100% config parity with the original: `log_level`,
//! `[grab_profiles.<name>]`, `patterns.command.flag_anchored`, `[ui]` +
//! `[profiles.<name>].preview`, `[colors]`, and `[types]`/`[actions]`/
//! `[limits]` (per-type verb overrides, action command templates,
//! per-verb dispatch caps - consumed by `actions.rs`). See
//! `doc/config-reference.md` for the full surface.
//!
//! Live reload is out of scope - config is read once per invocation,
//! matching the original's snapshot-once model.

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
    command: CommandSection,
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

/// `[patterns.command]` - tuning for the `cmd` type's detection.
#[derive(Debug, Clone, Deserialize, Default)]
struct CommandSection {
    /// The original's third command-detection strategy (walk back from
    /// a standalone `-x`/`--long-flag` token to the nearest boundary
    /// character to find the command word). Off by default - can
    /// produce false positives on prose containing flag-looking tokens.
    #[serde(default)]
    flag_anchored: bool,
}

/// Verbosity of `herdr-zextract`'s stderr diagnostics, ported from the
/// original's top-level `log_level` scalar. Ordered so `configured >=
/// level` is "should this message show" - `Off` sorts lowest, so
/// nothing is `>=` it except itself, matching "off means silence".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    log_level: LogLevel,
    #[serde(default)]
    patterns: PatternsSection,
    #[serde(default)]
    profiles: std::collections::HashMap<String, Profile>,
    #[serde(default)]
    grab_profiles: std::collections::HashMap<String, GrabProfileOverride>,
    #[serde(default)]
    ui: UiConfig,
    #[serde(default)]
    colors: ColorsConfig,
    #[serde(default)]
    types: std::collections::HashMap<String, TypeOverride>,
    #[serde(default)]
    actions: std::collections::HashMap<String, ActionTemplate>,
    #[serde(default)]
    limits: LimitsConfig,
}

/// `[types.<tag>]` - per-type verb allow-list/default override.
/// Keys are built-in type tags or custom pattern names.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TypeOverride {
    /// Verbs available for this type, replacing the built-in list
    /// entirely. Unrecognized verb names are silently dropped.
    pub actions: Option<Vec<String>>,
    /// Verb fired by `Enter`. Falls back to the built-in default if
    /// not present in the (possibly overridden) allow-list.
    pub default: Option<String>,
}

/// `[actions.<tag>]` - command templates for `open`/`edit`/`reveal`.
/// Keys are type tags or `"default"` (fallback for any type not
/// explicitly listed).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActionTemplate {
    pub open: Option<String>,
    pub edit: Option<String>,
    pub reveal: Option<String>,
}

/// `[limits]` - per-verb caps on multi-target dispatch. `0` disables a
/// verb entirely. `None` (key omitted) keeps that verb's built-in cap.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LimitsConfig {
    /// Caps `copy-raw`/`copy-display` batches together.
    pub copy: Option<u32>,
    /// Caps `insert`/`insert-display` batches together.
    pub insert: Option<u32>,
    pub open: Option<u32>,
    pub edit: Option<u32>,
    pub reveal: Option<u32>,
    pub json: Option<u32>,
}

/// `[colors]` block - full UI palette override, ported from the
/// original's `ColorsConfig`. Every key is optional; omitting a key
/// (or the whole block) keeps the built-in ANSI-palette default for
/// that slot. Values accept an ANSI name (`"dark_gray"`), `#rrggbb`
/// hex, or `rgb(r,g,b)` - parsing/defaulting happens in `picker::` at
/// render time, so this struct only carries raw strings.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ColorsConfig {
    pub muted: Option<String>,
    pub accent: Option<String>,
    pub cursor_bg: Option<String>,
    pub cursor_fg: Option<String>,
    pub highlight: Option<String>,
    pub error: Option<String>,
    pub fallback_type: Option<String>,
    pub type_url: Option<String>,
    pub type_file: Option<String>,
    pub type_diag: Option<String>,
    pub type_git: Option<String>,
    pub type_sha: Option<String>,
    pub type_ipv4: Option<String>,
    pub type_ipv6: Option<String>,
    pub type_uuid: Option<String>,
    pub type_quoted: Option<String>,
    pub type_command: Option<String>,
    pub type_secret: Option<String>,
}

/// Preview pane launch-state default, ported from the original's
/// `ui.preview`. A `[profiles.<name>].preview` override takes
/// precedence when set - see [`PreviewOverride`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PreviewState {
    #[default]
    Off,
    /// Closed by default. The original remembers the last session's
    /// state across launches; this port has no such persistence (each
    /// invocation is a fresh process), so `"auto"` behaves the same as
    /// `"off"` here - documented as a deliberate simplification.
    Auto,
    Always,
}

/// `[ui]` block - preview pane sizing/default state. Parsed here;
/// consumed by Phase 9's rendering.
#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub preview: PreviewState,
    #[serde(default = "default_preview_open_width")]
    pub preview_open_width: String,
    #[serde(default = "default_preview_closed_width")]
    pub preview_closed_width: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            preview: PreviewState::default(),
            preview_open_width: default_preview_open_width(),
            preview_closed_width: default_preview_closed_width(),
        }
    }
}

fn default_preview_open_width() -> String {
    "40%".to_string()
}

fn default_preview_closed_width() -> String {
    "70%".to_string()
}

/// User override for a named grab profile, ported from the original's
/// `grab { profiles { <name> { ... } } }`. Unlike the original (which
/// replaces *all* built-in profiles the instant this block is present
/// at all), an entry here overrides/adds by name only - built-ins for
/// names left untouched survive, consistent with `[profiles.<name>]`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GrabProfileOverride {
    /// `"scrollback"`, `"viewport"`, or `"tab"`. Unrecognized/absent
    /// falls back to `"scrollback"`, matching the original's default.
    pub source: Option<String>,
    /// Max lines to scan. `0` or absent means unbounded.
    pub lines: Option<u32>,
    /// Pattern type tags or custom pattern names to skip only when
    /// this profile is active - merged into the global disable list,
    /// not a replacement for it.
    #[serde(default)]
    pub disable: Vec<String>,
}

/// Per-keybind override bundle, selected at runtime by `ZEXTRACT_PROFILE`
/// and defined by name under `[profiles.<name>]` in the user's own
/// `config.toml` — the launcher action in `herdr-plugin.toml` only ever
/// picks a profile *name*, never the grab/pattern values themselves, so
/// tuning a keybind's behavior never requires touching plugin packaging.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Profile {
    /// Grab profile name (`quick`/`deep`/`viewport`/`full`/`tab-scan`,
    /// or any name defined under `[grab_profiles.<name>]`). `None`
    /// defaults to `quick` at the call site.
    pub grab: Option<String>,
    /// Allowlist of type tags to extract at all, overriding `[patterns]`
    /// `disable` entirely for this invocation. `None` means no
    /// restriction (the config file's own `disable` list still applies).
    pub patterns: Option<Vec<String>>,
    /// Type tags to pre-fill the picker query with as `#tag` filters.
    pub type_filter: Option<Vec<String>>,
    /// Force the preview pane open/closed for this keybind specifically,
    /// overriding `[ui].preview`'s launch-state default. `None` means
    /// "use the `[ui]` default" - this is the original's per-keybind
    /// `configuration.preview` override (`on`/`off`/`always`/`never`).
    pub preview: Option<PreviewOverride>,
}

/// `[profiles.<name>].preview` — per-keybind override of `[ui].preview`.
/// Consumed by Phase 9's rendering; this phase only adds the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewOverride {
    On,
    Off,
    Always,
    Never,
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
    pub log_level: LogLevel,
    /// Built-in type tags or custom pattern names to skip entirely.
    pub disabled: HashSet<String>,
    /// Whether `secret`'s entropy-fallback pass runs, on top of the
    /// curated-format regexes (which always run regardless).
    pub secret_entropy_filter: bool,
    /// Whether `cmd`'s flag-anchored (opt-in) detection strategy runs.
    pub command_flag_anchored: bool,
    pub custom: Vec<CustomPattern>,
    pub profiles: std::collections::HashMap<String, Profile>,
    pub grab_profiles: std::collections::HashMap<String, GrabProfileOverride>,
    pub ui: UiConfig,
    pub colors: ColorsConfig,
    pub types: std::collections::HashMap<String, TypeOverride>,
    pub actions: std::collections::HashMap<String, ActionTemplate>,
    pub limits: LimitsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            log_level: LogLevel::default(),
            disabled: HashSet::new(),
            secret_entropy_filter: true,
            command_flag_anchored: false,
            custom: Vec::new(),
            profiles: std::collections::HashMap::new(),
            grab_profiles: std::collections::HashMap::new(),
            ui: UiConfig::default(),
            colors: ColorsConfig::default(),
            types: std::collections::HashMap::new(),
            actions: std::collections::HashMap::new(),
            limits: LimitsConfig::default(),
        }
    }
}

impl Config {
    /// Print `msg` to stderr if `level` clears this config's
    /// `log_level` threshold - e.g. `log_level = "debug"` shows
    /// everything, `"off"` shows nothing, the default `"info"` shows
    /// info/warn/error but not debug traces.
    pub fn log(&self, level: LogLevel, msg: &str) {
        if self.log_level >= level {
            eprintln!("herdr-zextract: {msg}");
        }
    }

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

    /// Resolve `name` (from a profile's `grab` field) to a
    /// [`crate::grab::ResolvedGrabProfile`]: the user's own
    /// `[grab_profiles.<name>]` override wins if present (falling back
    /// to the built-in of the same name for any field left unset), else
    /// the built-in profile of that name, else `quick` - matching the
    /// original's "typos fall back to the first defined profile".
    pub fn resolve_grab_profile(&self, name: &str) -> crate::grab::ResolvedGrabProfile {
        let builtin = crate::grab::builtin_grab_profile(name);
        match self.grab_profiles.get(name) {
            Some(over) => crate::grab::ResolvedGrabProfile {
                source: over
                    .source
                    .as_deref()
                    .map(crate::grab::GrabSource::parse)
                    .unwrap_or_else(|| {
                        builtin
                            .as_ref()
                            .map(|b| b.source)
                            .unwrap_or(crate::grab::GrabSource::Scrollback)
                    }),
                lines: over
                    .lines
                    .or_else(|| builtin.as_ref().and_then(|b| b.lines)),
                disable: over.disable.clone(),
            },
            None => builtin.unwrap_or_else(|| {
                crate::grab::builtin_grab_profile("quick").expect("quick is always defined")
            }),
        }
    }

    /// Whether the preview pane should start open for this launch,
    /// combining `[ui].preview`'s default with `profile.preview`'s
    /// override (if set) - the override always wins when present.
    pub fn resolve_preview_open(&self, profile: &Profile) -> bool {
        match profile.preview {
            Some(PreviewOverride::On) | Some(PreviewOverride::Always) => true,
            Some(PreviewOverride::Off) | Some(PreviewOverride::Never) => false,
            None => matches!(self.ui.preview, PreviewState::Always),
        }
    }

    /// Every built-in type tag or custom pattern name *not* in
    /// `allowed` - allowlist mode, ported from the original's
    /// per-keybind `patterns` override: only the given tags run at all,
    /// overriding any `disable` list entirely rather than layering on
    /// top of it. Pure (returns instead of mutating `self.disabled`) so
    /// [`Self::disabled_for`] can precompute one disabled-set per
    /// grabber up front, in [`crate::grab::GrabCycler::new`], without
    /// needing a `Config` clone per grabber just to call this.
    fn complement_of(&self, allowed: &HashSet<String>) -> HashSet<String> {
        let mut disabled: HashSet<String> = crate::matcher::TYPE_PRIORITY
            .iter()
            .map(|t| t.tag().to_string())
            .collect();
        disabled.extend(self.custom.iter().map(|cp| cp.name.clone()));
        disabled.retain(|tag| !allowed.contains(tag));
        disabled
    }

    /// The `disabled` set in force for one grab profile: `allowed` (the
    /// launching profile's `patterns` allowlist) wins outright when
    /// present, ignoring `raw_disabled`/`grab_disable` entirely -
    /// "allowlist overrides every disable source, global and
    /// per-profile alike". With no allowlist, `raw_disabled`
    /// (`[patterns].disable`) merges with `grab_disable` (that
    /// profile's own `[grab_profiles.<name>].disable`).
    pub fn disabled_for(
        &self,
        allowed: Option<&HashSet<String>>,
        raw_disabled: &HashSet<String>,
        grab_disable: &[String],
    ) -> HashSet<String> {
        match allowed {
            Some(allowed) => self.complement_of(allowed),
            None => {
                let mut disabled = raw_disabled.clone();
                disabled.extend(grab_disable.iter().cloned());
                disabled
            }
        }
    }

    /// Every grab profile name selectable by `Ctrl-G` cycling, in a
    /// stable order: the five built-ins
    /// ([`crate::grab::BUILTIN_PROFILE_NAMES`]) first, then any wholly
    /// custom names defined under `[grab_profiles.<name>]` (not
    /// overriding a built-in), sorted alphabetically for determinism -
    /// `HashMap` iteration order isn't stable across runs otherwise.
    pub fn cycle_grab_profile_names(&self) -> Vec<String> {
        let mut names: Vec<String> = crate::grab::BUILTIN_PROFILE_NAMES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut custom: Vec<String> = self
            .grab_profiles
            .keys()
            .filter(|k| !crate::grab::BUILTIN_PROFILE_NAMES.contains(&k.as_str()))
            .cloned()
            .collect();
        custom.sort();
        names.extend(custom);
        names
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
                log_level: raw.log_level,
                disabled: raw.patterns.disable.into_iter().collect(),
                secret_entropy_filter: raw.patterns.secret.entropy_filter,
                command_flag_anchored: raw.patterns.command.flag_anchored,
                custom: raw.patterns.custom,
                profiles: raw.profiles,
                grab_profiles: raw.grab_profiles,
                ui: raw.ui,
                colors: raw.colors,
                types: raw.types,
                actions: raw.actions,
                limits: raw.limits,
            },
            Err(e) => {
                // log_level can't be consulted here - it lives in the
                // very file that just failed to parse - so this always
                // shows, matching the default level's threshold.
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
    fn shipped_example_config_parses() {
        // config.example.toml is the Ctrl-W starter template AND the
        // README's copy-pasteable reference - a syntax mistake in it
        // (bad indentation, wrong table header, etc.) should fail CI,
        // not surface as a live "failed to parse config.toml" report.
        toml::from_str::<RawConfig>(DEFAULT_CONFIG_TOML).expect("template must parse as TOML");
    }

    #[test]
    fn complement_of_disables_everything_not_allowed() {
        let config = Config::default();
        let allowed: HashSet<String> = ["url".to_string(), "ipv4".to_string()].into();
        let disabled = config.complement_of(&allowed);
        assert!(!disabled.contains("url"));
        assert!(!disabled.contains("ipv4"));
        assert!(disabled.contains("file"));
        assert!(disabled.contains("secret"));
    }

    #[test]
    fn disabled_for_allowlist_ignores_raw_and_grab_disable() {
        let config = Config::default();
        let allowed: HashSet<String> = ["url".to_string()].into();
        let raw_disabled: HashSet<String> = ["url".to_string()].into();
        let grab_disable = vec!["ipv4".to_string()];
        let disabled = config.disabled_for(Some(&allowed), &raw_disabled, &grab_disable);
        // "url" is explicitly allowed, so neither raw_disabled nor
        // grab_disable naming it (or anything else) survives - an
        // allowlist replaces every disable source outright.
        assert!(!disabled.contains("url"));
        assert!(disabled.contains("ipv4")); // not from grab_disable - it's just not in `allowed`
        assert!(disabled.contains("file"));
    }

    #[test]
    fn complement_of_keeps_allowed_custom_patterns() {
        let config = Config {
            custom: vec![CustomPattern {
                name: "jira".to_string(),
                regex: "x".to_string(),
                ty: "url".to_string(),
                template: None,
            }],
            ..Config::default()
        };
        let allowed: HashSet<String> = ["jira".to_string()].into();
        let disabled = config.complement_of(&allowed);
        assert!(!disabled.contains("jira"));
        assert!(disabled.contains("url"));
    }

    #[test]
    fn disabled_for_no_allowlist_merges_raw_and_grab_disable() {
        let config = Config::default();
        let raw_disabled: HashSet<String> = ["secret".to_string()].into();
        let grab_disable = vec!["ipv6".to_string()];
        let disabled = config.disabled_for(None, &raw_disabled, &grab_disable);
        assert!(disabled.contains("secret"));
        assert!(disabled.contains("ipv6"));
        assert!(!disabled.contains("url"));
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
                preview: None,
            },
        );
        let profile = config.resolve_profile("custom0");
        assert_eq!(profile.grab, Some("deep".to_string()));
        assert_eq!(profile.patterns, Some(vec!["secret".to_string()]));
    }

    #[test]
    fn resolve_grab_profile_builtin_with_zero_config() {
        let config = Config::default();
        let p = config.resolve_grab_profile("deep");
        assert_eq!(p.lines, Some(1500));
        assert!(p.disable.is_empty());
    }

    #[test]
    fn resolve_grab_profile_unknown_name_falls_back_to_quick() {
        let config = Config::default();
        let p = config.resolve_grab_profile("nonexistent");
        assert_eq!(p.lines, Some(150));
        assert_eq!(p.source, crate::grab::GrabSource::Scrollback);
    }

    #[test]
    fn resolve_grab_profile_override_fills_unset_fields_from_builtin() {
        let mut config = Config::default();
        config.grab_profiles.insert(
            "quick".to_string(),
            GrabProfileOverride {
                source: None,
                lines: Some(300),
                disable: vec!["secret".to_string()],
            },
        );
        let p = config.resolve_grab_profile("quick");
        // lines overridden, source falls back to quick's own builtin.
        assert_eq!(p.lines, Some(300));
        assert_eq!(p.source, crate::grab::GrabSource::Scrollback);
        assert_eq!(p.disable, vec!["secret".to_string()]);
    }

    #[test]
    fn resolve_grab_profile_user_defines_wholly_new_name() {
        let mut config = Config::default();
        config.grab_profiles.insert(
            "jira-deep".to_string(),
            GrabProfileOverride {
                source: Some("tab".to_string()),
                lines: Some(500),
                disable: Vec::new(),
            },
        );
        let p = config.resolve_grab_profile("jira-deep");
        assert_eq!(p.source, crate::grab::GrabSource::Tab);
        assert_eq!(p.lines, Some(500));
    }

    #[test]
    fn cycle_grab_profile_names_is_just_builtins_with_zero_config() {
        let config = Config::default();
        assert_eq!(
            config.cycle_grab_profile_names(),
            crate::grab::BUILTIN_PROFILE_NAMES
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn cycle_grab_profile_names_appends_custom_names_sorted() {
        let mut config = Config::default();
        config
            .grab_profiles
            .insert("zeta-deep".to_string(), GrabProfileOverride::default());
        config
            .grab_profiles
            .insert("jira-deep".to_string(), GrabProfileOverride::default());
        let names = config.cycle_grab_profile_names();
        assert_eq!(&names[..5], crate::grab::BUILTIN_PROFILE_NAMES);
        // Custom names sorted alphabetically, not HashMap insertion order.
        assert_eq!(&names[5..], &["jira-deep", "zeta-deep"]);
    }

    #[test]
    fn cycle_grab_profile_names_overriding_a_builtin_does_not_duplicate_it() {
        let mut config = Config::default();
        config
            .grab_profiles
            .insert("deep".to_string(), GrabProfileOverride::default());
        let names = config.cycle_grab_profile_names();
        assert_eq!(names.len(), crate::grab::BUILTIN_PROFILE_NAMES.len());
        assert_eq!(names.iter().filter(|n| n.as_str() == "deep").count(), 1);
    }

    #[test]
    fn log_off_suppresses_error_level() {
        let config = Config {
            log_level: LogLevel::Off,
            ..Config::default()
        };
        // No assertion on stderr content (would need capture plumbing)
        // - this just exercises the threshold comparison for panics.
        config.log(LogLevel::Error, "should not print");
        assert!(LogLevel::Off < LogLevel::Error);
    }

    #[test]
    fn log_level_ordering_matches_verbosity() {
        assert!(LogLevel::Off < LogLevel::Error);
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
    }

    #[test]
    fn resolve_preview_open_defaults_closed() {
        let config = Config::default();
        assert!(!config.resolve_preview_open(&Profile::default()));
    }

    #[test]
    fn resolve_preview_open_ui_always_opens_with_no_override() {
        let config = Config {
            ui: UiConfig {
                preview: PreviewState::Always,
                ..UiConfig::default()
            },
            ..Config::default()
        };
        assert!(config.resolve_preview_open(&Profile::default()));
    }

    #[test]
    fn resolve_preview_open_profile_override_wins_over_ui_default() {
        let config = Config {
            ui: UiConfig {
                preview: PreviewState::Always,
                ..UiConfig::default()
            },
            ..Config::default()
        };
        let profile = Profile {
            preview: Some(PreviewOverride::Never),
            ..Profile::default()
        };
        assert!(!config.resolve_preview_open(&profile));
    }

    #[test]
    fn resolve_preview_open_profile_on_overrides_off_default() {
        let config = Config::default();
        let profile = Profile {
            preview: Some(PreviewOverride::On),
            ..Profile::default()
        };
        assert!(config.resolve_preview_open(&profile));
    }
}
