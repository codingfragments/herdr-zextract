//! Scrollback-depth grab profiles and multi-pane tab-wide scanning,
//! ported from the original plugin's `grab { }` config and its
//! `source = "tab"` variant. See `doc/multi-pane-grab.md` for the full
//! design-decisions table this follows.
//!
//! Unlike the original, Herdr has no floating-pane concept and plugin
//! popups never appear in `pane.list` (confirmed live) - so "every
//! non-floating, non-plugin pane on the active tab" reduces to simply
//! every pane `pane.list` returns for that tab, no extra filtering.

use std::collections::{HashMap, HashSet};

use crate::config::Config;
use crate::matcher::{self, Match};

/// The five grab profiles with built-in Rust-side definitions, in the
/// fixed order [`Config::cycle_grab_profile_names`] and [`GrabCycler`]
/// present them in - custom names defined under `[grab_profiles.<name>]`
/// are appended after these, sorted alphabetically for a stable order.
pub const BUILTIN_PROFILE_NAMES: &[&str] = &["quick", "deep", "viewport", "full", "tab-scan"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabSource {
    Scrollback,
    Viewport,
    Tab,
}

impl GrabSource {
    /// Parses `[grab_profiles.<name>].source`'s string form.
    /// Unrecognized or absent falls back to `Scrollback`, matching the
    /// original's own default.
    pub fn parse(s: &str) -> Self {
        match s {
            "viewport" => Self::Viewport,
            "tab" => Self::Tab,
            _ => Self::Scrollback,
        }
    }
}

/// A fully-resolved grab profile, ready to capture with - the merge of
/// a built-in definition (if any) and a user's `[grab_profiles.<name>]`
/// override (if any), done by `Config::resolve_grab_profile`.
#[derive(Debug, Clone)]
pub struct ResolvedGrabProfile {
    pub source: GrabSource,
    /// Max lines to scan. `None` means unbounded.
    pub lines: Option<u32>,
    /// Pattern type tags/custom pattern names to skip only while this
    /// profile is active - merged into the invocation's disable list.
    pub disable: Vec<String>,
}

/// Built-in profile definitions, matching the original's own defaults
/// exactly for `quick`/`deep`/`viewport`/`full`. `tab-scan` is a
/// herdr-zextract-specific addition - the original doesn't ship it as
/// a built-in default, only as an opt-in example in its docs; this
/// port always has it available since `doc/keybinding.md`'s `tab`
/// profile depends on it existing with zero user config.
pub fn builtin_grab_profile(name: &str) -> Option<ResolvedGrabProfile> {
    let (source, lines) = match name {
        "quick" => (GrabSource::Scrollback, Some(150)),
        "deep" => (GrabSource::Scrollback, Some(1500)),
        "viewport" => (GrabSource::Viewport, None),
        "full" => (GrabSource::Scrollback, None),
        "tab-scan" => (GrabSource::Tab, Some(150)),
        _ => return None,
    };
    Some(ResolvedGrabProfile {
        source,
        lines,
        disable: Vec::new(),
    })
}

pub struct PaneCapture {
    pub pane_id: String,
    /// Pane title for the dim `[title]  ` list prefix, shown only when
    /// more than one pane contributes matches. Empty in single-pane mode.
    pub title: String,
    pub text: String,
}

/// Cycles the picker's live capture through every configured grab
/// profile (`Ctrl-G`), re-capturing and re-extracting on each step
/// without leaving `main.rs`'s one-shot launch flow — the picker owns
/// one of these for the duration of a session.
pub struct GrabCycler {
    names: Vec<String>,
    profiles: Vec<ResolvedGrabProfile>,
    /// Per-name effective `disabled` set, precomputed once at
    /// construction since neither the launch profile's allowlist nor
    /// `[patterns].disable` change during a picker session - only which
    /// grab profile's own `disable` list (if any) gets merged in varies.
    disabled_sets: Vec<HashSet<String>>,
    current: usize,
    socket_path: String,
    focused_pane_id: String,
    tab_id: String,
    /// Clone of the launch `Config`, so re-extraction sees the same
    /// `custom` patterns/etc. as the initial capture - `disabled` is
    /// overwritten per call from `disabled_sets`, never read from here.
    config_template: Config,
}

impl GrabCycler {
    /// `raw_disabled` is `[patterns].disable` before any grab-profile or
    /// launch-allowlist adjustment. `allowed` is the launching profile's
    /// `patterns` allowlist, if set - when present, every grabber in the
    /// cycle uses that same fixed complement and ignores each grab
    /// profile's own `disable` list, matching `Config::restrict_to`'s
    /// "allowlist overrides every disable source" rule. `initial_name`
    /// is the launching profile's `grab` field (or `"quick"`); cycling
    /// starts there (falling back to index 0 if unrecognized) and wraps
    /// forward from there.
    pub fn new(
        config: &Config,
        raw_disabled: &HashSet<String>,
        allowed: Option<&HashSet<String>>,
        initial_name: &str,
        socket_path: String,
        focused_pane_id: String,
        tab_id: String,
    ) -> Self {
        let names = config.cycle_grab_profile_names();
        let profiles: Vec<ResolvedGrabProfile> = names
            .iter()
            .map(|n| config.resolve_grab_profile(n))
            .collect();
        let disabled_sets: Vec<HashSet<String>> = profiles
            .iter()
            .map(|gp| config.disabled_for(allowed, raw_disabled, &gp.disable))
            .collect();
        let current = names.iter().position(|n| n == initial_name).unwrap_or(0);
        Self {
            names,
            profiles,
            disabled_sets,
            current,
            socket_path,
            focused_pane_id,
            tab_id,
            config_template: config.clone(),
        }
    }

    pub fn current_name(&self) -> &str {
        &self.names[self.current]
    }

    /// The resolved grab profile the cycle currently points at - lets
    /// the picker's grab-label display show the active line cap
    /// (`source`/`lines`) alongside the name, without re-resolving it.
    pub fn current_profile(&self) -> &ResolvedGrabProfile {
        &self.profiles[self.current]
    }

    /// Capture + extract with the grabber the cycle currently points
    /// at, used for the picker's initial load so `main.rs` doesn't
    /// duplicate the multi-pane/`__pane_id` handling done here.
    pub fn capture_current(&self) -> Result<CaptureResult, String> {
        self.capture_and_extract(self.current)
    }

    /// Capture + extract with the next grabber in the cycle (wrapping
    /// around), advancing `current` only on success - a socket failure
    /// leaves the picker's existing matches and displayed grabber name
    /// untouched rather than clearing the list.
    pub fn cycle_next(&mut self) -> Result<CaptureResult, String> {
        let next = (self.current + 1) % self.names.len();
        let result = self.capture_and_extract(next)?;
        self.current = next;
        Ok(result)
    }

    fn capture_and_extract(&self, index: usize) -> Result<CaptureResult, String> {
        let profile = &self.profiles[index];
        let captures = capture(
            profile,
            &self.socket_path,
            &self.focused_pane_id,
            &self.tab_id,
        )?;
        let multi_pane = captures.len() > 1;
        let mut config = self.config_template.clone();
        config.disabled = self.disabled_sets[index].clone();
        let mut matches = Vec::new();
        let mut pane_texts = HashMap::new();
        for cap in &captures {
            let mut found = matcher::extract_with_config(&cap.text, &config);
            // `__pane_id` is always set (not just in multi-pane mode) -
            // the preview pane needs it to find a match's source text
            // regardless of how many panes contributed. `__pane_title`
            // stays multi-pane-only; it only ever drives the list's
            // `[title]` prefix, which is itself multi-pane-only.
            for m in &mut found {
                m.fields
                    .insert("__pane_id".to_string(), cap.pane_id.clone());
                if multi_pane {
                    m.fields
                        .insert("__pane_title".to_string(), cap.title.clone());
                }
            }
            matches.extend(found);
            pane_texts.insert(cap.pane_id.clone(), cap.text.clone());
        }
        Ok(CaptureResult {
            matches,
            pane_texts,
        })
    }
}

/// One capture+extract cycle's result: the matches found, plus every
/// contributing pane's full captured text (keyed by pane id) - kept
/// around so the preview pane can slice ±3 lines of context around a
/// match's byte offset without re-reading the pane.
pub struct CaptureResult {
    pub matches: Vec<Match>,
    pub pane_texts: HashMap<String, String>,
}

/// Capture scrollback per `profile`. `focused_pane_id`/`tab_id` come
/// from the launch context (`HERDR_PLUGIN_CONTEXT_JSON`).
pub fn capture(
    profile: &ResolvedGrabProfile,
    socket_path: &str,
    focused_pane_id: &str,
    tab_id: &str,
) -> Result<Vec<PaneCapture>, String> {
    match profile.source {
        GrabSource::Tab => capture_tab(socket_path, focused_pane_id, tab_id, profile.lines),
        _ => {
            let source = if profile.source == GrabSource::Viewport {
                "visible"
            } else {
                "recent_unwrapped"
            };
            let text = read_pane(socket_path, focused_pane_id, source, profile.lines)?;
            Ok(vec![PaneCapture {
                pane_id: focused_pane_id.to_string(),
                title: String::new(),
                text,
            }])
        }
    }
}

fn read_pane(
    socket_path: &str,
    pane_id: &str,
    source: &str,
    lines: Option<u32>,
) -> Result<String, String> {
    let mut params = serde_json::json!({"pane_id": pane_id, "source": source});
    if let Some(n) = lines {
        params["lines"] = serde_json::json!(n);
    }
    let result = crate::socket_client::request(socket_path, "pane.read", params)
        .map_err(|e| format!("pane.read failed: {e}"))?;
    result
        .get("read")
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "pane.read response had no \"read.text\" field".to_string())
}

