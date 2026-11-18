//! [`Host`] for Herdr, over its newline-delimited JSON socket API
//! (`$HERDR_SOCKET_PATH`).
//!
//! Herdr has no floating-pane concept and plugin popups never appear in
//! `pane.list` (confirmed live), so "every pane on the source tab"
//! needs no extra filtering here.

use super::socket_client;
use super::{Host, PaneInfo, ReadSource, SourceRef};

pub struct HerdrHost {
    socket_path: String,
    source: SourceRef,
}

impl HerdrHost {
    /// Build from Herdr's plugin environment: the launch context
    /// (`HERDR_PLUGIN_CONTEXT_JSON`, or the `HERDR_ACTIVE_PANE_ID`
    /// dev-testing fallback) and `HERDR_SOCKET_PATH`.
    pub fn from_env() -> Result<Self, String> {
        let source = launch_context(
            std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok().as_deref(),
            std::env::var("HERDR_ACTIVE_PANE_ID").ok().as_deref(),
        )?;
        let socket_path = std::env::var("HERDR_SOCKET_PATH")
            .map_err(|_| "HERDR_SOCKET_PATH is not set".to_string())?;
        Ok(Self {
            socket_path,
            source,
        })
    }
}

impl Host for HerdrHost {
    fn name(&self) -> &'static str {
        "herdr"
    }

    fn source(&self) -> &SourceRef {
        &self.source
    }

    fn read_pane(
        &self,
        pane_id: &str,
        source: ReadSource,
        lines: Option<u32>,
    ) -> Result<String, String> {
        let source = match source {
            ReadSource::Viewport => "visible",
            ReadSource::Scrollback => "recent_unwrapped",
        };
        let mut params = serde_json::json!({"pane_id": pane_id, "source": source});
        if let Some(n) = lines {
            params["lines"] = serde_json::json!(n);
        }
        let result = socket_client::request(&self.socket_path, "pane.read", params)
            .map_err(|e| format!("pane.read failed: {e}"))?;
        result
            .get("read")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| "pane.read response had no \"read.text\" field".to_string())
    }

    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String> {
        let result = socket_client::request(&self.socket_path, "pane.list", serde_json::json!({}))
            .map_err(|e| format!("pane.list failed: {e}"))?;
        Ok(panes_on_tab(
            &result,
            &self.source.tab_id,
            &self.source.pane_id,
        ))
    }

    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String> {
        socket_client::request(
            &self.socket_path,
            "pane.send_text",
            serde_json::json!({"pane_id": pane_id, "text": text}),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}

/// Reads the launch context from either path: a real plugin-pane
/// invocation sets `HERDR_PLUGIN_CONTEXT_JSON`; a `[[keys.command]]`
/// custom-command popup (used for manual dev-testing ahead of a real
/// plugin keybind) sets `HERDR_ACTIVE_PANE_ID` directly, with no tab
/// context - tab-wide grabs then find zero panes, which is fine for a
/// manual dev keybind. Pure (env values passed in) for unit testing.
fn launch_context(
    context_json: Option<&str>,
    active_pane_id: Option<&str>,
) -> Result<SourceRef, String> {
    if let Some(context_json) = context_json {
        let context: serde_json::Value =
            serde_json::from_str(context_json).map_err(|e| format!("invalid context JSON: {e}"))?;
        let pane_id = context
            .get("focused_pane_id")
            .and_then(|v| v.as_str())
            .ok_or(
                "context JSON has no focused_pane_id (nothing was focused before this popup opened)",
            )?
            .to_string();
        let tab_id = context
            .get("tab_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(SourceRef { pane_id, tab_id });
    }
    let pane_id = active_pane_id
        .ok_or_else(|| {
            "neither HERDR_PLUGIN_CONTEXT_JSON nor HERDR_ACTIVE_PANE_ID is set".to_string()
        })?
        .to_string();
    Ok(SourceRef {
        pane_id,
        tab_id: String::new(),
    })
}

/// Every pane of a `pane.list` result that sits on `tab_id`, the
/// source pane first (matching the original plugin's ordering), the
/// rest in `pane.list`'s own order.
fn panes_on_tab(list_result: &serde_json::Value, tab_id: &str, source_pane: &str) -> Vec<PaneInfo> {
    let panes = list_result
        .get("panes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut on_tab: Vec<serde_json::Value> = panes
        .into_iter()
        .filter(|p| p.get("tab_id").and_then(|v| v.as_str()) == Some(tab_id))
        .collect();
    on_tab.sort_by_key(|p| p.get("pane_id").and_then(|v| v.as_str()) != Some(source_pane));
    on_tab
        .iter()
        .filter_map(|pane| {
            let pane_id = pane.get("pane_id").and_then(|v| v.as_str())?;
            Some(PaneInfo {
                pane_id: pane_id.to_string(),
                title: pane_title(pane),
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn panes_on_tab_filters_by_tab_and_puts_source_first() {
        let list = serde_json::json!({"panes": [
            {"pane_id": "p1", "tab_id": "t1", "label": "one"},
            {"pane_id": "p2", "tab_id": "t2", "label": "other tab"},
            {"pane_id": "p3", "tab_id": "t1", "label": "three"},
            {"pane_id": "p4", "tab_id": "t1"},
        ]});
        let panes = panes_on_tab(&list, "t1", "p3");
        let ids: Vec<&str> = panes.iter().map(|p| p.pane_id.as_str()).collect();
        assert_eq!(ids, vec!["p3", "p1", "p4"]);
        assert_eq!(panes[0].title, "three");
        assert_eq!(panes[2].title, "pane p4");
    }

    #[test]
    fn panes_on_tab_empty_tab_id_finds_nothing() {
        let list = serde_json::json!({"panes": [{"pane_id": "p1", "tab_id": "t1"}]});
        assert!(panes_on_tab(&list, "", "p1").is_empty());
    }

    #[test]
    fn launch_context_reads_plugin_context_json() {
        let src = launch_context(
            Some(r#"{"focused_pane_id": "w1:p2", "tab_id": "w1:t1"}"#),
            Some("ignored"),
        )
        .unwrap();
        assert_eq!(
            src,
            SourceRef {
                pane_id: "w1:p2".into(),
                tab_id: "w1:t1".into()
            }
        );
    }

    #[test]
    fn launch_context_falls_back_to_active_pane_id_without_tab() {
        let src = launch_context(None, Some("w1:p9")).unwrap();
        assert_eq!(src.pane_id, "w1:p9");
        assert_eq!(src.tab_id, "");
    }

    #[test]
    fn launch_context_errors_without_any_source() {
        assert!(launch_context(None, None).is_err());
        assert!(launch_context(Some(r#"{"tab_id": "t"}"#), None).is_err());
        assert!(launch_context(Some("not json"), None).is_err());
    }
}
