//! ±3 lines of context around a match's location in the pane text it
//! came from, for the `p`/`Ctrl-P` preview split. Pure - takes a
//! [`Match`] and the picker's `pane_texts` map, doesn't touch
//! rendering.

use std::collections::HashMap;

use crate::matcher::Match;

/// A slice of a pane's captured text, centered (as closely as the pane
/// boundaries allow) on `m`'s source line.
pub struct PreviewContext<'a> {
    /// Lines `lines[0]..=lines[last]`, in source order.
    pub lines: Vec<&'a str>,
    /// Index into `lines` of `m`'s own line.
    pub current: usize,
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
    // `span.0` is a byte offset into `text` from the same extraction
    // pass, so it's always a valid UTF-8 boundary - counting newlines
    // strictly before it gives the 0-based source line number.
    let line_no = text[..m.span.0].matches('\n').count();
    let all: Vec<&str> = text.lines().collect();
    if all.is_empty() {
        return None;
    }
    let line_no = line_no.min(all.len() - 1);
    let start = line_no.saturating_sub(3);
    let end = (line_no + 3).min(all.len() - 1);
    Some(PreviewContext {
        lines: all[start..=end].to_vec(),
        current: line_no - start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::MatchType;

    fn make_match(pane_id: &str, span_start: usize) -> Match {
        Match {
            ty: MatchType::Url,
            raw: "x".to_string(),
            display: "x".to_string(),
            context: "x".to_string(),
            span: (span_start, span_start + 1),
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
}