fn pane_title(pane: &serde_json::Value) -> String {
    pane.get("label")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            pane.get("terminal_title_stripped")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })
        .map(str::to_string)
        .unwrap_or_else(|| {
            let id = pane.get("pane_id").and_then(|v| v.as_str()).unwrap_or("?");
            format!("pane {id}")
        })
}

/// Every pane on `tab_id`, last-focused pane first (matching the
/// original's ordering), remaining panes in `pane.list`'s own order.
/// Panes whose scrollback can't be fetched (closed mid-grab) are
/// skipped silently, per the original's failure-handling rule.
fn capture_tab(
    socket_path: &str,
    focused_pane_id: &str,
    tab_id: &str,
    lines: Option<u32>,
) -> Result<Vec<PaneCapture>, String> {
    let result = crate::socket_client::request(socket_path, "pane.list", serde_json::json!({}))
        .map_err(|e| format!("pane.list failed: {e}"))?;
    let panes = result
        .get("panes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut on_tab: Vec<serde_json::Value> = panes
        .into_iter()
        .filter(|p| p.get("tab_id").and_then(|v| v.as_str()) == Some(tab_id))
        .collect();
    on_tab.sort_by_key(|p| p.get("pane_id").and_then(|v| v.as_str()) != Some(focused_pane_id));

    let mut out = Vec::new();
    for pane in &on_tab {
        let Some(pane_id) = pane.get("pane_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Ok(text) = read_pane(socket_path, pane_id, "recent_unwrapped", lines) else {
            continue;
        };
        out.push(PaneCapture {
            pane_id: pane_id.to_string(),
            title: pane_title(pane),
            text,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycler(config: &Config, initial_name: &str) -> GrabCycler {
        GrabCycler::new(
            config,
            &HashSet::new(),
            None,
            initial_name,
            "/tmp/socket".to_string(),
            "pane1".to_string(),
            "tab1".to_string(),
        )
    }

    #[test]
    fn grab_cycler_starts_at_the_named_initial_profile() {
        let config = Config::default();
        let c = cycler(&config, "deep");
        assert_eq!(c.current_name(), "deep");
    }

    #[test]
    fn grab_cycler_unrecognized_initial_name_falls_back_to_first() {
        let config = Config::default();
        let c = cycler(&config, "nonexistent");
        assert_eq!(c.current_name(), BUILTIN_PROFILE_NAMES[0]);
    }

    #[test]
    fn grab_cycler_names_include_custom_grab_profiles() {
        let mut config = Config::default();
        config.grab_profiles.insert(
            "jira-deep".to_string(),
            crate::config::GrabProfileOverride::default(),
        );
        let c = cycler(&config, "jira-deep");
        assert_eq!(c.current_name(), "jira-deep");
    }

    #[test]
    fn builtin_grab_profile_known_name() {
        let p = builtin_grab_profile("deep").unwrap();
        assert_eq!(p.lines, Some(1500));
    }

    #[test]
    fn builtin_grab_profile_unknown_name_returns_none() {
        assert!(builtin_grab_profile("nonexistent").is_none());
        assert!(builtin_grab_profile("").is_none());
    }

    #[test]
    fn tab_scan_profile_has_per_pane_line_cap() {
        let p = builtin_grab_profile("tab-scan").unwrap();
        assert_eq!(p.source, GrabSource::Tab);
        assert_eq!(p.lines, Some(150));
    }

    #[test]
    fn full_profile_is_unbounded() {
        assert_eq!(builtin_grab_profile("full").unwrap().lines, None);
    }

    #[test]
    fn grab_source_parse_recognizes_all_variants() {
        assert_eq!(GrabSource::parse("viewport"), GrabSource::Viewport);
        assert_eq!(GrabSource::parse("tab"), GrabSource::Tab);
        assert_eq!(GrabSource::parse("scrollback"), GrabSource::Scrollback);
    }

    #[test]
    fn grab_source_parse_unknown_falls_back_to_scrollback() {
        assert_eq!(GrabSource::parse("bogus"), GrabSource::Scrollback);
    }

    #[test]
    fn pane_title_prefers_label() {
        let pane = serde_json::json!({"pane_id": "w1:p1", "label": "editor", "terminal_title_stripped": "nvim"});
        assert_eq!(pane_title(&pane), "editor");
    }

    #[test]
    fn pane_title_falls_back_to_terminal_title() {
        let pane = serde_json::json!({"pane_id": "w1:p1", "terminal_title_stripped": "nvim"});
        assert_eq!(pane_title(&pane), "nvim");
    }

    #[test]
    fn pane_title_falls_back_to_pane_id() {
        let pane = serde_json::json!({"pane_id": "w1:p1"});
        assert_eq!(pane_title(&pane), "pane w1:p1");
    }
}
