//! Pattern-matching engine, ported from the original zellij-zextract
//! plugin's `crates/zextract/src/{extract.rs,pattern/*.rs}`. Each
//! submodule exposes a `pub fn extract(text: &str) -> Vec<Match>` that
//! finds all matches of its type; [`extract`] runs all of them and
//! dedupes the combined result.
//!
//! Deferred vs. the original (tracked for later phases):
//!   - User config (`PatternsConfig`: disabled types, custom patterns,
//!     command/secret tuning) - Phase 5.
//!   - `command`'s opt-in flag/comment/extension-anchored passes, which
//!     are off by default upstream too - Phase 5, if ever.
//!   - Per-pattern timing instrumentation - not needed until there's a
//!     debug log to feed.

pub mod command;
pub mod diagnostic;
pub mod file;
pub mod git;
pub mod ipv4;
pub mod ipv6;
pub mod quoted;
pub mod secret;
pub mod sha;
pub mod url;
pub mod uuid;

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub ty: MatchType,
    pub raw: String,
    pub display: String,
    pub context: String,
    /// Byte offsets in the input text. Used for dedup tie-breaking
    /// (latest = larger `span.0`).
    pub span: (usize, usize),
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchType {
    Url,
    File,
    Diagnostic,
    Git,
    Sha,
    Ipv4,
    Ipv6,
    Uuid,
    QuotedString,
    Command,
    Secret,
}

impl MatchType {
    pub fn tag(self) -> &'static str {
        match self {
            MatchType::Url => "url",
            MatchType::File => "file",
            MatchType::Diagnostic => "diag",
            MatchType::Git => "git",
            MatchType::Sha => "sha",
            MatchType::Ipv4 => "ipv4",
            MatchType::Ipv6 => "ipv6",
            MatchType::Uuid => "uuid",
            MatchType::QuotedString => "quote",
            MatchType::Command => "cmd",
            MatchType::Secret => "secret",
        }
    }
}

/// Type-priority list, front of list = highest priority. Drives
/// cross-type dedup (same raw text matched by two pattern types keeps
/// whichever type ranks earliest here).
const TYPE_PRIORITY: &[MatchType] = &[
    MatchType::Url,
    MatchType::Diagnostic,
    MatchType::File,
    MatchType::Uuid,
    MatchType::Git, // wins over bare Sha when hash appears in a git log line
    MatchType::Sha,
    MatchType::Ipv4,
    MatchType::Ipv6,
    MatchType::Command,
    MatchType::Secret, // entropy fallback is broad; let specific types win
    MatchType::QuotedString,
];

fn type_priority_index(ty: MatchType) -> usize {
    TYPE_PRIORITY
        .iter()
        .position(|&t| t == ty)
        .unwrap_or(TYPE_PRIORITY.len())
}

/// Trim trailing punctuation that's commonly adjacent to a match in
/// prose but not part of it. Used by URL, file, diagnostic etc.
pub fn trim_trailing_punct(s: &str) -> &str {
    s.trim_end_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>' | '"' | '\''
        )
    })
}

/// Run all built-in patterns against `text` and return the combined,
/// deduped, recency-ordered (latest match first) matches.
pub fn extract(text: &str) -> Vec<Match> {
    let mut all: Vec<Match> = Vec::new();
    all.extend(url::extract(text));
    all.extend(file::extract(text));
    all.extend(diagnostic::extract(text));
    all.extend(git::extract(text));
    all.extend(sha::extract(text));
    all.extend(ipv4::extract(text));
    all.extend(ipv6::extract(text));
    all.extend(uuid::extract(text));
    all.extend(quoted::extract(text));
    all.extend(command::extract(text));
    all.extend(secret::extract(text));

    let pass1 = dedup_keep_latest(all);
    dedup_by_raw_priority(pass1)
}

/// Pass 1: dedup by `(type, raw)`, keeping the latest occurrence.
fn dedup_keep_latest(mut matches: Vec<Match>) -> Vec<Match> {
    matches.sort_by_key(|m| m.span.0);
    let mut seen: HashSet<(MatchType, String)> = HashSet::new();
    let mut out: Vec<Match> = Vec::with_capacity(matches.len());
    for m in matches.into_iter().rev() {
        if seen.insert((m.ty, m.raw.clone())) {
            out.push(m);
        }
    }
    out
}

/// Pass 2: dedup by `raw` alone. When multiple types match the same raw
/// text, keep the one with the highest priority (front-of-list in
/// `TYPE_PRIORITY`). Ties resolved by recency (larger `span.0` wins).
/// Returns matches in latest-first order.
fn dedup_by_raw_priority(matches: Vec<Match>) -> Vec<Match> {
    let mut by_raw: HashMap<String, Match> = HashMap::new();
    for m in matches {
        let key = m.raw.clone();
        match by_raw.entry(key) {
            Entry::Vacant(e) => {
                e.insert(m);
            }
            Entry::Occupied(mut e) => {
                let incumbent = e.get();
                let new_prio = type_priority_index(m.ty);
                let cur_prio = type_priority_index(incumbent.ty);
                let replace = if new_prio < cur_prio {
                    true
                } else if new_prio == cur_prio {
                    m.span.0 > incumbent.span.0
                } else {
                    false
                };
                if replace {
                    e.insert(m);
                }
            }
        }
    }
    let mut out: Vec<Match> = by_raw.into_values().collect();
    out.sort_by_key(|m| std::cmp::Reverse(m.span.0));
    out
}
