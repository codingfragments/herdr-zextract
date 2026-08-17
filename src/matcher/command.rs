//! Command pattern: hybrid prompt-anchored + executable-anchored detection.
//!
//! Strategy:
//!   1. PROMPT-ANCHORED: line starts with a recognized prompt marker
//!      (`❯ `, `$ `, `> `, `% `, `# `). The command is the rest of the line
//!      plus any trailing-backslash continuation lines spliced in.
//!   2. EXEC-ANCHORED (fallback): line contains a known trigger executable
//!      (`sudo`, `curl`, `wget`, `cat`, `git`, ...). The command runs from
//!      the trigger to end-of-line. No continuation splicing for the exec
//!      flavor — too risky when embedded in prose.
//!
//! The original plugin also has three opt-in passes (flag-anchored,
//! extension-anchored, comment-anchored) that are off by default there
//! too — not ported here; revisit in Phase 5 if wanted.
//!
//! Captures `{match}` (and `{hint}` when an inline `# comment` follows).

use std::collections::HashMap;
use std::sync::OnceLock;

use regex_lite::Regex;

use super::{Match, MatchType};

const MAX_CONTINUATION_LINES: usize = 10;

/// Minimum character length for a command match. Filters out spurious
/// single-word or near-empty matches (e.g. bare `❯` lines, lone `$`).
const MIN_COMMAND_LEN: usize = 5;

/// Number of consecutive whitespace characters that signals the start of
/// a right-side prompt (rprompt) or other trailing noise, dropped from
/// the captured command. 5 avoids false positives on double-spaced
/// output (e.g. `git diff --stat`) while reliably catching rprompts.
const RPROMPT_MIN_SPACES: usize = 5;

/// Returns true if `s` looks like a plausible command — must contain at
/// least one ASCII letter. Rejects pure-numeric/punctuation strings such
/// as fish's right-aligned timestamp (`18:48:12`) that bleed onto an
/// otherwise empty prompt line in the terminal scrollback.
fn looks_like_command(s: &str) -> bool {
    s.trim().len() >= MIN_COMMAND_LEN && s.trim().chars().any(|c| c.is_ascii_alphabetic())
}

/// Default prompt markers.
const PROMPT_MARKERS: &[&str] = &["❯ ", "$ ", "> ", "% ", "# "];

/// Default trigger list.
const TRIGGERS: &[&str] = &[
    // Install / package managers
    "sudo",
    "apt",
    "apt-get",
    "yum",
    "dnf",
    "pacman",
    "brew",
    "snap",
    "pip",
    "pip3",
    "pipx",
    "gem",
    "cargo",
    "go",
    "npm",
    "yarn",
    "pnpm",
    "bun",
    "uv",
    "poetry",
    "conda",
    "mamba",
    // Fetch
    "curl",
    "wget",
    "fetch",
    // Shell exec
    "sh",
    "bash",
    "zsh",
    "fish",
    "/bin/sh",
    "/bin/bash",
    // Build
    "make",
    "cmake",
    "ninja",
    "just",
    "nix",
    "nix-shell",
    "nix-build",
    // Editor / pager / IO
    "nvim",
    "vim",
    "nano",
    "emacs",
    "less",
    "more",
    "cat",
    "tee",
    "xargs",
    "awk",
    "sed",
    "grep",
    "find",
    // VCS
    "git",
    "hg",
    "svn",
    // Containers / orchestration / multiplexers
    "docker",
    "podman",
    "kubectl",
    "helm",
    "zellij",
    "tmux",
    "herdr",
    // Language runners
    "python",
    "python3",
    "node",
    "deno",
    "ruby",
    "rustc",
    "java",
    "mvn",
    "gradle",
    // File ops
    "tar",
    "gunzip",
    "unzip",
    "chmod",
    "chown",
    "ln",
    "mkdir",
    "rm",
    "cp",
    "mv",
    "ssh",
    "scp",
    "rsync",
];

/// Patterns we strip from the start of a continuation line during
/// splicing. Order: most specific first.
const CONTINUATION_STRIP: &[&str] = &[
    r"^\s*\d+[:\.]?\s+", // line numbers ("  42  ", "2: ", "2. ")
    r"^[+\-]\s+",        // diff add/remove markers
    r"^[#>|]\s+",        // comment / quote / table-cell markers
    r"^\s+",             // leading whitespace (catchall)
];

