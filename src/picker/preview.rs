//! ±3 lines of context around a match's location in the pane text it
//! came from, for the `p`/`Ctrl-P` preview split. Pure - takes a
//! [`Match`] and the picker's `pane_texts` map, doesn't touch
//! rendering.

use std::collections::HashMap;

use crate::matcher::{Match, MatchType};

/// A slice of a pane's captured text, centered (as closely as the pane
/// boundaries allow) on `m`'s source line.
pub struct PreviewContext<'a> {
    /// Lines `lines[0]..=lines[last]`, in source order.
    pub lines: Vec<&'a str>,
    /// Index into `lines` of `m`'s own line.
    pub current: usize,
    /// Char-index range `(start, end)` within `lines[current]` covering
    /// `m`'s own extracted text - lets the preview pick out the exact
    /// finding, not just the line it's on. `(0, 0)` (empty) if it
    /// couldn't be resolved, e.g. a span the matcher recorded past the
    /// line's own length.
    pub span_chars: (usize, usize),
    /// Block matches (`ty == Block`) set this: every line in `lines`
    /// *is* the match, so the renderer highlights them all instead of
    /// singling out `current`.
    pub highlight_all: bool,
    /// Muted footnote rendered after `lines` - e.g. "+12 more lines"
    /// for a block preview truncated at [`BLOCK_PREVIEW_LINES`].
    pub note: Option<String>,
}

/// Up to 3 lines before and after the line `m` was found on, from the
/// pane text keyed by `m`'s `__pane_id` field in `pane_texts`. `None`
/// when that field or pane text is missing - e.g. a stale match from
/// just before a regrab replaced `pane_texts` out from under it, which
/// shouldn't happen in practice since a regrab replaces `matches` in
/// the same step.
pub fn context_lines<'a>(
    m: &Match,
    pane_texts: &'a HashMap<String, String>,
) -> Option<PreviewContext<'a>> {
    let pane_id = m.fields.get("__pane_id")?;
    let text = pane_texts.get(pane_id)?;
    // A block match's span covers its content lines directly - show
    // the whole block (capped) instead of ±3 lines around its first.
    if m.ty == MatchType::Block {
        return block_lines(m, text);
    }
    // `span.0`/`span.1` are byte offsets into `text` from the same
    // extraction pass (every built-in pattern module tracks them via
    // `text.lines()` + "+1 for the newline", the same coordinate space
    // used here), so they're always valid UTF-8 boundaries - counting
    // newlines strictly before `span.0` gives the 0-based line number,
    // and the last newline before it gives that line's own start offset.
    let line_start = text[..m.span.0].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_no = text[..m.span.0].matches('\n').count();
    let all: Vec<&str> = text.lines().collect();
    if all.is_empty() {
        return None;
    }
    let line_no = line_no.min(all.len() - 1);
    let start = line_no.saturating_sub(3);
    let end = (line_no + 3).min(all.len() - 1);
    let current_line = all[line_no];
    let span_start_byte = m.span.0.saturating_sub(line_start).min(current_line.len());
    let span_end_byte = m
        .span
        .1
        .saturating_sub(line_start)
        .max(span_start_byte)
        .min(current_line.len());
    let span_chars = (
        current_line[..span_start_byte].chars().count(),
        current_line[..span_end_byte].chars().count(),
    );
    Some(PreviewContext {
        lines: all[start..=end].to_vec(),
        current: line_no - start,
        span_chars,
        highlight_all: false,
        note: None,
    })
}

