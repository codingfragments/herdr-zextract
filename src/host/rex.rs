//! [`Host`] for Rex (superlogical's terminal multiplexer), through the
//! `rex` CLI.
//!
//! The tool runs in a floating layer that the Rex launcher
//! (`rex/zextract.lua`) opens. Inside that layer `REX_BLOCK` is the
//! layer's own block, never the pane the user was in, so the launcher
//! passes the source explicitly:
//!
//! | Env var | Meaning |
//! |---|---|
//! | `REX_SESSION` | session to work in (set by Rex in every terminal) |
//! | `<PREFIX>_SOURCE_BLOCK` | the pane to grab from and insert into (required) |
//! | `<PREFIX>_SOURCE_WINDOW` | its window (tab); found from the session if unset |
//! | `REX_CLI` | path of the `rex` CLI (default: `rex` on `PATH`) |

use std::io::Write;
use std::process::{Command, Stdio};

use super::{Host, PaneInfo, ReadSource, SourceRef};

/// Runs one `rex` CLI invocation and returns its stdout. A trait so
/// tests can check the exact arguments without a Rex server.
pub trait RexRunner {
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String>;
}

/// The real runner: spawns the `rex` CLI.
pub struct CliRunner {
    pub cli: String,
}

impl RexRunner for CliRunner {
    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        let mut child = Command::new(&self.cli)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!(
                    "cannot run the rex CLI ({}): {e} - set REX_CLI to its path",
                    self.cli
                )
            })?;
        if let Some(text) = stdin {
            if let Some(mut pipe) = child.stdin.take() {
                pipe.write_all(text.as_bytes())
                    .map_err(|e| format!("rex {}: writing stdin failed: {e}", args[0]))?;
            }
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("rex {}: {e}", args[0]))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.trim().trim_start_matches("Error: ");
            return Err(format!("rex {} failed: {err}", args[0]));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

pub struct RexHost {
    runner: Box<dyn RexRunner>,
    session: String,
    source: SourceRef,
}

impl RexHost {
    /// Build from the environment the Rex launcher sets (see the module
    /// docs). `prefix` is the tool's env prefix, e.g. `"ZEXTRACT"`.
    pub fn from_env(prefix: &str) -> Result<Self, String> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let session =
            var("REX_SESSION").ok_or("REX_SESSION is not set (not running inside Rex?)")?;
        let block_var = format!("{prefix}_SOURCE_BLOCK");
        let pane_id = var(&block_var).ok_or_else(|| {
            format!(
                "{block_var} is not set: open this through the Rex launcher \
                 (rex/zextract.lua), which passes the pane to grab from"
            )
        })?;
        let tab_id = var(&format!("{prefix}_SOURCE_WINDOW")).unwrap_or_default();
        let cli = var("REX_CLI").unwrap_or_else(|| "rex".to_string());
        Ok(Self::new(
            Box::new(CliRunner { cli }),
            session,
            SourceRef { pane_id, tab_id },
        ))
    }

    pub fn new(runner: Box<dyn RexRunner>, session: String, source: SourceRef) -> Self {
        Self {
            runner,
            session,
            source,
        }
    }

    fn rex(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
        self.runner.run(args, stdin)
    }

    /// The terminal's visible row count (`block call … size`).
    fn rows(&self, pane_id: &str) -> Result<usize, String> {
        let out = self.rex(
            &["block", "call", "-s", &self.session, "-b", pane_id, "size"],
            None,
        )?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex size: bad JSON: {e}"))?;
        v.get("rows")
            .and_then(|r| r.as_u64())
            .map(|r| r as usize)
            .ok_or_else(|| "rex size: no \"rows\" field".to_string())
    }

    /// The terminal title (`block call … title`), empty if unknown.
    fn title(&self, pane_id: &str) -> String {
        self.rex(
            &["block", "call", "-s", &self.session, "-b", pane_id, "title"],
            None,
        )
        .ok()
        .and_then(|out| serde_json::from_str::<serde_json::Value>(&out).ok())
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default()
    }
}

impl Host for RexHost {
    fn name(&self) -> &'static str {
        "rex"
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
        let session = self.session.as_str();
        match source {
            ReadSource::Scrollback => {
                let text = self.rex(
                    &[
                        "capture", "-s", session, "-b", pane_id, "--unwrap", "--trim",
                    ],
                    None,
                )?;
                Ok(last_lines(&text, lines.map(|n| n as usize)))
            }
            ReadSource::Viewport => {
                // The screen is the bottom `rows` rows of the capture.
                // Screen rows are wrapped rows, so no --unwrap here.
                let text = self.rex(&["capture", "-s", session, "-b", pane_id], None)?;
                let rows = self.rows(pane_id).ok();
                let cap = match (rows, lines) {
                    (Some(r), Some(n)) => Some(r.min(n as usize)),
                    (Some(r), None) => Some(r),
                    (None, n) => n.map(|n| n as usize),
                };
                Ok(last_lines(&text, cap))
            }
        }
    }

    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String> {
        let out = self.rex(&["session", "inspect", &self.session, "--json"], None)?;
        let view: serde_json::Value = serde_json::from_str(&out)
            .map_err(|e| format!("rex session inspect: bad JSON: {e}"))?;
        let panes = tab_blocks(&view, &self.source.tab_id, &self.source.pane_id);
        Ok(panes
            .into_iter()
            .map(|(pane_id, label)| {
                let title = pane_title(&label, || self.title(&pane_id), &pane_id);
                PaneInfo { pane_id, title }
            })
            .collect())
    }

    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String> {
        // Through stdin, so text starting with "-" or containing
        // newlines arrives exactly as given.
        self.rex(&["send", "-s", &self.session, "-b", pane_id], Some(text))
            .map(|_| ())
    }
}