fn trigger_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let pattern = format!(
            r"\b({})\b",
            TRIGGERS
                .iter()
                .map(|t| regex_escape(t))
                .collect::<Vec<_>>()
                .join("|")
        );
        Regex::new(&pattern).expect("trigger regex compiles")
    })
}

fn continuation_strip_regexes() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        CONTINUATION_STRIP
            .iter()
            .map(|p| Regex::new(p).expect("continuation-strip regex compiles"))
            .collect()
    })
}

fn regex_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\'
            | '/' => format!(r"\{}", c),
            other => other.to_string(),
        })
        .collect()
}

pub fn extract(text: &str) -> Vec<Match> {
    let lines: Vec<&str> = text.lines().collect();
    let line_offsets: Vec<usize> = compute_line_offsets(&lines);

    let mut out = Vec::new();
    let mut skip_until: usize = 0;

    for (i, line) in lines.iter().enumerate() {
        if i < skip_until {
            continue;
        }
        if is_comment_line(line) {
            continue; // whole-line comment (#… or //…) — nothing to extract
        }

        // 1. PROMPT-ANCHORED.
        if let Some((prompt_len, cmd_after_prompt)) = match_prompt(line) {
            // Strip inline comment before rprompt-trim so `\ # hint` sequences
            // don't get swallowed by the wide-space trim.
            let (cmd_no_comment, hint) = strip_inline_comment(cmd_after_prompt);
            let cmd_first = trim_rprompt(cmd_no_comment, RPROMPT_MIN_SPACES);
            if !cmd_first.trim().is_empty() {
                let (full_cmd, context, lines_consumed) = splice_continuation(&lines, i, cmd_first);
                let raw_cmd = full_cmd.trim_end();
                if looks_like_command(raw_cmd) {
                    let span_start = line_offsets[i] + prompt_len;
                    let span_end = if lines_consumed == 1 {
                        span_start + cmd_first.len()
                    } else {
                        line_offsets[i + lines_consumed - 1] + lines[i + lines_consumed - 1].len()
                    };
                    out.push(make_match(
                        raw_cmd.to_string(),
                        hint,
                        context,
                        span_start,
                        span_end,
                    ));
                    skip_until = i + lines_consumed;
                    continue;
                }
            }
        }

        // 2. EXEC-ANCHORED (fallback). No continuation splice — too risky in prose.
        // Scan only the pre-comment portion so triggers inside `# …` or `// …`
        // inline comments are not matched.
        if let Some(start_col) = match_exec(pre_comment_line(line)) {
            let (cmd_no_comment, hint) = strip_inline_comment(&line[start_col..]);
            let raw_cmd = trim_rprompt(cmd_no_comment, RPROMPT_MIN_SPACES).trim_end();
            if looks_like_command(raw_cmd) {
                let span_start = line_offsets[i] + start_col;
                let span_end = span_start + raw_cmd.len();
                out.push(make_match(
                    raw_cmd.to_string(),
                    hint,
                    line.to_string(),
                    span_start,
                    span_end,
                ));
            }
        }
    }
    out
}

fn compute_line_offsets(lines: &[&str]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(lines.len());
    let mut off = 0;
    for line in lines {
        offsets.push(off);
        off += line.len() + 1; // +1 for '\n'
    }
    offsets
}

/// If `line` begins with a known prompt marker, return (marker_len, rest).
fn match_prompt(line: &str) -> Option<(usize, &str)> {
    for marker in PROMPT_MARKERS {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some((marker.len(), rest));
        }
    }
    None
}

/// Return the byte column where a leftmost trigger occurs in `line`, or
/// None if no trigger fires. Filters out triggers that aren't in a
/// command-start context — \b alone matches `sh` inside `install.sh`
/// (the `.` is a non-word char so a word boundary exists), so we
/// additionally require the byte preceding the trigger to be a real
/// command-start (whitespace, line start, shell operator, ...).
fn match_exec(line: &str) -> Option<usize> {
    let re = trigger_regex();
    for m in re.find_iter(line) {
        let start = m.start();
        let prev = if start == 0 {
            None
        } else {
            line.as_bytes().get(start - 1).copied()
        };
        if ok_command_preceding_byte(prev) {
            return Some(start);
        }
    }
    None
}

