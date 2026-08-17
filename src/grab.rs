//! Scrollback-depth grab profiles and multi-pane tab-wide scanning,
//! ported from the original plugin's `grab { }` config and its
//! `source = "tab"` variant. See `doc/multi-pane-grab.md` for the full
//! design-decisions table this follows.
//!
//! Unlike the original, Herdr has no floating-pane concept and plugin
//! popups never appear in `pane.list` (confirmed live) - so "every
//! non-floating, non-plugin pane on the active tab" reduces to simply
//! every pane `pane.list` returns for that tab, no extra filtering.

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