/// Keep the last `n` lines of `text` (all of it for `None`).
fn last_lines(text: &str, n: Option<usize>) -> String {
    let Some(n) = n else {
        return text.to_string();
    };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    lines[lines.len().saturating_sub(n)..].concat()
}

/// The terminal blocks of the source window's tiled layers, as
/// `(block_id, label)`, the source block first and the rest in Rex's
/// order. Floating layers (the tool's own popup among them) are left
/// out. The window is `window_hint` if given and present, else the
/// window holding `source_block`, else the session's active window.
fn tab_blocks(
    view: &serde_json::Value,
    window_hint: &str,
    source_block: &str,
) -> Vec<(String, String)> {
    let windows = view
        .get("windows")
        .and_then(|w| w.as_array())
        .cloned()
        .unwrap_or_default();
    let str_of = |v: &serde_json::Value, k: &str| {
        v.get(k)
            .and_then(|s| s.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let blocks_of = |w: &serde_json::Value, tiled_only: bool| -> Vec<serde_json::Value> {
        w.get("layers")
            .and_then(|l| l.as_array())
            .into_iter()
            .flatten()
            .filter(|layer| !tiled_only || str_of(layer, "kind") == "tiled")
            .flat_map(|layer| {
                layer
                    .get("blocks")
                    .and_then(|b| b.as_array())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    };
    let window = windows
        .iter()
        .find(|w| !window_hint.is_empty() && str_of(w, "window_id") == window_hint)
        .or_else(|| {
            windows.iter().find(|w| {
                blocks_of(w, false)
                    .iter()
                    .any(|b| str_of(b, "block_id") == source_block)
            })
        })
        .or_else(|| {
            let active = str_of(view, "active_window_id");
            windows.iter().find(|w| str_of(w, "window_id") == active)
        });
    let Some(window) = window else {
        return Vec::new();
    };
    let mut blocks: Vec<(String, String)> = blocks_of(window, true)
        .iter()
        .filter(|b| str_of(b, "creator_name").starts_with("com.superlogical.terminal"))
        .map(|b| (str_of(b, "block_id"), str_of(b, "label")))
        .filter(|(id, _)| !id.is_empty())
        .collect();
    blocks.sort_by_key(|(id, _)| id != source_block);
    blocks
}

/// A pane's title: a label the user set, else the terminal title, else
/// the generic label, else `pane <id>`. Rex labels every new shell
/// block "shell", which says nothing; the terminal title ("~/src",
/// "nvim …") is more useful then. `title` is only called when needed.
fn pane_title(label: &str, title: impl FnOnce() -> String, pane_id: &str) -> String {
    if !label.is_empty() && label != "shell" {
        return label.to_string();
    }
    let t = title();
    if !t.is_empty() {
        return t;
    }
    if !label.is_empty() {
        return label.to_string();
    }
    format!("pane {pane_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    const S: &str = "session:01a1ec33-7bf2-7de6-939b-0b2e3c830260";
    const W1: &str = "window:01a1ec33-7c27-7910-a246-dd905785be2b";
    const W2: &str = "window:01a1ec33-7cab-7e30-a0e4-17635520f676";
    const B1: &str = "block:01a1ec33-7bf2-7ee8-9dd0-5e4037155b99";
    const B2: &str = "block:01a1ec33-7c70-772f-8d2e-9ba203f9e761";
    const B3: &str = "block:01a1ec33-7c9d-72d6-a30f-3ad1663414ca";
    const POPUP: &str = "block:01a1ec33-9897-7111-b5bd-4fa7659f4c90";

    fn fixture() -> serde_json::Value {
        // Recorded from a live Rex session (rex 0.1.0-preview.20): two
        // windows; W1 has two tiled shells and a floating "popup".
        serde_json::from_str(include_str!(
            "../../tests/fixtures/rex/session-inspect.json"
        ))
        .unwrap()
    }

    type Call = (Vec<String>, Option<String>);

    /// Records every call and answers from a queue of canned replies.
    #[derive(Clone, Default)]
    struct FakeRunner {
        calls: Rc<RefCell<Vec<Call>>>,
        replies: Rc<RefCell<VecDeque<Result<String, String>>>>,
    }

    impl FakeRunner {
        fn reply(self, r: Result<&str, &str>) -> Self {
            self.replies
                .borrow_mut()
                .push_back(r.map(str::to_string).map_err(str::to_string));
            self
        }
    }

    impl RexRunner for FakeRunner {
        fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
            self.calls.borrow_mut().push((
                args.iter().map(|a| a.to_string()).collect(),
                stdin.map(str::to_string),
            ));
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }
    }

    fn host(runner: &FakeRunner, window: &str) -> RexHost {
        RexHost::new(
            Box::new(runner.clone()),
            S.to_string(),
            SourceRef {
                pane_id: B2.to_string(),
                tab_id: window.to_string(),
            },
        )
    }

    fn args(call: &Call) -> Vec<&str> {
        call.0.iter().map(String::as_str).collect()
    }

    #[test]
    fn tab_blocks_lists_tiled_terminals_of_the_window_source_first() {
        let got = tab_blocks(&fixture(), W1, B2);
        let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec![B2, B1]);
        assert!(!ids.contains(&POPUP), "floating popup must be left out");
    }

    #[test]
    fn tab_blocks_finds_the_window_from_the_source_block() {
        let ids: Vec<String> = tab_blocks(&fixture(), "", B3)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec![B3.to_string()]);
    }

    #[test]
    fn tab_blocks_falls_back_to_the_active_window() {
        let ids: Vec<String> = tab_blocks(&fixture(), "window:nope", "block:nope")
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec![B1.to_string(), B2.to_string()]);
    }

    #[test]
    fn tab_blocks_window_hint_wins() {
        let ids: Vec<String> = tab_blocks(&fixture(), W2, B1)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, vec![B3.to_string()]);
    }

    #[test]
    fn pane_title_rules() {
        assert_eq!(pane_title("editor", || unreachable!(), "b"), "editor");
        assert_eq!(pane_title("shell", || "~/src".into(), "b"), "~/src");
        assert_eq!(pane_title("shell", String::new, "b"), "shell");
        assert_eq!(pane_title("", String::new, "b"), "pane b");
    }

    #[test]
    fn last_lines_keeps_the_newest() {
        assert_eq!(last_lines("a\nb\nc\n", Some(2)), "b\nc\n");
        assert_eq!(last_lines("a\nb\nc", Some(2)), "b\nc");
        assert_eq!(last_lines("a\nb\n", Some(10)), "a\nb\n");
        assert_eq!(last_lines("a\nb\n", None), "a\nb\n");
        assert_eq!(last_lines("", Some(3)), "");
    }

    #[test]
    fn scrollback_read_captures_unwrapped_and_caps_lines() {
        let runner = FakeRunner::default().reply(Ok("1\n2\n3\n4\n"));
        let text = host(&runner, W1)
            .read_pane(B1, ReadSource::Scrollback, Some(2))
            .unwrap();
        assert_eq!(text, "3\n4\n");
        let calls = runner.calls.borrow();
        assert_eq!(
            args(&calls[0]),
            vec!["capture", "-s", S, "-b", B1, "--unwrap", "--trim"]
        );
    }

    #[test]
    fn viewport_read_keeps_the_bottom_rows() {
        let runner = FakeRunner::default()
            .reply(Ok("old\nr1\nr2\n"))
            .reply(Ok(r#"{"columns":80,"rows":2}"#));
        let text = host(&runner, W1)
            .read_pane(B1, ReadSource::Viewport, None)
            .unwrap();
        assert_eq!(text, "r1\nr2\n");
        let calls = runner.calls.borrow();
        assert_eq!(args(&calls[0]), vec!["capture", "-s", S, "-b", B1]);
        assert_eq!(
            args(&calls[1]),
            vec!["block", "call", "-s", S, "-b", B1, "size"]
        );
    }

    #[test]
    fn insert_sends_text_through_stdin() {
        let runner = FakeRunner::default();
        host(&runner, W1).insert_text(B2, "-rf /tmp/x").unwrap();
        let calls = runner.calls.borrow();
        assert_eq!(args(&calls[0]), vec!["send", "-s", S, "-b", B2]);
        assert_eq!(calls[0].1.as_deref(), Some("-rf /tmp/x"));
    }

    #[test]
    fn list_tab_panes_uses_terminal_titles_for_generic_labels() {
        let runner = FakeRunner::default()
            .reply(Ok(include_str!(
                "../../tests/fixtures/rex/session-inspect.json"
            )))
            .reply(Ok(r#"{"title":"~/src/zextract"}"#))
            .reply(Err("no title"));
        let panes = host(&runner, W1).list_tab_panes().unwrap();
        assert_eq!(
            panes,
            vec![
                PaneInfo {
                    pane_id: B2.into(),
                    title: "~/src/zextract".into()
                },
                PaneInfo {
                    pane_id: B1.into(),
                    title: "shell".into()
                },
            ]
        );
        let calls = runner.calls.borrow();
        assert_eq!(args(&calls[0]), vec!["session", "inspect", S, "--json"]);
    }

    #[test]
    fn cli_errors_are_passed_on() {
        let runner = FakeRunner::default().reply(Err("rex capture failed: block not found"));
        let err = host(&runner, W1)
            .read_pane(B1, ReadSource::Scrollback, None)
            .unwrap_err();
        assert_eq!(err, "rex capture failed: block not found");
    }
}
