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
pub mod custom;
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

impl Match {
    /// The tag used for display, `#type` filtering, and the list row
    /// label. Custom patterns stash their configured name under the
    /// `__label` field; this returns that when present, otherwise
    /// `ty.tag()`. (A dedicated struct field would need touching every
    /// built-in pattern module's `Match` literal for a field only
    /// `custom.rs` ever sets - not worth the diff.)
    pub fn effective_tag(&self) -> &str {
        self.fields
            .get("__label")
            .map(String::as_str)
            .unwrap_or_else(|| self.ty.tag())
    }

    /// Set by `main.rs` in multi-pane (`tab-scan`) mode: which pane
    /// this match came from, for the `[title]  ` list row prefix.
    /// `None` in single-pane mode.
    pub fn source_pane_title(&self) -> Option<&str> {
        self.fields.get("__pane_title").map(String::as_str)
    }
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

    /// Inverse of [`tag`](Self::tag). Used to resolve a custom
    /// pattern's configured `type` string to a built-in `MatchType`.
    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "url" => Some(Self::Url),
            "file" => Some(Self::File),
            "diag" => Some(Self::Diagnostic),
            "git" => Some(Self::Git),
            "sha" => Some(Self::Sha),
            "ipv4" => Some(Self::Ipv4),
            "ipv6" => Some(Self::Ipv6),
            "uuid" => Some(Self::Uuid),
            "quote" => Some(Self::QuotedString),
            "cmd" => Some(Self::Command),
            "secret" => Some(Self::Secret),
            _ => None,
        }
    }
}