fn ok_command_preceding_byte(b: Option<u8>) -> bool {
    match b {
        None => true,
        Some(c) if c.is_ascii_whitespace() => true,
        // Shell separators / operators + prose punctuation that can
        // precede a command word.
        Some(
            b'|' | b';' | b'&' | b'(' | b'[' | b'{' | b'`' | b'$' | b'=' | b'>' | b'<' | b'"'
            | b'\'' | b':' | b',',
        ) => true,
        // `.` and `/` are explicitly rejected — they signal file-extension
        // (`install.sh`) or path-component (`/usr/bin/sh`) context, not a
        // standalone command word.
        _ => false,
    }
}

/// Splice a command's continuation lines (trailing `\`). Returns
/// `(full_command_text, full_context, lines_consumed)`. `lines_consumed`
/// is at least 1 (the starting line itself).
///
/// Each continuation line has its leading noise stripped, then its own
/// rprompt gap and inline `# comment` removed before being appended.
fn splice_continuation(
    lines: &[&str],
    start_idx: usize,
    first_cmd: &str,
) -> (String, String, usize) {
    let mut cmd = first_cmd.to_string();
    let mut context = lines[start_idx].to_string();
    let mut consumed = 1usize;
    let strip_res = continuation_strip_regexes();

    while ends_with_continuation(&cmd) && consumed < MAX_CONTINUATION_LINES {
        let next_idx = start_idx + consumed;
        if next_idx >= lines.len() {
            break;
        }
        let next_line = lines[next_idx];
        // Strip leading noise, then inline comment, then rprompt gap.
        // Comment-first avoids `\       # hint` sequences being eaten by rprompt.
        let stripped = strip_leading(next_line, strip_res);
        let (stripped, _) = strip_inline_comment(stripped);
        let stripped = trim_rprompt(stripped, RPROMPT_MIN_SPACES).trim_end();
        // Drop trailing backslash AND any whitespace around it, then add
        // exactly one space before the spliced continuation.
        let trimmed_len = cmd
            .trim_end_matches(|c: char| c.is_whitespace() || c == '\\')
            .len();
        cmd.truncate(trimmed_len);
        cmd.push(' ');
        cmd.push_str(stripped);
        context.push('\n');
        context.push_str(next_line);
        consumed += 1;
    }
    (cmd, context, consumed)
}

fn ends_with_continuation(s: &str) -> bool {
    s.trim_end().ends_with('\\')
}

/// Truncate `s` at the first run of `min_spaces` or more consecutive ASCII
/// whitespace characters. Fish/zsh right-side prompts (timestamps, git status)
/// are pushed to the right edge with a wide column of spaces; `min_spaces`
/// controls how many spaces in a row constitute a cut point.
fn trim_rprompt(s: &str, min_spaces: usize) -> &str {
    if min_spaces == 0 {
        return s;
    }
    let b = s.as_bytes();
    let mut run = 0usize;
    for (i, &byte) in b.iter().enumerate() {
        if byte.is_ascii_whitespace() {
            run += 1;
            if run >= min_spaces {
                return &s[..i + 1 - run];
            }
        } else {
            run = 0;
        }
    }
    s
}

fn strip_leading<'a>(line: &'a str, patterns: &[Regex]) -> &'a str {
    for re in patterns {
        if let Some(m) = re.find(line) {
            if m.start() == 0 {
                return &line[m.end()..];
            }
        }
    }
    line
}

/// Return true if `line` is a comment line (`#…` or `//…`), optionally
/// preceded by whitespace. Such lines are skipped by extraction.
fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('#') || t.starts_with("//")
}

/// Truncate `line` at the first unambiguous inline comment start:
///   `#`  preceded by whitespace (or at col 0)
///   `//` preceded by whitespace (or at col 0)
/// Prevents exec-anchored from firing on triggers inside comment text.
/// URL `://` is safe because `:` is not whitespace.
fn pre_comment_line(line: &str) -> &str {
    let b = line.as_bytes();
    for i in 0..b.len() {
        let prev_ws = i == 0 || b[i - 1].is_ascii_whitespace();
        if b[i] == b'#' && prev_ws {
            return &line[..i];
        }
        if b[i] == b'/' && b.get(i + 1) == Some(&b'/') && prev_ws {
            return &line[..i];
        }
    }
    line
}

