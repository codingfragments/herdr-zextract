//! `block` — whole code-block extraction.
//!
//! Modern codegen CLIs and READMEs *show* snippets: fenced ` ``` ` /
//! `~~~` blocks in catted markdown, box-framed sample blocks (pi/AI
//! CLI rounded frames, TUI tool output), and line-numbered listings
//! (`bat`, `cat -n`, `rg`). Selecting those by hand is tedious and
//! copying them verbatim drags the decoration along. This matcher
//! scans the whole capture once and emits each detected block as a
//! single [`Match`] whose `raw` is the **stripped content** — fence
//! rows drop, rule rows drop, side bars and number gutters drop —
//! so the existing copy/insert verbs hand over a clean snippet.
//!
//! Detection logic for boxes and gutters is ported from the sister
//! `herdr-flash` plugin's `src/decor.rs` (its per-selection decoration
//! analysis, feature/decoration-aware-copy), adapted from "what did
//! the user select" to "what is in the buffer":
//! - **Boxes** — a top rule row (`╭─╮` glyph families) followed by
//!   content lines that all open with the same Unicode bar, closed
//!   by a bottom rule row. A frame clipped by the capture edge (no
//!   bottom rule) still fires when it has ≥2 non-empty content lines.
//!   ASCII frames (`+ - |`) are excluded — markdown tables and ASCII
//!   art are content far more often than frames.
//! - **Side-bars-only runs** — consecutive lines opening with the same
//!   Unicode bar (chat TUIs, spinner frames), ≥2 non-empty lines,
//!   same bar column.
//! - **Gutters** — the `decor` pattern: optional identical file
//!   prefix (`rg` multi-file), spaces, a number, a separator, with the
//!   counter **strictly increasing** across the run (the monotonic
//!   requirement is what keeps `12:`-starting code lines from being
//!   misread as a gutter).
//! - **Fences** — ` ``` `/`~~~` open/close pairs (≥3 chars, same
//!   glyph, close at same-or-less indent with no info string), ≥1
//!   non-empty content line.
//!
//! Candidate arbitration: fenced blocks are the most explicit marker,
//! so a box/gutter candidate overlapping a fence is dropped; gutters
//! contained in a box drop (the box strip already removes the inner
//! numbers); otherwise the outermost candidate of each kind wins and
//! any candidate contained in a same-kind candidate drops. Blocks
//! longer than [`MAX_BLOCK_LINES`] lines are skipped — a 300-line
//! `bat` dump is not a snippet, and the picker's other matchers cover
//! its lines individually anyway.
//!
//! A block's `span` covers its *content* lines (fence/rule rows
//! excluded), which is what the preview pane shows; `context` carries
//! the verbatim opening line (the fence row or top rule row) so the
//! list row hints at what the block looked like in the pane.

use std::collections::HashMap;

use super::{Match, MatchType};

/// Content lines beyond this make a candidate too large to be a
/// useful snippet — skip it entirely rather than flood the list.
const MAX_BLOCK_LINES: usize = 200;

/// Preview cap for block matches — matches the picker's ±3-line
/// context spirit while still showing enough of a long block to
/// recognize it.
pub(crate) const BLOCK_PREVIEW_LINES: usize = 24;

// ── Box glyph sets — ported from herdr-flash src/decor.rs ─────────────────────

/// Top-row left corners.
const TOP_LEFT: [char; 5] = ['╭', '┌', '┏', '╔', '╒'];
/// Top-row right corners.
const TOP_RIGHT: [char; 5] = ['╮', '┐', '┓', '╗', '╕'];
/// Bottom-row left corners.
const BOTTOM_LEFT: [char; 5] = ['╰', '└', '┗', '╚', '╘'];
/// Bottom-row right corners.
const BOTTOM_RIGHT: [char; 5] = ['╯', '┘', '┛', '╝', '╛'];
/// Vertical bar glyphs (rounded/light `│`, heavy `┃`, double `║`, dashed `╎`).
const BARS: [char; 4] = ['│', '┃', '║', '╎'];
/// Horizontal rule glyphs (light `─`, heavy `━`, double `═`, dashed `╌`).
const H_RULES: [char; 4] = ['─', '━', '═', '╌'];
/// Filler glyphs used inside rule rows (pi pads frames with `·`).
const FILLERS: [char; 1] = ['·'];

// ── Internal candidate model ───────────────────────────────────────────────────