/// Type-priority list, front of list = highest priority. Drives
/// cross-type dedup (same raw text matched by two pattern types keeps
/// whichever type ranks earliest here).
pub const TYPE_PRIORITY: &[MatchType] = &[
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

/// Picker-rank score bonus derived from priority list position.
/// Symmetric around the middle: front of list = positive bonus, middle
/// = 0, tail = negative. Used by the picker's fuzzy filter to bias
/// relative ranking when fuzzy scores are close.
pub fn type_priority_bonus(ty: MatchType) -> i32 {
    let n = TYPE_PRIORITY.len() as i32;
    let pos = type_priority_index(ty) as i32;
    n / 2 - pos
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

/// Run all built-in patterns (no user config) against `text`. A thin
/// convenience wrapper over [`extract_with_config`] for callers (and
/// most tests) that don't need custom patterns or disabled types.
/// Production code (`main.rs`) uses `extract_with_config` directly, so
/// this is unused outside `#[cfg(test)]` in a normal build.
#[allow(dead_code)]
pub fn extract(text: &str) -> Vec<Match> {
    extract_with_config(text, &crate::config::Config::default())
}

/// Run all built-in patterns plus `config`'s custom patterns against
/// `text`, honoring `config.disabled` and `config.secret_entropy_filter`,
/// and return the combined, deduped, recency-ordered (latest match
/// first) matches.
pub fn extract_with_config(text: &str, config: &crate::config::Config) -> Vec<Match> {
    let dis = &config.disabled;
    let mut all: Vec<Match> = Vec::new();
    if !dis.contains("url") {
        all.extend(url::extract(text));
    }
    if !dis.contains("file") {
        all.extend(file::extract(text));
    }
    if !dis.contains("diag") {
        all.extend(diagnostic::extract(text));
    }
    if !dis.contains("git") {
        all.extend(git::extract(text));
    }
    if !dis.contains("sha") {
        all.extend(sha::extract(text));
    }
    if !dis.contains("ipv4") {
        all.extend(ipv4::extract(text));
    }
    if !dis.contains("ipv6") {
        all.extend(ipv6::extract(text));
    }
    if !dis.contains("uuid") {
        all.extend(uuid::extract(text));
    }
    if !dis.contains("quote") {
        all.extend(quoted::extract(text));
    }
    if !dis.contains("cmd") {
        all.extend(command::extract(text));
    }
    if !dis.contains("secret") {
        all.extend(secret::extract(text, config.secret_entropy_filter));
    }
    all.extend(custom::extract(text, &config.custom, dis));

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

#[cfg(test)]
mod fixture_tests {
    //! Integration coverage ported from the original plugin's
    //! `fixture_tests` module: read each fixture file and assert minimum
    //! counts per type against the combined `extract()` pipeline. Lighter
    //! than snapshot diffing but catches cross-pattern regressions that
    //! per-module unit tests can't (a match getting stolen by dedup, a
    //! pattern silently ceasing to fire on realistic multi-line text).
    //!
    //! `multi_group_patterns.txt` / `custom_patterns.txt` and their tests
    //! are not ported — they require `PatternsConfig` custom patterns,
    //! deferred to Phase 5.
    use super::*;

    fn count_by_type(text: &str, ty: MatchType) -> usize {
        extract(text).into_iter().filter(|m| m.ty == ty).count()
    }

    #[test]
    fn urls_fixture_has_urls() {
        let text = include_str!("../../tests/fixtures/urls.txt");
        assert!(count_by_type(text, MatchType::Url) >= 5);
    }

    #[test]
    fn files_fixture_has_files() {
        let text = include_str!("../../tests/fixtures/files.txt");
        assert!(count_by_type(text, MatchType::File) >= 3);
    }

    #[test]
    fn diagnostics_fixture_has_diagnostics() {
        let text = include_str!("../../tests/fixtures/diagnostics.txt");
        assert!(count_by_type(text, MatchType::Diagnostic) >= 2);
    }

    #[test]
    fn git_log_fixture_has_git_matches() {
        let text = include_str!("../../tests/fixtures/git_log.txt");
        assert!(count_by_type(text, MatchType::Git) >= 5);
    }

    #[test]
    fn commands_fixture_has_commands() {
        // Original asserts >=5 with flag/comment/extension-anchored also
        // enabled; those passes aren't ported (Phase 5), so this checks
        // the prompt+exec-anchored core still finds a healthy share.
        let text = include_str!("../../tests/fixtures/commands.txt");
        assert!(count_by_type(text, MatchType::Command) >= 3);
    }

    #[test]
    fn secrets_fixture_has_secrets() {
        let text = include_str!("../../tests/fixtures/secrets.txt");
        let matches = extract(text);
        let secrets: Vec<_> = matches
            .iter()
            .filter(|m| m.ty == MatchType::Secret)
            .collect();
        assert!(secrets.len() >= 7, "got {} secrets", secrets.len());
        let formats: std::collections::HashSet<&str> = secrets
            .iter()
            .filter_map(|m| m.fields.get("secret_format").map(|s| s.as_str()))
            .collect();
        for required in ["jwt", "aws", "github", "gitlab", "stripe", "bearer"] {
            assert!(formats.contains(required), "missing format: {required}");
        }
    }

    #[test]
    fn realworld_fixture_finds_diverse_types() {
        let text = include_str!("../../tests/fixtures/realworld.txt");
        let matches = extract(text);
        let types: HashSet<MatchType> = matches.iter().map(|m| m.ty).collect();
        assert!(types.len() >= 5, "got types: {types:?}");
    }

    #[test]
    fn adversarial_fixture_rejects_near_misses() {
        let text = include_str!("../../tests/fixtures/adversarial.txt");
        let matches = extract(text);
        // No SHA from "12345678" (pure-numeric).
        assert!(!matches
            .iter()
            .any(|m| m.ty == MatchType::Sha && m.raw == "12345678"));
        // No IPv4 from "999.1.1.1".
        assert!(!matches
            .iter()
            .any(|m| m.ty == MatchType::Ipv4 && m.raw.starts_with("999.")));
    }

    #[test]
    fn stress_fixture_dense_mixed_corpus() {
        // 260+ line realistic transcript — exercises the "many matches
        // across many types" path.
        let text = include_str!("../../tests/fixtures/stress.txt");
        let matches = extract(text);
        let types: HashSet<MatchType> = matches.iter().map(|m| m.ty).collect();

        assert!(
            types.len() >= 7,
            "stress fixture should exercise >=7 types, got {} ({:?})",
            types.len(),
            types
        );
        assert!(
            matches.len() >= 40,
            "stress fixture should yield >=40 matches, got {}",
            matches.len()
        );
        for required in [
            MatchType::Url,
            MatchType::File,
            MatchType::Command,
            MatchType::Sha,
            MatchType::Secret,
        ] {
            assert!(
                matches.iter().any(|m| m.ty == required),
                "stress fixture missing required type {required:?}"
            );
        }
    }

    fn cp(name: &str, regex: &str, template: Option<&str>) -> crate::config::CustomPattern {
        crate::config::CustomPattern {
            name: name.to_string(),
            regex: regex.to_string(),
            ty: "url".to_string(),
            template: template.map(str::to_string),
        }
    }

    fn extract_custom_only(text: &str, patterns: &[crate::config::CustomPattern]) -> Vec<Match> {
        let config = crate::config::Config {
            custom: patterns.to_vec(),
            ..crate::config::Config::default()
        };
        // Skip all built-ins - these fixtures are about custom-pattern
        // extraction specifically, and their scrollback also trips
        // several built-in patterns (ipv4, sha, ...) that would just be
        // noise for what's being asserted here.
        custom::extract(text, &config.custom, &config.disabled)
    }

    #[test]
    fn custom_patterns_fixture_port_and_jira() {
        let text = include_str!("../../tests/fixtures/custom_patterns.txt");
        let patterns = vec![
            cp("port", r":[0-9]{4,5}\b", None),
            cp(
                "jira",
                r"[A-Z]+-[0-9]+",
                Some("https://jira.example.com/browse/{match}"),
            ),
        ];
        let matches = extract_custom_only(text, &patterns);

        let ports: Vec<_> = matches.iter().filter(|m| m.raw.starts_with(':')).collect();
        assert!(
            ports.len() >= 4,
            "expected >=4 port matches, got {}: {:?}",
            ports.len(),
            ports.iter().map(|m| &m.raw).collect::<Vec<_>>()
        );

        let jira_by_display: Vec<_> = matches
            .iter()
            .filter(|m| m.display.contains("jira.example.com"))
            .collect();
        assert!(
            jira_by_display.len() >= 4,
            "expected >=4 jira matches with template applied, got {}: {:?}",
            jira_by_display.len(),
            jira_by_display
                .iter()
                .map(|m| &m.display)
                .collect::<Vec<_>>()
        );

        let proj123 = matches
            .iter()
            .find(|m| m.display == "https://jira.example.com/browse/PROJ-123")
            .unwrap();
        assert_eq!(proj123.raw, "https://jira.example.com/browse/PROJ-123");
        assert_eq!(
            proj123.fields.get("url").unwrap(),
            "https://jira.example.com/browse/PROJ-123"
        );
    }

    #[test]
    fn multi_group_patterns_fixture() {
        let text = include_str!("../../tests/fixtures/multi_group_patterns.txt");
        let patterns = vec![
            cp("port", r":[0-9]{4,5}\b", None),
            cp(
                "jira",
                r"([A-Z]+)-([0-9]+[A-Z]*)",
                Some("https://jira.example.com/browse/{1}-{2}"),
            ),
            cp(
                "github-pr",
                r"github\.com/([^/\s]+)/([^/\s]+)/pull/([0-9]+)",
                Some("https://github.com/{1}/{2}/pull/{3}"),
            ),
        ];
        let matches = extract_custom_only(text, &patterns);

        let ports: Vec<_> = matches
            .iter()
            .filter(|m| m.effective_tag() == "port")
            .collect();
        assert!(
            ports.len() >= 3,
            "expected >=3 port matches, got {}",
            ports.len()
        );

        let jira: Vec<_> = matches
            .iter()
            .filter(|m| m.effective_tag() == "jira")
            .collect();
        assert!(
            jira.len() >= 4,
            "expected >=4 jira matches, got {}",
            jira.len()
        );

        let st = jira
            .iter()
            .find(|m| m.fields.get("2").map(String::as_str) == Some("154R"))
            .expect("ST-154R not found");
        assert_eq!(st.fields.get("1").unwrap(), "ST");
        assert_eq!(st.fields.get("2").unwrap(), "154R");
        assert_eq!(st.display, "https://jira.example.com/browse/ST-154R");

        let prs: Vec<_> = matches
            .iter()
            .filter(|m| m.effective_tag() == "github-pr")
            .collect();
        assert!(prs.len() >= 3, "expected >=3 PR matches, got {}", prs.len());

        let pr99 = prs
            .iter()
            .find(|m| m.fields.get("3").map(String::as_str) == Some("99"))
            .expect("PR #99 not found");
        assert_eq!(pr99.fields.get("1").unwrap(), "myorg");
        assert_eq!(pr99.fields.get("2").unwrap(), "myrepo");
        assert_eq!(pr99.display, "https://github.com/myorg/myrepo/pull/99");

        let pr7 = prs
            .iter()
            .find(|m| m.fields.get("1").map(String::as_str) == Some("otherorg"))
            .expect("cross-org PR not found");
        assert_eq!(pr7.display, "https://github.com/otherorg/otherrepo/pull/7");
    }
}
