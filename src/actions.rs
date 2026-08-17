//! Action verbs, per-type allow-lists, defaults, and dispatch — ported
//! from the original zellij-zextract plugin's `action.rs`, adapted for
//! Herdr's native-process model.
//!
//! One deliberate departure from the original: its "edit" verb couldn't
//! spawn an interactive `$EDITOR` (a WASM plugin has no real TTY to hand
//! it), so it *typed the editor command into the source pane* for the
//! user to review and run themselves. Herdr plugins are native
//! processes with a real PTY (this popup), so `edit` here spawns
//! `$EDITOR` directly, inheriting the popup's terminal — a genuine
//! capability upgrade the port can take advantage of.
//!
//! Deferred to Phase 5: user config for action templates/overrides,
//! custom patterns. The allow-lists and defaults below are the static
//! built-in tables, matching the *actual* original code (not
//! `doc/types.md`, which is stale in one spot: the doc claims `file`
//! defaults to `edit`, but `action.rs::static_default_verb` puts it in
//! the `Insert` bucket alongside everything except `Url` and
//! `Diagnostic` — verified against the source, not the prose).

use std::process::Command;

use crate::matcher::{Match, MatchType};
use crate::socket_client::SocketClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    CopyRaw,
    CopyDisplay,
    Insert,
    InsertDisplay,
    Open,
    Edit,
    /// Export the match as a JSON array (of one object) to stdout.
    /// Universal — always allowed for every type.
    Json,
}

impl Verb {
    pub fn label(self) -> &'static str {
        match self {
            Verb::CopyRaw => "copy",
            Verb::CopyDisplay => "copy-display",
            Verb::Insert => "insert",
            Verb::InsertDisplay => "insert-display",
            Verb::Open => "open",
            Verb::Edit => "edit",
            Verb::Json => "export",
        }
    }
}

/// Static type-keyed allow-list, ported from `action.rs::static_allowed_verbs`
/// (its `Reveal` entries dropped — not in this phase's scope).
fn static_allowed_verbs(ty: MatchType) -> &'static [Verb] {
    use MatchType::*;
    use Verb::*;
    match ty {
        Url => &[Open, CopyRaw, Insert],
        File | Diagnostic => &[Edit, CopyRaw, Insert],
        Git => &[Insert, CopyRaw],
        Sha => &[CopyRaw, Insert],
        Ipv4 | Ipv6 => &[CopyRaw, Insert],
        Uuid => &[CopyRaw, Insert],
        QuotedString => &[CopyRaw, CopyDisplay, Insert, InsertDisplay],
        Command => &[Insert, CopyRaw],
        // Secret: never open/edit - hardcoded deny in is_verb_allowed too.
        Secret => &[CopyRaw, Insert],
    }
}

pub fn allowed_verbs(ty: MatchType) -> &'static [Verb] {
    static_allowed_verbs(ty)
}

/// Default verb fired by Enter, ported from
/// `action.rs::static_default_verb` verbatim.
pub fn default_verb(ty: MatchType) -> Verb {
    use MatchType::*;
    use Verb::*;
    match ty {
        Url => Open,
        Diagnostic => Edit,
        File | Command | Git | Sha | Ipv4 | Ipv6 | Uuid | QuotedString | Secret => Insert,
    }
}

/// True if `verb` may fire for `m`. `CopyRaw`/`Json` are universally
/// allowed; secrets hardcoded-deny `Open`/`Edit`.
pub fn is_verb_allowed(m: &Match, verb: Verb) -> bool {
    if matches!(verb, Verb::CopyRaw | Verb::Json) {
        return true;
    }
    if matches!(m.ty, MatchType::Secret) && matches!(verb, Verb::Open | Verb::Edit) {
        return false;
    }
    allowed_verbs(m.ty).contains(&verb)
}

pub enum Outcome {
    Done(String),
    Failed(String),
}

