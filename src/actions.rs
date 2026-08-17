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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    CopyRaw,
    CopyDisplay,
    Insert,
    InsertDisplay,
    Open,
    Edit,
    /// Open the file's containing location in Finder/a file manager.
    /// Not in any type's default allow-list below, matching the
    /// original exactly - `static_allowed_verbs` there doesn't include
    /// `Reveal` for any type either. It's real, dispatchable machinery,
    /// just opt-in only via `types.<tag>.actions` config (Phase 8).
    Reveal,
    /// Export the match as a JSON array to stdout.
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
            Verb::Reveal => "reveal",
            Verb::Json => "export",
        }
    }

    /// The List-mode key that fires this verb bare (no modifier).
    pub fn key_label(self) -> &'static str {
        match self {
            Verb::CopyRaw => "y",
            Verb::CopyDisplay => "Y",
            Verb::Insert => "i",
            Verb::InsertDisplay => "I",
            Verb::Open => "o",
            Verb::Edit => "e",
            Verb::Reveal => "r",
            Verb::Json => "J",
        }
    }

    /// Max matches for one multi-target dispatch, ported from the
    /// original's `limits{}` defaults. Hardcoded until Phase 8 makes
    /// this real config.
    pub fn cap(self) -> usize {
        match self {
            Verb::CopyRaw | Verb::CopyDisplay | Verb::Json => 100,
            Verb::Insert | Verb::InsertDisplay | Verb::Edit => 5,
            Verb::Open | Verb::Reveal => 10,
        }
    }
}

/// Map a List-mode keystroke to a verb. Plain letters fire the raw
/// variant, capitals (Shift) fire the *-display variant where one
/// exists. Ported from `action.rs::verb_from_char`.
pub fn verb_from_char(c: char) -> Option<Verb> {
    match c {
        'y' => Some(Verb::CopyRaw),
        'Y' => Some(Verb::CopyDisplay),
        'i' => Some(Verb::Insert),
        'I' => Some(Verb::InsertDisplay),
        'o' => Some(Verb::Open),
        'e' => Some(Verb::Edit),
        'r' => Some(Verb::Reveal),
        'J' => Some(Verb::Json),
        _ => None,
    }
}