/// A detected block before it becomes a [`Match`].
struct Candidate {
    /// Machine kind, mirrored into `fields["kind"]`.
    kind: &'static str,
    /// Fence info string (` ```sh ` → `sh`), fenced blocks only.
    lang: Option<String>,
    /// Byte span of the block's *content* lines (fence/rule rows
    /// excluded) in the scanned text.
    span: (usize, usize),
    /// Stripped content, exactly what `Match::raw` will carry.
    raw: String,
    /// Verbatim opening line (fence row / top rule row / first
    /// guttered line) — `Match::context`.
    context: String,
}

impl Candidate {
    /// Summary for the list row: `sh · 3 lines · cargo build`.
    fn display(&self) -> String {
        let label = match (self.kind, &self.lang) {
            ("fenced", Some(l)) if !l.is_empty() => l.clone(),
            ("fenced", _) => "code".to_string(),
            ("box", _) => "box".to_string(),
            ("gutter", _) => "numbers".to_string(),
            _ => "block".to_string(),
        };
        let lines = self.raw.split('\n').count();
        let first = self
            .raw
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        let head: String = first.trim().chars().take(60).collect();
        format!("{label} · {lines} lines · {head}")
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Find fenced, boxed, and line-numbered code blocks in `text`.
pub fn extract(text: &str) -> Vec<Match> {
    let lines = scan_lines(text);

    let fenced = find_fences(&lines);
    let mut boxes = find_boxes(&lines);
    boxes.extend(find_side_bar_runs(&lines));
    let mut gutters = find_gutters(&lines);

    // Fences are the most explicit marker: drop any box/gutter whose
    // span overlaps one (defensive — box content must open with a bar,
    // fence rows never do, so genuine overlaps are rare).
    let fspans: Vec<(usize, usize)> = fenced.iter().map(|c| c.span).collect();
    boxes.retain(|c| !fspans.iter().any(|f| overlaps(*f, c.span)));
    gutters.retain(|c| !fspans.iter().any(|f| overlaps(*f, c.span)));

    // Gutters inside a box drop: the box strip already removes the
    // inner number prefixes (decor's boxed+numbered path).
    let bspans: Vec<(usize, usize)> = boxes.iter().map(|c| c.span).collect();
    gutters.retain(|c| !bspans.iter().any(|b| contains(*b, c.span)));

    // Outermost wins within each kind.
    let mut out: Vec<Candidate> = drop_contained(fenced);
    out.extend(drop_contained(boxes));
    out.extend(drop_contained(gutters));

    out.sort_by_key(|c| c.span);
    out.into_iter()
        .map(|c| {
            let mut fields = HashMap::new();
            fields.insert("kind".to_string(), c.kind.to_string());
            fields.insert("lines".to_string(), c.raw.split('\n').count().to_string());
            if let Some(l) = &c.lang {
                if !l.is_empty() {
                    fields.insert("lang".to_string(), l.clone());
                }
            }
            Match {
                ty: MatchType::Block,
                raw: c.raw.clone(),
                display: c.display(),
                context: c.context,
                span: c.span,
                fields,
            }
        })
        .collect()
}

// ── Line indexing ──────────────────────────────────────────────────────────────

/// One input line: its byte offset and text.
struct LineRef<'a> {
    start: usize,
    text: &'a str,
}

fn scan_lines(text: &str) -> Vec<LineRef<'_>> {
    let mut out = Vec::new();
    let mut off = 0;
    for line in text.lines() {
        out.push(LineRef {
            start: off,
            text: line,
        });
        off += line.len() + 1; // +1 for the '\n' we split on
    }
    out
}

