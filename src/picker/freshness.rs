//! `freshness` — a match's position in its pane's captured buffer.
//!
//! Scrollback has no timestamps, so the only truthful freshness signal
//! is *where* in the buffer a match sits: how many lines above the
//! bottom (the newest line the pane had when captured) it was found.
//! That's exactly what users need to tell near-identical matches apart
//! — the same command or URL printed repeatedly with only a token
//! differing, where the freshest occurrence is almost always the one
//! wanted.
//!
//! This module is pure: [`PanePositions`] precomputes a pane's newline
//! offsets once (rebuilt whenever `pane_texts` is replaced, i.e. on
//! regrab/cycle), and everything else is arithmetic on that table.
//! It powers:
//! - the filtered list's freshness tie-break (equal fuzzy scores order
//!   freshest-first; with an empty query every score is 0, so the list
//!   is fully freshness-ordered),
//! - the dim `↑N` column on each list row,
//! - the preview border title `preview · L1204/1309 · 105↑`.

/// A pane's precomputed line table. Built once per capture, not per
/// keystroke — `refilter` sorts against it via binary search.
#[derive(Debug, Clone)]
pub struct PanePositions {
    /// Byte offset of every `\n` in the captured text, ascending.
    newline_offsets: Vec<usize>,
    /// Total line count — `text.lines().len()` semantics.
    total_lines: usize,
}

impl PanePositions {
    pub fn build(text: &str) -> Self {
        Self {
            newline_offsets: text.match_indices('\n').map(|(i, _)| i).collect(),
            total_lines: text.lines().count(),
        }
    }

    /// 0-based line index containing byte `offset` — the number of
    /// newlines strictly before it. Offsets past the text's end clamp
    /// to the last line.
    pub fn line_of(&self, offset: usize) -> usize {
        self.newline_offsets.partition_point(|&o| o < offset)
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }
}

/// Lines from the bottom of the pane's buffer: `0` = the match is on
/// the last (newest) line, `105` = 105 lines above it. Higher = older.
pub fn distance_from_bottom(p: &PanePositions, span_start: usize) -> usize {
    p.total_lines()
        .saturating_sub(1)
        .saturating_sub(p.line_of(span_start))
}

/// Sort key for a match whose pane table is unavailable (stale
/// `__pane_id` after a regrab): sorts after every known position.
pub const UNKNOWN: usize = usize::MAX;

/// The dim list-row label: `↑105` (`0` = newest, on the buffer's last
/// line). `↑?` when the position is unknown — keeps the column visible
/// instead of silently shifting row widths.
pub fn row_label(distance: Option<usize>) -> String {
    match distance {
        Some(d) => format!("↑{d}"),
        None => "↑?".to_string(),
    }
}

/// Preview border title: plain `preview` when the position is unknown
/// or display is off, else `preview · L1204/1309 · 105↑` (1-based line
/// number, total lines, then lines-from-the-bottom).
pub fn preview_title(position: Option<(usize, usize)>) -> String {
    match position {
        Some((line_no, total)) if total > 0 => {
            let distance = total - 1 - line_no;
            format!("preview · L{}/{total} · {distance}↑", line_no + 1)
        }
        _ => "preview".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(text: &str) -> PanePositions {
        PanePositions::build(text)
    }

    #[test]
    fn line_of_counts_newlines_before_offset() {
        // "0\n1\n2" — lines "0","1","2" at byte starts 0, 2, 4.
        let p = build("0\n1\n2");
        assert_eq!(p.total_lines(), 3);
        assert_eq!(p.line_of(0), 0); // line "0"
        assert_eq!(p.line_of(1), 0); // still on line "0"
        assert_eq!(p.line_of(2), 1); // line "1"
        assert_eq!(p.line_of(5), 2); // line "2"
    }

    #[test]
    fn line_of_offsets_past_the_end_clamp_to_the_last_line() {
        let p = build("a\nb");
        assert_eq!(p.line_of(999), 1);
    }

    #[test]
    fn line_of_is_byte_not_char_offset() {
        // Multibyte content must not skew the offset math.
        let p = build("héllo\nwörld");
        // "héllo\n" is 7 bytes (2-byte é), so "w" starts at 7.
        assert_eq!(p.line_of(7), 1);
        assert_eq!(p.line_of(3), 0);
    }

    #[test]
    fn distance_zero_is_the_last_line() {
        let p = build("old\nmid\nnew");
        assert_eq!(distance_from_bottom(&p, "old\nmid\n".len()), 0);
        assert_eq!(distance_from_bottom(&p, "old\n".len()), 1);
        assert_eq!(distance_from_bottom(&p, 0), 2);
    }

    #[test]
    fn trailing_newline_counts_like_lines_iterator() {
        // "a\n" is one line for `lines()`, and matches the preview's
        // `text.lines()` slicing — no phantom empty last line.
        let p = build("a\n");
        assert_eq!(p.total_lines(), 1);
        assert_eq!(p.line_of(0), 0);
        assert_eq!(distance_from_bottom(&p, 0), 0);
    }

    #[test]
    fn empty_text_has_no_lines() {
        let p = build("");
        assert_eq!(p.total_lines(), 0);
        assert_eq!(p.line_of(0), 0);
    }

    #[test]
    fn row_labels() {
        assert_eq!(row_label(Some(0)), "↑0");
        assert_eq!(row_label(Some(105)), "↑105");
        assert_eq!(row_label(None), "↑?");
    }

    #[test]
    fn preview_titles() {
        // 1309 lines, match on 0-based line 1203.
        assert_eq!(
            preview_title(Some((1203, 1309))),
            "preview · L1204/1309 · 105↑"
        );
        assert_eq!(
            preview_title(Some((1308, 1309))),
            "preview · L1309/1309 · 0↑"
        );
        assert_eq!(preview_title(Some((0, 1))), "preview · L1/1 · 0↑");
        assert_eq!(preview_title(None), "preview");
        assert_eq!(preview_title(Some((0, 0))), "preview");
    }
}