/// Run `verb` against `m`. `source_pane_id` is required for
/// insert/insert-display, sent back to that pane over `pane.send_text`.
///
/// Insert opens a **fresh** socket connection rather than reusing one
/// held from earlier in the process's life: the picker session between
/// launch and a user's keypress can run long enough (real typing/
/// thinking time) that a connection opened before it goes stale on the
/// server side, surfacing as "Broken pipe" — found by manual testing.
pub fn dispatch(verb: Verb, m: &Match, source_pane_id: &str) -> Outcome {
    if !is_verb_allowed(m, verb) {
        return Outcome::Failed(format!(
            "'{}' not available for [{}]",
            verb.label(),
            m.ty.tag()
        ));
    }
    match verb {
        Verb::CopyRaw => copy_to_clipboard(&m.raw),
        Verb::CopyDisplay => copy_to_clipboard(&m.display),
        Verb::Insert => insert_text(&m.raw, source_pane_id),
        Verb::InsertDisplay => insert_text(&m.display, source_pane_id),
        Verb::Open => run_open(m),
        Verb::Edit => run_edit(m),
        Verb::Json => Outcome::Done(match_to_json(m)),
    }
}

fn copy_to_clipboard(text: &str) -> Outcome {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        Ok(()) => Outcome::Done(format!("copied: {text}")),
        Err(e) => Outcome::Failed(format!("copy failed: {e}")),
    }
}

fn insert_text(text: &str, source_pane_id: &str) -> Outcome {
    let socket_path = match std::env::var("HERDR_SOCKET_PATH") {
        Ok(p) => p,
        Err(_) => return Outcome::Failed("HERDR_SOCKET_PATH is not set".to_string()),
    };
    let mut socket = match SocketClient::connect(&socket_path) {
        Ok(s) => s,
        Err(e) => return Outcome::Failed(format!("failed to connect to {socket_path}: {e}")),
    };
    match socket.request(
        "pane.send_text",
        serde_json::json!({"pane_id": source_pane_id, "text": text}),
    ) {
        Ok(_) => Outcome::Done(format!("inserted into {source_pane_id}: {text}")),
        Err(e) => Outcome::Failed(format!("insert failed: {e}")),
    }
}

fn run_open(m: &Match) -> Outcome {
    let target = m.fields.get("url").map(String::as_str).unwrap_or(&m.raw);
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match Command::new(opener).arg(target).status() {
        Ok(status) if status.success() => Outcome::Done(format!("opened {target}")),
        Ok(status) => Outcome::Failed(format!("{opener} exited with {status}")),
        Err(e) => Outcome::Failed(format!("{opener} failed: {e}")),
    }
}

/// Spawns `$EDITOR` (falling back to `vi`) directly, inheriting this
/// process's stdio — see the module doc for why this differs from the
/// original's "type the command into the source pane" approach.
fn run_edit(m: &Match) -> Outcome {
    let file = m.fields.get("file").map(String::as_str).unwrap_or(&m.raw);
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

    let mut cmd = Command::new(&editor);
    if let Some(line) = m.fields.get("line").filter(|l| !l.is_empty()) {
        cmd.arg(format!("+{line}"));
    }
    cmd.arg(file);

    match cmd.status() {
        Ok(status) if status.success() => Outcome::Done(format!("edited {file}")),
        Ok(status) => Outcome::Failed(format!("{editor} exited with {status}")),
        Err(e) => Outcome::Failed(format!("failed to launch {editor}: {e}")),
    }
}

fn match_to_json(m: &Match) -> String {
    let mut fields = m.fields.clone();
    fields.insert("type".to_string(), m.ty.tag().to_string());
    fields.insert("raw".to_string(), m.raw.clone());
    fields.insert("display".to_string(), m.display.clone());
    fields.insert("context".to_string(), m.context.clone());
    let object = serde_json::json!(fields);
    serde_json::to_string(&[object]).unwrap_or_else(|_| "[]".to_string())
}
