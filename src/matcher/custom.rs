//! User-defined regex patterns, ported from the original plugin's
//! `extract_single_custom`/`extract_custom` (in `extract.rs`) and its
//! `{match}`/`{0}`/`{1}`... template substitution.

use std::collections::HashMap;

use regex_lite::Regex;

use super::{Match, MatchType};
use crate::config::CustomPattern;

/// Run every pattern in `patterns` against `text`, skipping any whose
/// `name` is in `disabled` and any with an invalid regex (silently, per
/// the original's contract - a typo in one custom pattern shouldn't
/// break the others or crash the plugin).
pub fn extract(
    text: &str,
    patterns: &[CustomPattern],
    disabled: &std::collections::HashSet<String>,
) -> Vec<Match> {
    let mut out = Vec::new();
    for cp in patterns {
        if disabled.contains(&cp.name) {
            continue;
        }
        let Ok(re) = Regex::new(&cp.regex) else {
            continue;
        };
        let ty = MatchType::from_tag(&cp.ty).unwrap_or(MatchType::Url);
        let mut byte_offset_of_line = 0usize;
        for line in text.lines() {
            for caps in re.captures_iter(line) {
                let full = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect();
                // {match} = group 1 if the regex has groups, else the full match.
                let match_val = groups
                    .first()
                    .and_then(|g| g.clone())
                    .unwrap_or_else(|| full.as_str().to_string());

                let (raw, display) = match &cp.template {
                    Some(tmpl) => {
                        let expanded = expand_template(tmpl, full.as_str(), &match_val, &groups);
                        (expanded.clone(), expanded)
                    }
                    None => (match_val.clone(), match_val.clone()),
                };

                let mut fields = HashMap::new();
                fields.insert("match".to_string(), match_val);
                fields.insert("0".to_string(), full.as_str().to_string());
                for (i, g) in groups.iter().enumerate() {
                    fields.insert((i + 1).to_string(), g.clone().unwrap_or_default());
                }
                fields.insert("__label".to_string(), cp.name.clone());
                // Feeds {url}/{file} in a future action-template pass;
                // also what the multi_group_patterns fixture asserts on.
                match ty {
                    MatchType::Url => {
                        fields.insert("url".to_string(), display.clone());
                    }
                    MatchType::File => {
                        fields.insert("file".to_string(), display.clone());
                    }
                    _ => {}
                }

                // Span covers group 1 when the regex has one (more
                // precise than the whole match), else the full match.
                let (span_start, span_end) = match caps.get(1) {
                    Some(g) => (
                        byte_offset_of_line + g.start(),
                        byte_offset_of_line + g.end(),
                    ),
                    None => (
                        byte_offset_of_line + full.start(),
                        byte_offset_of_line + full.end(),
                    ),
                };
                out.push(Match {
                    ty,
                    raw,
                    display,
                    context: line.to_string(),
                    span: (span_start, span_end),
                    fields,
                });
            }
            byte_offset_of_line += line.len() + 1;
        }
    }
    out
}

/// Expand `{match}`, `{0}`, `{1}`, ... placeholders in `template`.
/// Unknown `{name}` tokens are left literal.
fn expand_template(
    template: &str,
    full: &str,
    match_val: &str,
    groups: &[Option<String>],
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            out.push(c);
            continue;
        }
        let mut key = String::new();
        let mut closed = false;
        while let Some(&nc) = chars.peek() {
            chars.next();
            if nc == '}' {
                closed = true;
                break;
            }
            key.push(nc);
        }
        if !closed {
            out.push('{');
            out.push_str(&key);
            continue;
        }
        match key.as_str() {
            "match" => out.push_str(match_val),
            "0" => out.push_str(full),
            k => match k.parse::<usize>() {
                Ok(idx) if idx >= 1 => {
                    if let Some(Some(g)) = groups.get(idx - 1) {
                        out.push_str(g);
                    }
                }
                _ => {
                    out.push('{');
                    out.push_str(k);
                    out.push('}');
                }
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn cp(name: &str, regex: &str, ty: &str, template: Option<&str>) -> CustomPattern {
        CustomPattern {
            name: name.to_string(),
            regex: regex.to_string(),
            ty: ty.to_string(),
            template: template.map(str::to_string),
        }
    }

    #[test]
    fn no_groups_no_template() {
        let patterns = vec![cp("port", r":[0-9]{4,5}\b", "url", None)];
        let m = extract("server on :3000 now", &patterns, &HashSet::new());
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].raw, ":3000");
        assert_eq!(m[0].fields.get("__label").unwrap(), "port");
    }

    #[test]
    fn single_group_with_template() {
        let patterns = vec![cp(
            "jira-ticket",
            "New Jira ticket : ([A-Z]+-[0-9]+[A-Z]*)",
            "url",
            Some("https://jira.example.com/browse/{match}"),
        )];
        let m = extract(
            "New Jira ticket : ST-154R assigned",
            &patterns,
            &HashSet::new(),
        );
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].raw, "https://jira.example.com/browse/ST-154R");
        assert_eq!(m[0].display, "https://jira.example.com/browse/ST-154R");
    }

    #[test]
    fn multi_group_template() {
        let patterns = vec![cp(
            "jira",
            "([A-Z]+)-([0-9]+)",
            "url",
            Some("https://jira.example.com/browse/{1}-{2}"),
        )];
        let m = extract("ticket BACKEND-42 blocked", &patterns, &HashSet::new());
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].raw, "https://jira.example.com/browse/BACKEND-42");
        assert_eq!(m[0].fields["1"], "BACKEND");
        assert_eq!(m[0].fields["2"], "42");
    }

    #[test]
    fn disabled_pattern_skipped() {
        let patterns = vec![cp("port", r":[0-9]{4,5}\b", "url", None)];
        let mut disabled = HashSet::new();
        disabled.insert("port".to_string());
        let m = extract("server on :3000 now", &patterns, &disabled);
        assert!(m.is_empty());
    }

    #[test]
    fn invalid_regex_silently_skipped() {
        let patterns = vec![cp("broken", "(unclosed", "url", None)];
        let m = extract("anything", &patterns, &HashSet::new());
        assert!(m.is_empty());
    }

    #[test]
    fn assigns_configured_type() {
        let patterns = vec![cp("jira", "([A-Z]+)-([0-9]+)", "file", None)];
        let m = extract("BACKEND-42", &patterns, &HashSet::new());
        assert_eq!(m[0].ty, MatchType::File);
    }

    #[test]
    fn unknown_type_falls_back_to_url() {
        let patterns = vec![cp("jira", "([A-Z]+)-([0-9]+)", "nonsense", None)];
        let m = extract("BACKEND-42", &patterns, &HashSet::new());
        assert_eq!(m[0].ty, MatchType::Url);
    }
}