fn strip_inline_comment(s: &str) -> (&str, Option<&str>) {
    if let Some(pos) = s.find(" # ") {
        let hint = s[pos + 3..].trim();
        if !hint.is_empty() {
            return (s[..pos].trim_end(), Some(hint));
        }
    }
    (s, None)
}

fn make_match(
    raw: String,
    hint: Option<&str>,
    context: String,
    span_start: usize,
    span_end: usize,
) -> Match {
    let mut fields = HashMap::new();
    fields.insert("match".to_string(), raw.clone());
    if let Some(h) = hint {
        fields.insert("hint".to_string(), h.to_string());
    }
    Match {
        ty: MatchType::Command,
        raw: raw.clone(),
        display: raw,
        context,
        span: (span_start, span_end),
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_anchored_simple() {
        let m = extract("$ git log --oneline -n 20");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].raw, "git log --oneline -n 20");
    }

    #[test]
    fn prompt_anchored_unicode() {
        let m = extract("❯ cargo build --release");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].raw, "cargo build --release");
    }

    #[test]
    fn exec_anchored_in_prose() {
        let m = extract("To install run sudo apt install zellij from the README.");
        assert_eq!(m.len(), 1);
        assert!(m[0].raw.starts_with("sudo apt install zellij"));
    }

    #[test]
    fn exec_anchored_pipeline_kept_together() {
        let m = extract("curl -fsSL https://example.com/install.sh | sudo bash");
        assert_eq!(m.len(), 1);
        assert!(m[0].raw.contains("curl"));
        assert!(m[0].raw.contains("| sudo bash"));
    }

    #[test]
    fn continuation_splicing_basic() {
        let text = "$ curl -fsSL https://example.com/install.sh \\\n    | sudo bash";
        let m = extract(text);
        assert_eq!(m.len(), 1);
        assert_eq!(
            m[0].raw,
            "curl -fsSL https://example.com/install.sh | sudo bash"
        );
    }

    #[test]
    fn continuation_strips_line_number_prefix() {
        let text = "$ curl -fsSL https://example.com/install.sh \\\n2:  | sudo bash";
        let m = extract(text);
        assert_eq!(m.len(), 1);
        assert_eq!(
            m[0].raw,
            "curl -fsSL https://example.com/install.sh | sudo bash"
        );
    }

    #[test]
    fn continuation_strips_diff_marker() {
        let text = "$ curl -fsSL https://example.com/install.sh \\\n+   | sudo bash";
        let m = extract(text);
        assert_eq!(
            m[0].raw,
            "curl -fsSL https://example.com/install.sh | sudo bash"
        );
    }

    #[test]
    fn continuation_capped_at_max_lines() {
        // 12 continuation lines — should stop at MAX_CONTINUATION_LINES.
        let mut text = String::from("$ echo \\");
        for _ in 0..12 {
            text.push_str("\n  hello \\");
        }
        text.push_str("\n  final");
        let m = extract(&text);
        assert_eq!(m.len(), 1);
        let backslash_count = m[0].raw.matches('\\').count();
        assert!(backslash_count > 0);
    }

    #[test]
    fn prompt_wins_over_exec_on_same_line() {
        let m = extract("❯ sudo apt install foo");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].raw, "sudo apt install foo");
    }

    #[test]
    fn no_match_in_random_prose() {
        let m = extract("the quick brown fox jumps over the lazy dog");
        assert!(m.is_empty());
    }

    #[test]
    fn rejects_trigger_inside_filename() {
        let m = extract("Downloaded install.sh from the mirror");
        assert!(m.is_empty(), "false positive: {m:?}");
    }

    #[test]
    fn rejects_trigger_inside_path() {
        let m = extract("path/to/sh detected");
        assert!(m.is_empty(), "false positive: {m:?}");
    }

    #[test]
    fn still_triggers_after_space() {
        let m = extract("Run sh -c 'foo' please");
        assert_eq!(m.len(), 1);
        assert!(m[0].raw.starts_with("sh"));
    }

    #[test]
    fn zellij_exec_anchored_in_output() {
        let m = extract("[dry-run] zellij --session claude-chats --layout cfdefault.kdl");
        assert_eq!(m.len(), 1);
        assert!(m[0].raw.starts_with("zellij --session"));
    }

    #[test]
    fn tmux_exec_anchored() {
        let m = extract("running: tmux new-session -s main");
        assert_eq!(m.len(), 1);
        assert!(m[0].raw.starts_with("tmux new-session"));
    }
}
