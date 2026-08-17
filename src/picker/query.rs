//! Inline `#type` filter parsing for the picker query, ported from the
//! original zellij-zextract plugin's `crates/zextract/src/query.rs`
//! verbatim — pure and host-agnostic, no changes needed for the port.
//!
//! The picker's query text mixes two concerns:
//!   - **Filter tokens** like `#url`, `#!secret`, `#ur` (prefix match),
//!     or `##main` (escape — literal `#main` text). They restrict
//!     which matches are visible.
//!   - **Fuzzy tokens** — everything else. Passed to nucleo for
//!     scoring against each remaining match.
//!
//! `parse_query` is pure: it takes the raw query text and a slice of
//! known tags and returns a `ParsedQuery` with the three buckets
//! resolved. The known-tag set is derived from `matcher::TYPE_PRIORITY`.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedQuery {
    /// Tag names to **include** (only matches with one of these types
    /// pass). Empty = no include constraint, all types pass.
    pub includes: Vec<String>,
    /// Tag names to **exclude** (matches with one of these types are
    /// dropped). Applied after includes.
    pub excludes: Vec<String>,
    /// The fuzzy-search text — non-filter tokens joined by spaces,
    /// passed to nucleo as the needle. Empty = passthrough (all
    /// post-filter matches kept).
    pub fuzzy: String,
}

/// Parse `text` into filter buckets + fuzzy text, resolving `#…`
/// tokens against `known_tags` by exact-or-unique-prefix match.
///
/// Token forms recognized:
///   - `#X`       include filter, prefix-resolved
///   - `#!X`      exclude filter, prefix-resolved
///   - `##X`      escape: emit literal `#X` as fuzzy text
///   - anything else → fuzzy text
///
/// Ambiguous prefix (multiple tags match) or unknown prefix (no tag
/// matches) → falls back to fuzzy (the literal `#…` text).
pub fn parse_query(text: &str, known_tags: &[&str]) -> ParsedQuery {
    let mut out = ParsedQuery::default();
    let mut fuzzy_parts: Vec<&str> = Vec::new();

    for token in text.split_whitespace() {
        // `##X` → literal `#X` as fuzzy
        if let Some(rest) = token.strip_prefix("##") {
            out.fuzzy
                .push_str(if out.fuzzy.is_empty() { "" } else { " " });
            out.fuzzy.push('#');
            out.fuzzy.push_str(rest);
            continue;
        }
        // `#!X` → exclude
        if let Some(name) = token.strip_prefix("#!") {
            match resolve_tag(name, known_tags) {
                Some(tag) => out.excludes.push(tag.to_string()),
                None => fuzzy_parts.push(token), // ambiguous/unknown — literal
            }
            continue;
        }
        // `#X` → include
        if let Some(name) = token.strip_prefix('#') {
            if name.is_empty() {
                fuzzy_parts.push(token); // bare `#` — literal
                continue;
            }
            match resolve_tag(name, known_tags) {
                Some(tag) => out.includes.push(tag.to_string()),
                None => fuzzy_parts.push(token), // ambiguous/unknown — literal
            }
            continue;
        }
        // plain fuzzy token
        fuzzy_parts.push(token);
    }

    if !fuzzy_parts.is_empty() {
        if !out.fuzzy.is_empty() {
            out.fuzzy.push(' ');
        }
        out.fuzzy.push_str(&fuzzy_parts.join(" "));
    }
    out
}

/// Resolve a typed prefix against the known-tag set. Returns the tag
/// when exactly one tag starts with `prefix` (case-insensitive), or
/// when `prefix` is itself an exact tag match.
fn resolve_tag<'a>(prefix: &str, known_tags: &'a [&'a str]) -> Option<&'a str> {
    if let Some(t) = known_tags.iter().find(|t| eq_ic(t, prefix)) {
        return Some(*t);
    }
    let mut candidates = known_tags.iter().filter(|t| starts_with_ic(t, prefix));
    let first = candidates.next()?;
    if candidates.next().is_some() {
        None // ambiguous — multiple tags share this prefix
    } else {
        Some(*first)
    }
}

fn eq_ic(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(x, y)| x.eq_ignore_ascii_case(&y))
}