/// Byte offset one past the end of `lines[i]`'s text.
fn line_end(lines: &[LineRef<'_>], i: usize) -> usize {
    lines[i].start + lines[i].text.len()
}

// ── Fence detection ───────────────────────────────────────────────────────────

/// A parsed ` ``` `/`~~~` marker row.
struct FenceMark {
    indent: usize,
    ch: char,
    len: usize,
    /// Info string (`sh`, `python`); empty on a valid close row.
    info: String,
}

fn fence_mark(line: &str) -> Option<FenceMark> {
    let indent = line.chars().take_while(|&c| c == ' ').count();
    let rest = &line[indent..];
    let mut ch: Option<char> = None;
    let mut len = 0;
    for c in rest.chars() {
        match ch {
            None if c == '`' || c == '~' => {
                ch = Some(c);
                len = 1;
            }
            Some(f) if c == f => len += 1,
            _ => break,
        }
    }
    let ch = ch?;
    if len < 3 {
        return None;
    }
    // Both fence glyphs are 1 byte — the slice cut is safe.
    let info = rest[len..].trim().to_string();
    Some(FenceMark {
        indent,
        ch,
        len,
        info,
    })
}

fn find_fences(lines: &[LineRef<'_>]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let n = lines.len();
    let mut i = 0;
    while i < n {
        let Some(open) = fence_mark(lines[i].text) else {
            i += 1;
            continue;
        };
        // Close: same glyph, at least as long, no info string, no
        // deeper indentation.
        let mut close = None;
        for (k, line) in lines[i + 1..].iter().enumerate() {
            if let Some(m) = fence_mark(line.text) {
                if m.ch == open.ch
                    && m.len >= open.len
                    && m.info.is_empty()
                    && m.indent <= open.indent
                {
                    close = Some(i + 1 + k);
                    break;
                }
            }
        }
        if let Some(j) = close {
            let content = &lines[i + 1..j];
            let has_content = content.iter().any(|l| !l.text.trim().is_empty());
            if has_content && content.len() <= MAX_BLOCK_LINES {
                let raw = content
                    .iter()
                    .map(|l| l.text)
                    .collect::<Vec<_>>()
                    .join("\n");
                out.push(Candidate {
                    kind: "fenced",
                    lang: (!open.info.is_empty()).then(|| open.info.clone()),
                    span: (content[0].start, line_end(lines, j - 1)),
                    raw,
                    context: lines[i].text.to_string(),
                });
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ── Box detection (ported from flash decor.rs, buffer-wide) ───────────────────

/// Which frame edge a rule row is checked against.
enum Side {
    Top,
    Bottom,
}

/// Rule-row check, ported from decor.rs: opens with a corner of the
/// given edge, closes with the matching corner (after trailing
/// spaces), is at least 3 chars, and contains at least one rule/filler
/// glyph between the corners — title text in `╭· fish ···╮` sits
/// *between* fillers, so the middle may contain arbitrary characters.
fn is_rule_row(line: &str, side: Side) -> bool {
    let t = line.trim_end();
    let chars: Vec<char> = t.chars().collect();
    if chars.len() < 3 {
        return false;
    }
    let (lefts, rights) = match side {
        Side::Top => (&TOP_LEFT[..], &TOP_RIGHT[..]),
        Side::Bottom => (&BOTTOM_LEFT[..], &BOTTOM_RIGHT[..]),
    };
    if !lefts.contains(&chars[0]) || !rights.contains(&chars[chars.len() - 1]) {
        return false;
    }
    chars[1..chars.len() - 1]
        .iter()
        .any(|c| H_RULES.contains(c) || FILLERS.contains(c))
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|&c| c == ' ').count()
}

/// Drop `indent` leading spaces (safe: `indent` counts ASCII spaces).
fn skip_indent(line: &str, indent: usize) -> &str {
    let actual = line.chars().take_while(|&c| c == ' ').count();
    &line[actual.min(indent)..]
}

fn find_boxes(lines: &[LineRef<'_>]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let n = lines.len();
    let mut i = 0;
    while i < n {
        let ind = leading_spaces(lines[i].text);
        if !is_rule_row(skip_indent(lines[i].text, ind), Side::Top) {
            i += 1;
            continue;
        }
        // Gather content: bar lines (any Unicode bar first) plus
        // blank lines, until a bottom rule row, a non-bar non-blank
        // line, or EOF.
        let mut content: Vec<usize> = Vec::new();
        let mut bottom: Option<usize> = None;
        let mut j = i + 1;
        while j < n {
            let t = skip_indent(lines[j].text, ind);
            if is_rule_row(t, Side::Bottom) {
                bottom = Some(j);
                break;
            }
            if t.trim().is_empty() || BARS.contains(&t.chars().next().unwrap_or(' ')) {
                content.push(j);
                j += 1;
            } else {
                break;
            }
        }
        let nonempty = content
            .iter()
            .filter(|&&k| !lines[k].text.trim().is_empty())
            .count();
        // decor's evidence bar: with a bottom rule row (full frame)
        // one non-empty content line suffices; a frame clipped by the
        // capture edge needs two.
        let accept = match bottom {
            Some(_) => nonempty >= 1,
            None => nonempty >= 2,
        };
        let last = bottom.unwrap_or(j.saturating_sub(1));
        let total = last.saturating_sub(i + 1) + 1;
        if accept && !content.is_empty() && total <= MAX_BLOCK_LINES {
            // Every non-empty content line must open with the *same*
            // bar (decor's `common_bar`) — a mixed-glyph run is not
            // one coherent frame.
            let texts: Vec<&str> = content
                .iter()
                .map(|&k| skip_indent(lines[k].text, ind))
                .collect();
            let bar = texts
                .iter()
                .find(|t| !t.trim().is_empty())
                .and_then(|t| t.chars().next())
                .filter(|c| BARS.contains(c));
            if let Some(bar) = bar {
                if texts
                    .iter()
                    .filter(|t| !t.trim().is_empty())
                    .all(|t| t.starts_with(bar))
                {
                    let content_texts: Vec<&str> = content.iter().map(|&k| lines[k].text).collect();
                    let raw = strip_boxed(&content_texts, ind);
                    if !raw.trim().is_empty() {
                        let first = content[0];
                        let end = line_end(lines, *content.last().unwrap());
                        out.push(Candidate {
                            kind: "box",
                            lang: None,
                            span: (lines[first].start, end),
                            raw,
                            context: lines[i].text.to_string(),
                        });
                        i = last + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

/// Strip a box's side bars and any inner line-number gutter, ported
/// from decor.rs: prefix = margin + bar + one padding space, suffix =
/// trailing spaces + right bar (only when present), then `detect_gutter`
/// on the remainders (boxed `bat` output strips both).
fn strip_boxed(content: &[&str], indent: usize) -> String {
    // The bar must be read off the indent-stripped line - the raw
    // content line opens with the frame's leading margin.
    let bar = content
        .iter()
        .map(|l| skip_indent(l, indent))
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.chars().next())
        .unwrap_or('│');

    let mut prefix: Vec<usize> = vec![0; content.len()];
    let mut suffix: Vec<usize> = vec![0; content.len()];
    for (i, line) in content.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        // Margin + bar.
        let mut p = indent + 1;
        // One padding space after the bar, if present.
        if chars.get(p) == Some(&' ') {
            p += 1;
        }
        prefix[i] = p;
        // Right bar: last non-whitespace char is the bar → consume it
        // plus the whitespace run before it.
        if let Some(pos) = chars.iter().rposition(|c| !c.is_whitespace()) {
            if pos >= p && chars[pos] == bar {
                let mut s = pos;
                while s > p && chars[s - 1].is_whitespace() {
                    s -= 1;
                }
                suffix[i] = chars.len() - s;
            }
        }
    }

    // Inner gutter (decor's boxed+numbered path).
    let items: Vec<(usize, String)> = content
        .iter()
        .enumerate()
        .map(|(i, l)| (i, remainder(l, prefix[i], suffix[i])))
        .collect();
    if let Some(lens) = detect_gutter(&items) {
        for (i, len) in lens {
            prefix[i] += len;
        }
    }

    let mut out = Vec::with_capacity(content.len());
    for (i, line) in content.iter().enumerate() {
        out.push(remainder(line, prefix[i], suffix[i]));
    }
    out.join("\n")
}

/// Side-bars-only runs (no rule rows anywhere): consecutive lines
/// opening with the same Unicode bar at the same column, ≥2
/// non-empty. Blank lines inside the run are allowed (chat TUIs pad
/// messages); the run ends at the first non-bar non-blank line.
fn find_side_bar_runs(lines: &[LineRef<'_>]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let n = lines.len();
    let mut i = 0;
    while i < n {
        let ind = leading_spaces(lines[i].text);
        let Some(bar) = skip_indent(lines[i].text, ind).chars().next() else {
            i += 1;
            continue;
        };
        if !BARS.contains(&bar) {
            i += 1;
            continue;
        }
        let mut run: Vec<usize> = vec![i];
        let mut j = i + 1;
        while j < n {
            let t = lines[j].text;
            let indj = leading_spaces(t);
            let rest = &t[indj..];
            // Blank lines may sit inside the run; non-blank lines must
            // carry the same bar at the same column.
            let continues = rest.trim().is_empty() || (indj == ind && rest.starts_with(bar));
            if !continues {
                break;
            }
            run.push(j);
            j += 1;
        }
        // Trim trailing blank lines from the run (span/rarity only).
        while run.len() > 1 && lines[*run.last().unwrap()].text.trim().is_empty() {
            run.pop();
        }
        let nonempty = run
            .iter()
            .filter(|&&k| !lines[k].text.trim().is_empty())
            .count();
        if nonempty >= 2 && run.len() <= MAX_BLOCK_LINES {
            let content: Vec<&str> = run.iter().map(|&k| lines[k].text).collect();
            let raw = strip_boxed(&content, ind);
            if !raw.trim().is_empty() {
                let first = run[0];
                let end = line_end(lines, *run.last().unwrap());
                out.push(Candidate {
                    kind: "box",
                    lang: None,
                    span: (lines[first].start, end),
                    raw,
                    context: lines[first].text.to_string(),
                });
                i = *run.last().unwrap() + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ── Gutter detection (ported from flash decor.rs, run-based) ───────────────────

/// Parsed numeric gutter prefix of a single line: `[spaces][digits]`
/// then an optional single space, then a separator (`:` `|` `│` or tab
/// for `cat -n`), then an optional single space/tab. Returns the total
/// gutter length in chars plus the parsed counter value. Ported from
/// decor.rs.
fn parse_gutter_simple(s: &str) -> Option<(usize, u64)> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }
    let num_start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i == num_start {
        return None; // No number at the start.
    }
    let num: u64 = chars[num_start..i]
        .iter()
        .collect::<String>()
        .parse()
        .ok()?;
    let mut j = i;
    // bat-style: one space between number and `│`.
    if j < chars.len() && chars[j] == ' ' {
        j += 1;
    }
    if j < chars.len() && matches!(chars[j], ':' | '|' | '│' | '\t') {
        j += 1;
    } else {
        return None;
    }
    // The single space after bat's `│` separator is gutter padding —
    // but only there. After `:`/`|` (grep/rg) a following space belongs
    // to the content (often code indentation) and must survive.
    if j < chars.len() && chars[j - 1] == '│' && (chars[j] == ' ' || chars[j] == '\t') {
        j += 1;
    }
    Some((j, num))
}

fn strictly_increasing(nums: &[u64]) -> bool {
    nums.windows(2).all(|w| w[0] < w[1])
}

/// Detect a line-number gutter across `(item index, text)` items,
/// ported from decor.rs. Pass 1: bare counter (`  12 │ text`,
/// `12:text`). Pass 2: identical path-like prefix ending in `:`
/// before the counter (`rg`/`git grep` multi-file). Empty items are
/// skipped. Returns per-item gutter char-lengths, or `None`.
fn detect_gutter(items: &[(usize, String)]) -> Option<Vec<(usize, usize)>> {
    let nonempty: Vec<(usize, &str)> = items
        .iter()
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(i, t)| (*i, t.as_str()))
        .collect();
    if nonempty.len() < 2 {
        return None;
    }

    // Pass 1: bare counter at line start.
    let parsed: Vec<Option<(usize, u64)>> = nonempty
        .iter()
        .map(|(_, t)| parse_gutter_simple(t))
        .collect();
    if parsed.iter().all(|p| p.is_some()) {
        let nums: Vec<u64> = parsed.iter().map(|p| p.unwrap().1).collect();
        if strictly_increasing(&nums) {
            return Some(
                items
                    .iter()
                    .map(|(i, t)| {
                        let len = if t.trim().is_empty() {
                            0
                        } else {
                            parse_gutter_simple(t).expect("just validated").0
                        };
                        (*i, len)
                    })
                    .collect(),
            );
        }
    }

    // Pass 2: identical literal prefix ending in ':' before the
    // counter. Longest candidate that validates across every item wins.
    let first = nonempty[0].1;
    let mut best: Option<(String, usize)> = None;
    for (byte_idx, c) in first.char_indices() {
        if c != ':' {
            continue;
        }
        let prefix = &first[..byte_idx + 1]; // ':' is 1 byte — safe cut.
        if !prefix.contains('/') && !prefix.contains('.') {
            continue; // Must look like a path, not `name:port`.
        }
        let prefix_len = prefix.chars().count();
        if nonempty.iter().all(|(_, t)| {
            t.starts_with(prefix) && parse_gutter_simple(&t[prefix.len()..]).is_some()
        }) {
            let nums: Vec<u64> = nonempty
                .iter()
                .map(|(_, t)| parse_gutter_simple(&t[prefix.len()..]).unwrap().1)
                .collect();
            if strictly_increasing(&nums) {
                let p = prefix.to_string();
                if best
                    .as_ref()
                    .is_none_or(|(b, _)| b.chars().count() < prefix_len)
                {
                    best = Some((p, prefix_len));
                }
            }
        }
    }
    if let Some((prefix, prefix_len)) = best {
        return Some(
            items
                .iter()
                .map(|(i, t)| {
                    let len = if t.trim().is_empty() {
                        0
                    } else {
                        prefix_len + parse_gutter_simple(&t[prefix.len()..]).unwrap().0
                    };
                    (*i, len)
                })
                .collect(),
        );
    }

    None
}

/// Maximal runs of consecutive non-blank lines; blank lines end a run
/// (real `bat`/`rg` listings have none, and merging across blanks
/// would stitch unrelated output together).
fn find_gutters(lines: &[LineRef<'_>]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let n = lines.len();
    let mut i = 0;
    while i < n {
        if lines[i].text.trim().is_empty() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && !lines[j].text.trim().is_empty() {
            j += 1;
        }
        let run = &lines[i..j];
        if run.len() >= 2 && run.len() <= MAX_BLOCK_LINES {
            let items: Vec<(usize, String)> = run
                .iter()
                .enumerate()
                .map(|(k, l)| (k, l.text.to_string()))
                .collect();
            if let Some(lens) = detect_gutter(&items) {
                let raw = run
                    .iter()
                    .enumerate()
                    .map(|(k, l)| {
                        let len = lens[k].1;
                        let chars: Vec<char> = l.text.chars().collect();
                        chars[len.min(chars.len())..].iter().collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !raw.trim().is_empty() {
                    out.push(Candidate {
                        kind: "gutter",
                        lang: None,
                        span: (run[0].start, line_end(lines, j - 1)),
                        raw,
                        context: run[0].text.to_string(),
                    });
                }
            }
        }
        i = j;
    }
    out
}

// ── Small helpers ─────────────────────────────────────────────────────────────

/// Text between the decoration columns of a line (ported from
/// decor.rs).
fn remainder(line: &str, prefix: usize, suffix: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let start = prefix.min(chars.len());
    let end = (chars.len().saturating_sub(suffix)).max(start);
    chars[start..end].iter().collect()
}

/// `a` fully contains `b` (byte spans, end-exclusive), and they differ.
fn contains(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 <= b.0 && b.1 <= a.1 && a != b
}

/// `a` and `b` share at least one byte.
fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Drop every candidate contained in a same-kind candidate (equal
/// spans keep the first — e.g. a full-frame box and its side-bar
/// run over the same content). `n` is tiny, so O(n²) is fine.
fn drop_contained(cands: Vec<Candidate>) -> Vec<Candidate> {
    let keep: Vec<bool> = cands
        .iter()
        .enumerate()
        .map(|(i, c)| {
            !cands.iter().enumerate().any(|(j, o)| {
                j != i
                    && o.span.0 <= c.span.0
                    && c.span.1 <= o.span.1
                    && (o.span != c.span || j < i)
            })
        })
        .collect();
    cands
        .into_iter()
        .zip(keep)
        .filter_map(|(c, k)| k.then_some(c))
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Match {
        let m = extract(text);
        assert_eq!(m.len(), 1, "expected exactly one block: {m:?}");
        m.into_iter().next().unwrap()
    }

    // ── Fences ────────────────────────────────────────────────────────────────

    #[test]
    fn fenced_block_with_lang() {
        let m = one("intro\n```sh\ncd ~/proj\ncargo build\n```\noutro");
        assert_eq!(m.fields["kind"], "fenced");
        assert_eq!(m.fields["lang"], "sh");
        assert_eq!(m.fields["lines"], "2");
        assert_eq!(m.raw, "cd ~/proj\ncargo build");
        assert_eq!(m.display, "sh · 2 lines · cd ~/proj");
        // Span covers the content lines, not the fence rows.
        let start = "intro\n```sh\n".len();
        assert_eq!(m.span, (start, start + "cd ~/proj\ncargo build".len()));
        // Context is the verbatim opening fence row.
        assert_eq!(m.context, "```sh");
    }

    #[test]
    fn fenced_block_without_lang_labels_code() {
        let m = one("```\nset -gx DEMO_HOME ~/.config/demo\n```");
        assert!(!m.fields.contains_key("lang"));
        assert!(m.display.starts_with("code · 1 lines · "));
        assert_eq!(m.raw, "set -gx DEMO_HOME ~/.config/demo");
    }

    #[test]
    fn tilde_fence_and_longer_close() {
        let m = one("~~~\nalpha\nbeta\n~~~~~~");
        assert_eq!(m.fields["kind"], "fenced");
        assert_eq!(m.raw, "alpha\nbeta");
    }

    #[test]
    fn unclosed_fence_never_fires() {
        // A ~~~ open with no later ~~~ close is skipped — and it must
        // not swallow or suppress subsequent ``` fences (close
        // requires the same glyph).
        let m = extract("~~~\nstray open only\n```sh\nok\n```");
        assert_eq!(m.len(), 1, "{m:?}");
        assert_eq!(m[0].raw, "ok");
        assert!(!m.iter().any(|x| x.raw.contains("stray")));
    }

    #[test]
    fn no_close_at_all_fires_nothing() {
        // ```sh carries an info string so it can never close the ~~~;
        // neither open finds a close before EOF.
        assert!(extract("~~~\na\n```sh\nstill content").is_empty());
    }

    #[test]
    fn blank_content_lines_kept_in_fenced_raw() {
        let m = one("```python\nfrom demo import Client\n\nc.panes()\n```");
        assert_eq!(m.raw, "from demo import Client\n\nc.panes()");
        assert_eq!(m.fields["lines"], "3");
    }

    // ── Boxes ─────────────────────────────────────────────────────────────────

    #[test]
    fn pi_fish_rounded_box_strips_frame() {
        let text = "\
╭· fish ·····································╮
│   cd ~/.dotfiles/dotSrc/app_coding_agents  │
│   git checkout feat/demo-s5                │
│                                            │
│   # 1. Temp workspace                      │
╰············································╯";
        let m = one(text);
        assert_eq!(m.fields["kind"], "box");
        assert_eq!(
            m.raw,
            "  cd ~/.dotfiles/dotSrc/app_coding_agents\n  \
             git checkout feat/demo-s5\n\n  # 1. Temp workspace"
        );
        assert_eq!(m.context, "╭· fish ·····································╮");
        assert!(m.display.starts_with("box · 4 lines · "));
    }

    #[test]
    fn heavy_and_double_frames() {
        assert_eq!(one("┏━━━━━━┓\n┃ heavy┃\n┗━━━━━━┛").raw, "heavy");
        assert_eq!(one("╔═══════╗\n║ double║\n╚═══════╝").raw, "double");
    }

    #[test]
    fn clipped_box_without_bottom_rule() {
        // Frame clipped by the capture edge: top rule + bars, no
        // bottom. ≥2 non-empty content lines are required.
        let m = one("╭────────╮\n│ clipped │\n│ frame   │");
        assert_eq!(m.raw, "clipped\nframe");
    }

    #[test]
    fn clipped_box_needs_two_content_lines() {
        // decor allows a single framed line when rule rows are in
        // range, but buffer-wide a lone bar line under a rule row is
        // too little evidence (spinner frames) — clipped requires 2.
        assert!(extract("╭────╮\n│ one │").is_empty());
    }

    #[test]
    fn indented_box_strips_margin() {
        let m = one("   ╭──────────╮\n   │ hello    │\n   ╰──────────╯");
        assert_eq!(m.raw, "hello");
    }

    #[test]
    fn side_bars_only_run() {
        let m = one("│ alpha │\n│ beta  │\n│ gamma │");
        assert_eq!(m.fields["kind"], "box");
        assert_eq!(m.raw, "alpha\nbeta\ngamma");
    }

    #[test]
    fn side_bar_run_ends_at_non_bar_line() {
        let m = one("│ one │\n│ two │\nplain prose ends the run");
        assert_eq!(m.raw, "one\ntwo");
    }

    #[test]
    fn mixed_bars_reject_the_box() {
        assert!(extract("│ one │\n┃ two ┃").is_empty());
    }

    #[test]
    fn boxed_numbered_content_strips_both() {
        let text = "\
╭─────────────────╮
│   1 │ cd ~/x    │
│   2 │ git pull  │
╰─────────────────╯";
        let m = one(text);
        assert_eq!(m.raw, "cd ~/x\ngit pull");
    }

    #[test]
    fn overflowing_content_line_has_no_right_bar() {
        let text = "\
╭──────╮
│ short│
│ this line is far longer than the frame and has no closing bar
╰──────╯";
        assert_eq!(
            one(text).raw,
            "short\nthis line is far longer than the frame and has no closing bar"
        );
    }

    #[test]
    fn fence_inside_a_box_is_box_content_not_a_block() {
        // The fence rows sit behind the bar and are never fence
        // candidates; the box match carries them verbatim in `raw`.
        let text = "\
╭─ snippet ────────╮
│ ```sh           │
│ demo run --all  │
│ ```             │
╰─────────────────╯";
        let m = one(text);
        assert_eq!(m.fields["kind"], "box");
        assert!(m.raw.contains("demo run --all"));
        assert!(m.raw.contains("```sh"));
    }

    // ── Gutters ───────────────────────────────────────────────────────────────

    #[test]
    fn bat_gutter() {
        let m = one("   1 │ cd ~/somewhere\n   2 │ git status\n  10 │ cargo build");
        assert_eq!(m.fields["kind"], "gutter");
        assert_eq!(m.raw, "cd ~/somewhere\ngit status\ncargo build");
        assert!(m.display.starts_with("numbers · 3 lines · "));
    }

    #[test]
    fn cat_n_gutter() {
        assert_eq!(one("     1\talpha\n     2\tbeta").raw, "alpha\nbeta");
    }

    #[test]
    fn rg_multifile_gutter() {
        let m = one(
            "src/main.rs:12:fn main() {\nsrc/main.rs:13:    println!(\"hi\");\nsrc/main.rs:14:}",
        );
        assert_eq!(m.raw, "fn main() {\n    println!(\"hi\");\n}");
    }

    #[test]
    fn grep_n_counter_jumps_allowed() {
        assert_eq!(
            one("12:first match\n13:second match\n40:third match").raw,
            "first match\nsecond match\nthird match"
        );
    }

    #[test]
    fn gutter_run_breaks_at_blank_line() {
        let m = extract("1: alpha\n2: beta\n\n1: gamma\n2: delta");
        assert_eq!(m.len(), 2, "two separate runs: {m:?}");
    }

    // ── Negatives: look-alikes must not fire ───────────────────────────────────

    #[test]
    fn markdown_table_is_not_a_block() {
        assert!(extract("| key | value |\n|-----|-------|\n| a   | 1     |").is_empty());
    }

    #[test]
    fn ascii_art_is_not_a_block() {
        assert!(extract("+--------+\n|  art   |\n+--------+").is_empty());
    }

    #[test]
    fn non_monotonic_numbered_code_is_not_a_gutter() {
        assert!(extract("12: this is code\n5: more code\n99: more code").is_empty());
    }

    #[test]
    fn plain_numbered_prose_is_not_a_gutter() {
        assert!(extract("3 apples\n4 pears").is_empty());
        assert!(extract("host:8080:/a\nhost:8081:/b").is_empty());
    }

    #[test]
    fn lone_rule_rows_and_lone_bar_lines_are_not_blocks() {
        assert!(extract("╭────╮\n╰────╯").is_empty());
        assert!(extract("│ only one line │").is_empty());
    }

    // ── Caps and arbitration ──────────────────────────────────────────────────

    #[test]
    fn giant_blocks_are_skipped() {
        let mut text = String::from("```sh\n");
        for i in 0..MAX_BLOCK_LINES + 5 {
            text.push_str(&format!("line {i}\n"));
        }
        text.push_str("```");
        assert!(extract(&text).is_empty());
    }

    #[test]
    fn adjacent_blocks_are_separate_matches() {
        // block::extract returns span order (oldest first); the
        // matcher pipeline re-sorts recency-first for the picker.
        let m = extract("```sh\na\n```\n\n```sh\nb\n```");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].raw, "a");
        assert_eq!(m[1].raw, "b");
    }

    #[test]
    fn blocks_do_not_hide_matches_inside_them() {
        // The pipeline-level guarantee lives in matcher::mod's dedup
        // (by raw text), but assert here that extract() itself is a
        // pure add-on: a URL inside a fenced block still comes out of
        // the combined pipeline (tested in mod.rs fixture tests), while
        // block::extract reports the block only.
        let text = "```sh\ncurl -fsSL https://example.com/install.sh | bash\n```";
        let m = one(text);
        assert_eq!(m.fields["kind"], "fenced");
        assert!(m.raw.contains("https://example.com"));
    }
}
