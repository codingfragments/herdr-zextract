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

#[derive(Debug, Clone)]
pub struct GrabProfile {
    pub name: &'static str,
    pub source: GrabSource,
    pub lines: Option<u32>,
}

/// Built-in profiles, matching the original's defaults exactly.
pub const PROFILES: &[GrabProfile] = &[
    GrabProfile {
        name: "quick",
        source: GrabSource::Scrollback,
        lines: Some(150),
    },
    GrabProfile {
        name: "deep",
        source: GrabSource::Scrollback,
        lines: Some(1500),
    },
    GrabProfile {
        name: "viewport",
        source: GrabSource::Viewport,
        lines: None,
    },
    GrabProfile {
        name: "full",
        source: GrabSource::Scrollback,
        lines: None,
    },
    GrabProfile {
        name: "tab-scan",
        source: GrabSource::Tab,
        lines: Some(150),
    },
];

/// Resolve `name` to a profile, falling back to the first defined
/// profile (`quick`) on an empty or unknown name - matches the
/// original's "typos fall back to the first profile" behavior.
pub fn resolve(name: &str) -> &'static GrabProfile {
    PROFILES
        .iter()
        .find(|p| p.name == name)
        .unwrap_or(&PROFILES[0])
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
    profile: &GrabProfile,
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
    fn resolve_known_profile() {
        assert_eq!(resolve("deep").name, "deep");
        assert_eq!(resolve("deep").lines, Some(1500));
    }

    #[test]
    fn resolve_unknown_falls_back_to_first_profile() {
        assert_eq!(resolve("nonexistent").name, "quick");
    }

    #[test]
    fn resolve_empty_falls_back_to_first_profile() {
        assert_eq!(resolve("").name, "quick");
    }

    #[test]
    fn tab_scan_profile_has_per_pane_line_cap() {
        let p = resolve("tab-scan");
        assert_eq!(p.source, GrabSource::Tab);
        assert_eq!(p.lines, Some(150));
    }

    #[test]
    fn full_profile_is_unbounded() {
        assert_eq!(resolve("full").lines, None);
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