fn starts_with_ic(haystack: &str, prefix: &str) -> bool {
    haystack.len() >= prefix.len()
        && haystack
            .bytes()
            .zip(prefix.bytes())
            .all(|(h, p)| h.eq_ignore_ascii_case(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1_TAGS: &[&str] = &[
        "url", "file", "diag", "sha", "ipv4", "ipv6", "uuid", "quote", "cmd", "secret",
    ];

    fn parse(text: &str) -> ParsedQuery {
        parse_query(text, V1_TAGS)
    }

    #[test]
    fn include_exact_tag() {
        let p = parse("#url");
        assert_eq!(p.includes, vec!["url"]);
        assert!(p.excludes.is_empty());
        assert!(p.fuzzy.is_empty());
    }

    #[test]
    fn exclude_exact_tag() {
        let p = parse("#!secret config");
        assert_eq!(p.excludes, vec!["secret"]);
        assert!(p.includes.is_empty());
        assert_eq!(p.fuzzy, "config");
    }

    #[test]
    fn prefix_unique_resolves() {
        let p = parse("#ur install");
        assert_eq!(p.includes, vec!["url"]);
        assert_eq!(p.fuzzy, "install");
    }

    #[test]
    fn prefix_single_letter_unique() {
        let p = parse("#f");
        assert_eq!(p.includes, vec!["file"]);
    }

    #[test]
    fn prefix_uu_unique_to_uuid() {
        let p = parse("#uu");
        assert_eq!(p.includes, vec!["uuid"]);
    }

    #[test]
    fn prefix_exclude_form_works() {
        let p = parse("#!se find me");
        assert_eq!(p.excludes, vec!["secret"]);
        assert_eq!(p.fuzzy, "find me");
    }

    #[test]
    fn ambiguous_prefix_becomes_literal_fuzzy() {
        let p = parse("#u install");
        assert!(p.includes.is_empty());
        assert!(p.excludes.is_empty());
        assert_eq!(p.fuzzy, "#u install");
    }

    #[test]
    fn ambiguous_s_prefix() {
        let p = parse("#s");
        assert!(p.includes.is_empty());
        assert_eq!(p.fuzzy, "#s");
    }

    #[test]
    fn ambiguous_i_prefix_for_ipv4_ipv6() {
        let p = parse("#i");
        assert!(p.includes.is_empty());
        assert_eq!(p.fuzzy, "#i");
    }

    #[test]
    fn ipv4_full_disambiguates() {
        let p = parse("#ipv4");
        assert_eq!(p.includes, vec!["ipv4"]);
    }

    #[test]
    fn unknown_type_is_literal_fuzzy() {
        let p = parse("#main-content");
        assert!(p.includes.is_empty());
        assert_eq!(p.fuzzy, "#main-content");
    }

    #[test]
    fn escape_double_hash() {
        let p = parse("##main install");
        assert!(p.includes.is_empty());
        assert_eq!(p.fuzzy, "#main install");
    }

    #[test]
    fn bare_hash_is_literal() {
        let p = parse("# alone");
        assert_eq!(p.fuzzy, "# alone");
    }

    #[test]
    fn empty_query() {
        let p = parse("");
        assert!(p.includes.is_empty());
        assert!(p.excludes.is_empty());
        assert!(p.fuzzy.is_empty());
    }

    #[test]
    fn multiple_includes() {
        let p = parse("#url #file brew");
        assert_eq!(p.includes, vec!["url", "file"]);
        assert_eq!(p.fuzzy, "brew");
    }

    #[test]
    fn include_plus_exclude() {
        let p = parse("#file #!secret config");
        assert_eq!(p.includes, vec!["file"]);
        assert_eq!(p.excludes, vec!["secret"]);
        assert_eq!(p.fuzzy, "config");
    }

    #[test]
    fn token_order_independent() {
        let a = parse("#url install");
        let b = parse("install #url");
        assert_eq!(a.includes, b.includes);
        assert_eq!(a.fuzzy, b.fuzzy);
    }

    #[test]
    fn case_insensitive_prefix() {
        let p = parse("#URL install");
        assert_eq!(p.includes, vec!["url"]);
        let p2 = parse("#Ur install");
        assert_eq!(p2.includes, vec!["url"]);
    }

    #[test]
    fn caller_supplied_tag_set() {
        let tags: &[&str] = &["url", "file", "jira"];
        let p = parse_query("#j PROJ-123", tags);
        assert_eq!(p.includes, vec!["jira"]);
        assert_eq!(p.fuzzy, "PROJ-123");
    }

    #[test]
    fn caller_supplied_tag_set_handles_prefix_collisions_with_custom_types() {
        let tags: &[&str] = &["url", "file", "urgent"];
        let p = parse_query("#ur install", tags);
        assert!(p.includes.is_empty(), "got {:?}", p.includes);
        assert_eq!(p.fuzzy, "#ur install");
    }
}