/// Block-match preview: the span's own line range, capped at
/// [`BLOCK_PREVIEW_LINES`] with a "+N more lines" note. Every shown
/// line is part of the match, so `highlight_all` is set and
/// `span_chars` is unused (`(0, 0)`).
fn block_lines<'a>(m: &Match, text: &'a str) -> Option<PreviewContext<'a>> {
    let all: Vec<&str> = text.lines().collect();
    if all.is_empty() {
        return None;
    }
    let first = text[..m.span.0].matches('\n').count().min(all.len() - 1);
    let last = text[..m.span.1].matches('\n').count().min(all.len() - 1);
    let last = last.max(first);
    let cap = crate::matcher::block::BLOCK_PREVIEW_LINES;
    let shown_end = last.min(first + cap - 1);
    let note = ((last - first + 1) > cap).then(|| format!("+{} more lines", last - shown_end));
    Some(PreviewContext {
        lines: all[first..=shown_end].to_vec(),
        current: 0,
        span_chars: (0, 0),
        highlight_all: true,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::MatchType;

    fn make_match(pane_id: &str, span_start: usize) -> Match {
        make_match_spanning(pane_id, span_start, span_start + 1)
    }

    fn make_match_spanning(pane_id: &str, span_start: usize, span_end: usize) -> Match {
        Match {
            ty: MatchType::Url,
            raw: "x".to_string(),
            display: "x".to_string(),
            context: "x".to_string(),
            span: (span_start, span_end),
            fields: HashMap::from([("__pane_id".to_string(), pane_id.to_string())]),
        }
    }

    #[test]
    fn centers_on_the_matchs_line() {
        let text = "0\n1\n2\n3\n4\n5\n6\n7\n8\n9".to_string();
        let pane_texts = HashMap::from([("p1".to_string(), text)]);
        // Line "5" starts at byte offset 10 ("0\n1\n2\n3\n4\n" = 10 bytes).
        let m = make_match("p1", 10);
        let ctx = context_lines(&m, &pane_texts).unwrap();
        assert_eq!(ctx.lines, vec!["2", "3", "4", "5", "6", "7", "8"]);
        assert_eq!(ctx.current, 3);
        assert_eq!(ctx.lines[ctx.current], "5");
        assert_eq!(ctx.span_chars, (0, 1));
    }

    #[test]
    fn span_chars_covers_just_the_extracted_substring() {
        // "see " (4 bytes) + "https://example.com" (20 bytes) + " for details"
        let line = "see https://example.com for details";
        let text = format!("intro\n{line}\noutro");
        let pane_texts = HashMap::from([("p1".to_string(), text)]);
        let url_start = 6 + 4; // "intro\n" (6) + "see " (4)
        let url_end = url_start + "https://example.com".len();
        let m = make_match_spanning("p1", url_start, url_end);
        let ctx = context_lines(&m, &pane_texts).unwrap();
        assert_eq!(ctx.lines[ctx.current], line);
        let (start, end) = ctx.span_chars;
        let chars: Vec<char> = line.chars().collect();
        let extracted: String = chars[start..end].iter().collect();
        assert_eq!(extracted, "https://example.com");
    }

    #[test]
    fn span_chars_accounts_for_multibyte_chars_before_the_match() {
        // "héllo " has a 2-byte 'é' - byte offset != char offset once
        // the match starts after it.
        let line = "héllo world";
        let text = format!("intro\n{line}");
        let pane_texts = HashMap::from([("p1".to_string(), text)]);
        let world_start = 6 + line.find("world").unwrap();
        let m = make_match_spanning("p1", world_start, world_start + "world".len());
        let ctx = context_lines(&m, &pane_texts).unwrap();
        let chars: Vec<char> = line.chars().collect();
        let (start, end) = ctx.span_chars;
        let extracted: String = chars[start..end].iter().collect();
        assert_eq!(extracted, "world");
    }

    #[test]
    fn clamps_at_the_start_of_the_text() {
        // 5 lines, match on line 0 - the window can't extend below 0,
        // so it's shorter than a full ±3 span rather than padded out
        // with lines that don't exist.
        let text = "0\n1\n2\n3\n4".to_string();
        let pane_texts = HashMap::from([("p1".to_string(), text)]);
        let m = make_match("p1", 0); // line 0
        let ctx = context_lines(&m, &pane_texts).unwrap();
        assert_eq!(ctx.lines, vec!["0", "1", "2", "3"]);
        assert_eq!(ctx.current, 0);
    }

    #[test]
    fn clamps_at_the_end_of_the_text() {
        let text = "0\n1\n2\n3\n4".to_string();
        let pane_texts = HashMap::from([("p1".to_string(), text)]);
        let m = make_match("p1", 8); // line 4, the last line
        let ctx = context_lines(&m, &pane_texts).unwrap();
        assert_eq!(ctx.lines, vec!["1", "2", "3", "4"]);
        assert_eq!(ctx.current, 3);
    }

    #[test]
    fn missing_pane_id_field_returns_none() {
        let pane_texts = HashMap::from([("p1".to_string(), "text".to_string())]);
        let mut m = make_match("p1", 0);
        m.fields.remove("__pane_id");
        assert!(context_lines(&m, &pane_texts).is_none());
    }

    #[test]
    fn unknown_pane_id_returns_none() {
        let pane_texts = HashMap::from([("p1".to_string(), "text".to_string())]);
        let m = make_match("gone", 0);
        assert!(context_lines(&m, &pane_texts).is_none());
    }

    fn make_block_match(span: (usize, usize)) -> Match {
        Match {
            ty: MatchType::Block,
            raw: "x".to_string(),
            display: "x".to_string(),
            context: "x".to_string(),
            span,
            fields: HashMap::from([("__pane_id".to_string(), "p1".to_string())]),
        }
    }

    #[test]
    fn block_match_previews_whole_block() {
        let text = "before\n```sh\nalpha\nbeta\n```".to_string();
        let pane_texts = HashMap::from([("p1".to_string(), text)]);
        // Content lines are "alpha\nbeta": from after "```sh\n" to
        // before the trailing "\n```".
        let start = "before\n```sh\n".len();
        let end = "before\n```sh\nalpha\nbeta".len();
        let m = make_block_match((start, end));
        let ctx = context_lines(&m, &pane_texts).unwrap();
        assert!(ctx.highlight_all);
        assert_eq!(ctx.current, 0);
        assert_eq!(ctx.lines, vec!["alpha", "beta"]);
        assert_eq!(ctx.span_chars, (0, 0));
        assert!(ctx.note.is_none());
    }

    #[test]
    fn long_block_preview_is_capped_with_note() {
        let mut text = String::from("before\n```sh\n");
        for i in 0..30 {
            text.push_str(&format!("line-{i:02}\n"));
        }
        text.push_str("```");
        let start = "before\n```sh\n".len();
        let end = text.len() - "\n```".len();
        let pane_texts = HashMap::from([("p1".to_string(), text)]);
        let m = make_block_match((start, end));
        let ctx = context_lines(&m, &pane_texts).unwrap();
        let cap = crate::matcher::block::BLOCK_PREVIEW_LINES;
        assert!(ctx.highlight_all);
        assert_eq!(ctx.lines.len(), cap);
        assert_eq!(ctx.lines[0], "line-00");
        assert_eq!(ctx.lines[cap - 1], "line-23");
        assert_eq!(ctx.note.as_deref(), Some("+6 more lines"));
    }
}
