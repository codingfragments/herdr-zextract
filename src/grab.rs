//! Scrollback-depth grab profiles and multi-pane tab-wide scanning,
//! ported from the original plugin's `grab { }` config and its
//! `source = "tab"` variant. See `doc/multi-pane-grab.md` for the full
//! design-decisions table this follows.
//!
//! All multiplexer access goes through [`crate::host::Host`]: which
//! panes count as "on the tab" (and that floating/popup panes are left
//! out) is the host's business, not this module's.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::config::Config;
use crate::host::{Host, ReadSource};
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

// ── Cycle-list resolution ─────────────────────────────────────────────────────

/// Resolve the cycler's cycle list from the launching profile's
/// `grab_cycle`: keep only known names (order as configured, unknown
/// names dropped), falling back to the full list when the restriction
/// names nothing known - a typo in `grab_cycle` degrades to today's
/// cycle-everything behavior instead of leaving nothing to cycle
/// through. Pure, for unit testing without a host.
fn resolve_cycle_names(all: Vec<String>, cycle: Option<&[String]>) -> Vec<String> {
    match cycle {
        Some(list) => {
            let restricted: Vec<String> =
                list.iter().filter(|n| all.contains(n)).cloned().collect();
            if restricted.is_empty() {
                all
            } else {
                restricted
            }
        }
        None => all,
    }
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
    /// The multiplexer to capture from; shared with `main.rs`, which
    /// still needs it for Insert after the picker (which owns this
    /// cycler) has returned.
    host: Rc<dyn Host>,
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
    /// forward from there. `cycle` is the launching profile's
    /// `grab_cycle` restriction - `None` cycles through every
    /// configured grab profile. `host` supplies the source pane/tab
    /// and does the actual reads.
    pub fn new(
        config: &Config,
        raw_disabled: &HashSet<String>,
        allowed: Option<&HashSet<String>>,
        initial_name: &str,
        cycle: Option<&[String]>,
        host: Rc<dyn Host>,
    ) -> Self {
        let names = resolve_cycle_names(config.cycle_grab_profile_names(), cycle);
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
            host,
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
    /// around), advancing `current` only on success - a host failure
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
        let captures = capture(profile, self.host.as_ref())?;
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

/// Capture per `profile` from the host's source pane - or, for a
/// `tab` profile, from every pane on the source tab.
pub fn capture(profile: &ResolvedGrabProfile, host: &dyn Host) -> Result<Vec<PaneCapture>, String> {
    let source_pane = &host.source().pane_id;
    match profile.source {
        GrabSource::Tab => capture_tab(host, profile.lines),
        GrabSource::Viewport | GrabSource::Scrollback => {
            let source = if profile.source == GrabSource::Viewport {
                ReadSource::Viewport
            } else {
                ReadSource::Scrollback
            };
            let text = host.read_pane(source_pane, source, profile.lines)?;
            Ok(vec![PaneCapture {
                pane_id: source_pane.clone(),
                title: String::new(),
                text,
            }])
        }
    }
}

/// Every pane on the source tab, in the host's order (source pane
/// first). Panes whose scrollback can't be fetched (closed mid-grab)
/// are skipped silently, per the original's failure-handling rule.
fn capture_tab(host: &dyn Host, lines: Option<u32>) -> Result<Vec<PaneCapture>, String> {
    let mut out = Vec::new();
    for pane in host.list_tab_panes()? {
        let Ok(text) = host.read_pane(&pane.pane_id, ReadSource::Scrollback, lines) else {
            continue;
        };
        out.push(PaneCapture {
            pane_id: pane.pane_id,
            title: pane.title,
            text,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::host::fake::{FakeHost, ReadCall};

    fn target() -> Rc<dyn Host> {
        Rc::new(FakeHost::new("pane1", "tab1"))
    }

    fn cycler(config: &Config, initial_name: &str) -> GrabCycler {
        GrabCycler::new(config, &HashSet::new(), None, initial_name, None, target())
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
    fn grab_cycle_restriction_keeps_listed_order_and_drops_unknown() {
        let all = vec![
            "quick".to_string(),
            "deep".to_string(),
            "viewport".to_string(),
            "full".to_string(),
            "tab-scan".to_string(),
        ];
        let restricted = resolve_cycle_names(
            all.clone(),
            Some(&["full".to_string(), "nope".to_string(), "deep".to_string()]),
        );
        assert_eq!(restricted, vec!["full".to_string(), "deep".to_string()]);
        // No restriction → the full list, order unchanged.
        assert_eq!(resolve_cycle_names(all.clone(), None), all);
        // All-unknown restriction → falls back to the full cycle
        // rather than leaving nothing to cycle through.
        assert_eq!(
            resolve_cycle_names(all.clone(), Some(&["nope".to_string()])),
            all
        );
    }

    #[test]
    fn grab_cycle_restriction_limits_the_cycler_and_starts_at_grab() {
        let config = Config::default();
        let c = GrabCycler::new(
            &config,
            &HashSet::new(),
            None,
            "deep",
            Some(&["full".to_string(), "deep".to_string()]),
            target(),
        );
        // Starts at the profile's `grab` name, inside the restricted
        // list; `quick` (the unrestricted default start) is not in it.
        assert_eq!(c.current_name(), "deep");
        // A `grab` name outside the restricted list starts at its head.
        let c = GrabCycler::new(
            &config,
            &HashSet::new(),
            None,
            "quick",
            Some(&["full".to_string(), "deep".to_string()]),
            target(),
        );
        assert_eq!(c.current_name(), "full");
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

    fn profile(source: GrabSource, lines: Option<u32>) -> ResolvedGrabProfile {
        ResolvedGrabProfile {
            source,
            lines,
            disable: Vec::new(),
        }
    }

    #[test]
    fn capture_scrollback_reads_the_source_pane_with_the_line_cap() {
        let host = FakeHost::new("p1", "t1")
            .with_pane("p1", "one", "hello")
            .with_pane("p2", "two", "other");
        let caps = capture(&profile(GrabSource::Scrollback, Some(150)), &host).unwrap();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].pane_id, "p1");
        assert_eq!(caps[0].title, "");
        assert_eq!(caps[0].text, "hello");
        assert_eq!(
            host.reads.borrow().as_slice(),
            &[ReadCall {
                pane_id: "p1".into(),
                source: ReadSource::Scrollback,
                lines: Some(150)
            }]
        );
    }

    #[test]
    fn capture_viewport_reads_the_viewport() {
        let host = FakeHost::new("p1", "t1").with_pane("p1", "one", "screen");
        capture(&profile(GrabSource::Viewport, None), &host).unwrap();
        assert_eq!(host.reads.borrow()[0].source, ReadSource::Viewport);
        assert_eq!(host.reads.borrow()[0].lines, None);
    }

    #[test]
    fn capture_source_read_failure_is_an_error() {
        let host = FakeHost::new("gone", "t1");
        assert!(capture(&profile(GrabSource::Scrollback, None), &host).is_err());
    }

    #[test]
    fn capture_tab_reads_every_listed_pane_and_skips_unreadable_ones() {
        let mut host = FakeHost::new("p2", "t1")
            .with_pane("p2", "editor", "two")
            .with_pane("p1", "shell", "one");
        // Listed on the tab but unreadable (closed mid-grab).
        host.tab.push(crate::host::PaneInfo {
            pane_id: "p3".into(),
            title: "gone".into(),
        });
        let caps = capture(&profile(GrabSource::Tab, Some(150)), &host).unwrap();
        let got: Vec<(&str, &str, &str)> = caps
            .iter()
            .map(|c| (c.pane_id.as_str(), c.title.as_str(), c.text.as_str()))
            .collect();
        assert_eq!(got, vec![("p2", "editor", "two"), ("p1", "shell", "one")]);
        assert!(host
            .reads
            .borrow()
            .iter()
            .all(|r| r.source == ReadSource::Scrollback && r.lines == Some(150)));
    }

    #[test]
    fn grab_cycler_tags_matches_with_their_pane() {
        let host: Rc<dyn Host> = Rc::new(
            FakeHost::new("p1", "t1")
                .with_pane("p1", "one", "see https://example.com/a")
                .with_pane("p2", "two", "and https://example.com/b"),
        );
        let config = Config::default();
        let c = GrabCycler::new(&config, &HashSet::new(), None, "tab-scan", None, host);
        let result = c.capture_current().unwrap();
        assert!(!result.matches.is_empty());
        assert!(result
            .matches
            .iter()
            .all(|m| m.fields.contains_key("__pane_id") && m.fields.contains_key("__pane_title")));
        assert_eq!(result.pane_texts.len(), 2);
    }
}
