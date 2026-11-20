//! Rules engine: compiles mod-defined rules and checks chat messages against them.

use crate::irc::ChatMessage;
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleKind {
    /// Plain words or phrases, one per pattern.
    Keyword,
    /// Regular expressions, one per pattern.
    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub kind: RuleKind,
    pub patterns: Vec<String>,
    pub whole_word: bool,
    pub case_sensitive: bool,
    /// Channels this rule applies to (lowercase logins). Empty = every channel.
    pub channels: Vec<String>,
    pub severity: Severity,
    /// Skip messages from mods, VIPs and the broadcaster.
    pub exempt_privileged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleMatch {
    pub rule_id: i64,
    pub rule_name: String,
    pub severity: Severity,
    /// The distinct pieces of text that triggered the rule.
    pub terms: Vec<String>,
}

enum Matcher {
    Keywords { ac: AhoCorasick, patterns: Vec<String>, case_sensitive: bool, whole_word: bool },
    Regex(Regex),
}

struct CompiledRule {
    rule: Rule,
    matcher: Matcher,
}

#[derive(Default)]
pub struct RuleEngine {
    rules: Vec<CompiledRule>,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn at_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before_ok = text[..start].chars().next_back().is_none_or(|c| !is_word_char(c));
    let after_ok = text[end..].chars().next().is_none_or(|c| !is_word_char(c));
    before_ok && after_ok
}

fn clean_patterns(rule: &Rule) -> Vec<String> {
    rule.patterns
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Check a rule definition and build its matcher. Errors are human-readable.
fn compile(rule: &Rule) -> Result<Matcher, String> {
    let patterns = clean_patterns(rule);
    if patterns.is_empty() {
        return Err("Add at least one word, phrase or pattern.".into());
    }
    match rule.kind {
        RuleKind::Keyword => {
            let patterns: Vec<String> = if rule.case_sensitive {
                patterns
            } else {
                patterns.iter().map(|p| p.to_lowercase()).collect()
            };
            let ac = AhoCorasickBuilder::new()
                .match_kind(MatchKind::Standard)
                .build(&patterns)
                .map_err(|e| e.to_string())?;
            Ok(Matcher::Keywords {
                ac,
                patterns,
                case_sensitive: rule.case_sensitive,
                whole_word: rule.whole_word,
            })
        }
        RuleKind::Regex => {
            for p in &patterns {
                Regex::new(p).map_err(|e| format!("Invalid pattern `{p}`: {e}"))?;
            }
            let joined = patterns.iter().map(|p| format!("(?:{p})")).collect::<Vec<_>>().join("|");
            let source = if rule.whole_word { format!(r"\b(?:{joined})\b") } else { joined };
            let re = RegexBuilder::new(&source)
                .case_insensitive(!rule.case_sensitive)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Matcher::Regex(re))
        }
    }
}

pub fn validate(rule: &Rule) -> Result<(), String> {
    if rule.name.trim().is_empty() {
        return Err("Give the rule a name.".into());
    }
    compile(rule).map(|_| ())
}

impl RuleEngine {
    /// Build an engine from saved rules. Disabled or invalid rules are skipped.
    pub fn new(rules: &[Rule]) -> Self {
        let rules = rules
            .iter()
            .filter(|r| r.enabled)
            .filter_map(|r| compile(r).ok().map(|matcher| CompiledRule { rule: r.clone(), matcher }))
            .collect();
        RuleEngine { rules }
    }

    pub fn evaluate(&self, msg: &ChatMessage) -> Vec<RuleMatch> {
        let privileged = msg.is_mod || msg.is_broadcaster || msg.is_vip;
        let mut lowered: Option<String> = None;
        let mut out = Vec::new();

        for cr in &self.rules {
            let rule = &cr.rule;
            if rule.exempt_privileged && privileged {
                continue;
            }
            if !rule.channels.is_empty() && !rule.channels.iter().any(|c| c.eq_ignore_ascii_case(&msg.channel)) {
                continue;
            }

            let mut terms: Vec<String> = Vec::new();
            match &cr.matcher {
                Matcher::Keywords { ac, patterns, case_sensitive, whole_word } => {
                    let haystack: &str = if *case_sensitive {
                        &msg.text
                    } else {
                        lowered.get_or_insert_with(|| msg.text.to_lowercase())
                    };
                    for m in ac.find_overlapping_iter(haystack) {
                        if *whole_word && !at_word_boundaries(haystack, m.start(), m.end()) {
                            continue;
                        }
                        let term = &patterns[m.pattern().as_usize()];
                        if !terms.contains(term) {
                            terms.push(term.clone());
                        }
                    }
                }
                Matcher::Regex(re) => {
                    for m in re.find_iter(&msg.text) {
                        let term = m.as_str().to_string();
                        if !term.is_empty() && !terms.contains(&term) {
                            terms.push(term);
                        }
                    }
                }
            }

            if !terms.is_empty() {
                out.push(RuleMatch {
                    rule_id: rule.id,
                    rule_name: rule.name.clone(),
                    severity: rule.severity,
                    terms,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kind: RuleKind, patterns: &[&str]) -> Rule {
        Rule {
            id: 1,
            name: "test".into(),
            enabled: true,
            kind,
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
            whole_word: true,
            case_sensitive: false,
            channels: vec![],
            severity: Severity::Medium,
            exempt_privileged: false,
        }
    }

    fn msg(channel: &str, text: &str) -> ChatMessage {
        ChatMessage {
            id: "1".into(),
            channel: channel.into(),
            user_login: "viewer".into(),
            display_name: "Viewer".into(),
            color: None,
            text: text.into(),
            is_action: false,
            is_mod: false,
            is_broadcaster: false,
            is_vip: false,
            is_subscriber: false,
            is_first_msg: false,
            timestamp: 0,
        }
    }

    #[test]
    fn keyword_is_case_insensitive_and_whole_word() {
        let engine = RuleEngine::new(&[rule(RuleKind::Keyword, &["scam", "free followers"])]);
        let hits = engine.evaluate(&msg("a", "Get FREE Followers now, totally not a SCAM"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].terms, vec!["free followers", "scam"]);
        assert!(engine.evaluate(&msg("a", "scampi for dinner")).is_empty());
    }

    #[test]
    fn whole_word_prefers_longer_overlapping_term() {
        let engine = RuleEngine::new(&[rule(RuleKind::Keyword, &["bad", "badword"])]);
        let hits = engine.evaluate(&msg("a", "that is a badword"));
        assert_eq!(hits[0].terms, vec!["badword"]);
    }

    #[test]
    fn substring_matching_when_whole_word_off() {
        let mut r = rule(RuleKind::Keyword, &["scam"]);
        r.whole_word = false;
        let engine = RuleEngine::new(&[r]);
        assert_eq!(engine.evaluate(&msg("a", "scampi")).len(), 1);
    }

    #[test]
    fn case_sensitive_keyword() {
        let mut r = rule(RuleKind::Keyword, &["NO"]);
        r.case_sensitive = true;
        let engine = RuleEngine::new(&[r]);
        assert!(engine.evaluate(&msg("a", "no way")).is_empty());
        assert_eq!(engine.evaluate(&msg("a", "NO way")).len(), 1);
    }

    #[test]
    fn regex_rules_match_and_validate() {
        let engine = RuleEngine::new(&[rule(RuleKind::Regex, &[r"bit\.ly/\w+"])]);
        let hits = engine.evaluate(&msg("a", "check bit.ly/abc123 now"));
        assert_eq!(hits[0].terms, vec!["bit.ly/abc123"]);
        assert!(validate(&rule(RuleKind::Regex, &["(unclosed"])).is_err());
        assert!(validate(&rule(RuleKind::Keyword, &["  "])).is_err());
    }

    #[test]
    fn channel_scope_and_exemptions() {
        let mut r = rule(RuleKind::Keyword, &["spoiler"]);
        r.channels = vec!["gamer".into()];
        r.exempt_privileged = true;
        let engine = RuleEngine::new(&[r]);
        assert_eq!(engine.evaluate(&msg("gamer", "spoiler alert")).len(), 1);
        assert!(engine.evaluate(&msg("other", "spoiler alert")).is_empty());
        let mut from_mod = msg("gamer", "spoiler alert");
        from_mod.is_mod = true;
        assert!(engine.evaluate(&from_mod).is_empty());
    }

    #[test]
    fn disabled_rules_are_ignored() {
        let mut r = rule(RuleKind::Keyword, &["x"]);
        r.enabled = false;
        assert!(RuleEngine::new(&[r]).evaluate(&msg("a", "x")).is_empty());
    }
}