/// Static type-keyed allow-list, ported from `action.rs::static_allowed_verbs`.
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
/// allowed; secrets hardcoded-deny `Open`/`Edit`/`Reveal`.
pub fn is_verb_allowed(m: &Match, verb: Verb) -> bool {
    if matches!(verb, Verb::CopyRaw | Verb::Json) {
        return true;
    }
    if matches!(m.ty, MatchType::Secret) && matches!(verb, Verb::Open | Verb::Edit | Verb::Reveal) {
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
pub fn dispatch(verb: Verb, m: &Match, source_pane_id: &str) -> Outcome {
    if !is_verb_allowed(m, verb) {
        return Outcome::Failed(format!(
            "'{}' not available for [{}]",
            verb.label(),
            m.effective_tag()
        ));
    }
    match verb {
        Verb::CopyRaw => copy_to_clipboard(&m.raw),
        Verb::CopyDisplay => copy_to_clipboard(&m.display),
        Verb::Insert => insert_text(&m.raw, source_pane_id),
        Verb::InsertDisplay => insert_text(&m.display, source_pane_id),
        Verb::Open => run_open(m),
        Verb::Edit => run_edit(m),
        Verb::Reveal => run_reveal(m),
        Verb::Json => Outcome::Done(matches_to_json(&[m])),
    }
}

/// Whether a multi-target dispatch of `verb` over `matches` should
/// proceed, without running any side effect. `Err` carries the exact
/// status message the picker should show while staying open (ported
/// from the original's "silent-permissive type-mismatch, loud-reject
/// if zero allowed, per-verb cap" rules in `dispatch_verb_on_targets`).
pub fn plan_batch(verb: Verb, matches: &[&Match]) -> Result<(), String> {
    let allowed_count = matches.iter().filter(|m| is_verb_allowed(m, verb)).count();
    if allowed_count == 0 {
        let sample = matches
            .first()
            .map(|m| m.effective_tag())
            .unwrap_or("selection");
        return Err(format!("'{}' not available for [{}]", verb.label(), sample));
    }
    let cap = verb.cap();
    if allowed_count > cap {
        return Err(format!(
            "refused: {allowed_count} matches exceeds cap of {cap} for '{}'",
            verb.label()
        ));
    }
    Ok(())
}

/// Execute `verb` over `matches`, assuming [`plan_batch`] already
/// approved it. Targets not individually allowed are silently skipped
/// (the "silent-permissive type-mismatch" rule: batch-copying a mix of
/// types just copies the ones that allow copy). A single allowed
/// target delegates to [`dispatch`] (preserves per-row semantics like
/// edit's `+line`); multiple targets join/batch per verb.
pub fn execute_batch(verb: Verb, matches: &[&Match], source_pane_id: &str) -> Outcome {
    let allowed: Vec<&Match> = matches
        .iter()
        .copied()
        .filter(|m| is_verb_allowed(m, verb))
        .collect();
    if allowed.is_empty() {
        return Outcome::Failed(format!("'{}' not available for selection", verb.label()));
    }
    if allowed.len() == 1 {
        return dispatch(verb, allowed[0], source_pane_id);
    }
    match verb {
        Verb::CopyRaw => copy_to_clipboard(&join(&allowed, |m| &m.raw, "\n")),
        Verb::CopyDisplay => copy_to_clipboard(&join(&allowed, |m| &m.display, "\n")),
        Verb::Insert => insert_text(&join(&allowed, |m| &m.raw, " "), source_pane_id),
        Verb::InsertDisplay => insert_text(&join(&allowed, |m| &m.display, " "), source_pane_id),
        Verb::Open => {
            let ok = allowed
                .iter()
                .filter(|m| matches!(run_open(m), Outcome::Done(_)))
                .count();
            Outcome::Done(format!("opened {ok}/{} targets", allowed.len()))
        }
        Verb::Reveal => {
            let ok = allowed
                .iter()
                .filter(|m| matches!(run_reveal(m), Outcome::Done(_)))
                .count();
            Outcome::Done(format!("revealed {ok}/{} targets", allowed.len()))
        }
        Verb::Edit => run_edit_batch(&allowed),
        Verb::Json => Outcome::Done(matches_to_json(&allowed)),
    }
}

fn join<'a>(matches: &[&'a Match], f: impl Fn(&'a Match) -> &'a String, sep: &str) -> String {
    matches
        .iter()
        .map(|&m| f(m).as_str())
        .collect::<Vec<_>>()
        .join(sep)
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
    match crate::socket_client::request(
        &socket_path,
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

/// Reveal the file's containing folder. macOS: `open -R` selects the
/// file itself in Finder. Linux has no universal "select in file
/// manager" API across desktop environments, so this falls back to
/// opening the parent directory with `xdg-open` - close enough, not
/// exact parity with Finder's select-in-place behavior.
fn run_reveal(m: &Match) -> Outcome {
    let file = m.fields.get("file").map(String::as_str).unwrap_or(&m.raw);
    if cfg!(target_os = "macos") {
        return match Command::new("open").arg("-R").arg(file).status() {
            Ok(status) if status.success() => Outcome::Done(format!("revealed {file}")),
            Ok(status) => Outcome::Failed(format!("open -R exited with {status}")),
            Err(e) => Outcome::Failed(format!("open -R failed: {e}")),
        };
    }
    let dir = std::path::Path::new(file)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());
    match Command::new("xdg-open").arg(&dir).status() {
        Ok(status) if status.success() => Outcome::Done(format!("opened {dir}")),
        Ok(status) => Outcome::Failed(format!("xdg-open exited with {status}")),
        Err(e) => Outcome::Failed(format!("xdg-open failed: {e}")),
    }
}

/// Multi-target edit: chains one `$EDITOR` invocation per file with
/// `&&` into a single shell command, spawned via `sh -c` inheriting
/// this process's stdio (same direct-spawn approach as [`run_edit`]).
fn run_edit_batch(matches: &[&Match]) -> Outcome {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let cmd = matches
        .iter()
        .map(|m| {
            let file = m.fields.get("file").map(String::as_str).unwrap_or(&m.raw);
            match m.fields.get("line").filter(|l| !l.is_empty()) {
                Some(line) => format!("{editor} +{line} {}", shell_quote(file)),
                None => format!("{editor} {}", shell_quote(file)),
            }
        })
        .collect::<Vec<_>>()
        .join(" && ");
    match Command::new("sh").arg("-c").arg(&cmd).status() {
        Ok(status) if status.success() => Outcome::Done(format!("edited {} files", matches.len())),
        Ok(status) => Outcome::Failed(format!("edit chain exited with {status}")),
        Err(e) => Outcome::Failed(format!("failed to launch {editor}: {e}")),
    }
}

/// POSIX-safe shell quoting: wraps in single quotes (with embedded `'`
/// escaped as `'\''`) unless every char is already shell-safe as-is.
fn shell_quote(s: &str) -> String {
    let is_safe = !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~' | ':' | '+')
        });
    if is_safe {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

fn matches_to_json(matches: &[&Match]) -> String {
    let objects: Vec<serde_json::Value> = matches
        .iter()
        .map(|m| {
            let mut fields = m.fields.clone();
            fields.remove("__label"); // internal bookkeeping, not a real match field
            fields.insert("type".to_string(), m.effective_tag().to_string());
            fields.insert("raw".to_string(), m.raw.clone());
            fields.insert("display".to_string(), m.display.clone());
            fields.insert("context".to_string(), m.context.clone());
            serde_json::json!(fields)
        })
        .collect();
    serde_json::to_string(&objects).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn m(ty: MatchType, raw: &str) -> Match {
        Match {
            ty,
            raw: raw.to_string(),
            display: raw.to_string(),
            context: raw.to_string(),
            span: (0, raw.len()),
            fields: HashMap::new(),
        }
    }

    #[test]
    fn verb_from_char_maps_raw_and_display_variants() {
        assert_eq!(verb_from_char('y'), Some(Verb::CopyRaw));
        assert_eq!(verb_from_char('Y'), Some(Verb::CopyDisplay));
        assert_eq!(verb_from_char('i'), Some(Verb::Insert));
        assert_eq!(verb_from_char('I'), Some(Verb::InsertDisplay));
        assert_eq!(verb_from_char('z'), None);
    }

    #[test]
    fn plan_batch_rejects_when_none_allowed() {
        let secret = m(MatchType::Secret, "AKIAEXAMPLE");
        let refs = vec![&secret];
        assert!(plan_batch(Verb::Open, &refs).is_err());
    }

    #[test]
    fn plan_batch_rejects_over_cap() {
        let matches: Vec<Match> = (0..6)
            .map(|i| m(MatchType::File, &format!("f{i}")))
            .collect();
        let refs: Vec<&Match> = matches.iter().collect();
        // Edit's cap is 5; 6 file matches should be refused.
        let err = plan_batch(Verb::Edit, &refs).unwrap_err();
        assert!(
            err.contains("cap"),
            "expected a cap-refusal message, got: {err}"
        );
    }

    #[test]
    fn plan_batch_allows_within_cap() {
        let matches: Vec<Match> = (0..3)
            .map(|i| m(MatchType::File, &format!("f{i}")))
            .collect();
        let refs: Vec<&Match> = matches.iter().collect();
        assert!(plan_batch(Verb::Edit, &refs).is_ok());
    }

    #[test]
    fn matches_to_json_produces_array_with_all_targets() {
        let a = m(MatchType::Url, "https://example.com");
        let b = m(MatchType::Sha, "abc1234");
        let json = matches_to_json(&[&a, &b]);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        assert_eq!(parsed[0]["type"], "url");
        assert_eq!(parsed[1]["type"], "sha");
    }

    #[test]
    fn shell_quote_leaves_safe_paths_bare() {
        assert_eq!(shell_quote("/tmp/foo.rs"), "/tmp/foo.rs");
    }

    #[test]
    fn shell_quote_wraps_and_escapes_unsafe_paths() {
        assert_eq!(
            shell_quote("/tmp/it's a dir/f.rs"),
            "'/tmp/it'\\''s a dir/f.rs'"
        );
    }
}
