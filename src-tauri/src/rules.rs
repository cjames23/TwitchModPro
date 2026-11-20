//! Rules engine: compiles mod-defined rules and checks chat messages against them.

use crate::irc::ChatMessage;
use crate::normalize::{normalize, normalize_pattern, Normalized};
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    /// Plain words or phrases, one per pattern.
    Keyword,
    /// Regular expressions, one per pattern.
    Regex,
    /// Any link. Patterns are allowed domains.
    Links,
    /// Mostly-uppercase messages.
    Caps,
    /// Long runs of one character, too many emotes, or ASCII art.
    Spam,
    /// The same user sending the same message repeatedly.
    Repeat,
    /// A user's first message in the channel.
    FirstMessage,
    /// Accounts younger than `params.max_account_age_days` (looked up from Twitch).
    NewAccount,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    #[default]
    Message,
    Username,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActiveDuring {
    #[default]
    Always,
    /// Only while the channel is in raid mode.
    Raid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RuleParams {
    /// Caps: ignore messages with fewer letters than this.
    pub min_length: usize,
    /// Caps: flag when at least this percent of letters are uppercase.
    pub caps_percent: u8,
    /// Spam: flag a run of the same character at least this long.
    pub max_repeated_chars: usize,
    /// Spam: flag messages with more emotes than this.
    pub max_emotes: usize,
    /// Repeat: flag on the Nth identical message...
    pub repeat_count: usize,
    /// ...within this many seconds.
    pub repeat_window_secs: u64,
    /// NewAccount: flag accounts created less than this many days ago.
    pub max_account_age_days: u32,
}

impl Default for RuleParams {
    fn default() -> Self {
        RuleParams {
            min_length: 12,
            caps_percent: 75,
            max_repeated_chars: 12,
            max_emotes: 12,
            repeat_count: 3,
            repeat_window_secs: 60,
            max_account_age_days: 7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub kind: RuleKind,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    /// Channels this rule applies to (lowercase logins). Empty = every channel.
    #[serde(default)]
    pub channels: Vec<String>,
    pub severity: Severity,
    /// Skip messages from mods, VIPs and the broadcaster.
    #[serde(default)]
    pub exempt_privileged: bool,
    /// Keyword/regex rules: match against the message or the sender's username.
    #[serde(default)]
    pub target: Target,
    /// Keyword/regex rules: undo leetspeak, lookalike letters, accents and hidden characters first.
    #[serde(default)]
    pub normalize: bool,
    #[serde(default)]
    pub active_during: ActiveDuring,
    #[serde(default)]
    pub params: RuleParams,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleMatch {
    pub rule_id: i64,
    pub rule_name: String,
    pub severity: Severity,
    /// Pieces of the original message that triggered the rule (for highlighting).
    pub terms: Vec<String>,
    /// Why a built-in check fired, e.g. "92% caps".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

enum Matcher {
    Keywords { ac: AhoCorasick, patterns: Vec<String> },
    Regex(Regex),
    Links { allow: Vec<String> },
    Builtin,
}

struct CompiledRule {
    rule: Rule,
    matcher: Matcher,
}

/// Recent messages per (channel, user), for repeat detection.
#[derive(Default)]
struct RepeatTracker {
    recent: HashMap<(String, String), VecDeque<(i64, String)>>,
    max_window_ms: i64,
}

impl RepeatTracker {
    /// Record the message and return how many identical ones the user sent within `window_ms` (including this one).
    fn record(&mut self, msg: &ChatMessage, key_text: &str) {
        let entry = self.recent.entry((msg.channel.clone(), msg.user_login.clone())).or_default();
        entry.push_back((msg.timestamp, key_text.to_string()));
        while entry.front().is_some_and(|(t, _)| msg.timestamp - t > self.max_window_ms) || entry.len() > 50 {
            entry.pop_front();
        }
        if self.recent.len() > 20_000 {
            let cutoff = msg.timestamp - self.max_window_ms;
            self.recent.retain(|_, q| q.back().is_some_and(|(t, _)| *t >= cutoff));
        }
    }

    fn count(&self, msg: &ChatMessage, key_text: &str, window_ms: i64) -> usize {
        self.recent
            .get(&(msg.channel.clone(), msg.user_login.clone()))
            .map(|q| {
                q.iter()
                    .filter(|(t, text)| msg.timestamp - t <= window_ms && text == key_text)
                    .count()
            })
            .unwrap_or(0)
    }
}

pub struct RuleEngine {
    rules: Vec<CompiledRule>,
    repeats: Mutex<RepeatTracker>,
    has_repeat_rules: bool,
}

impl Default for RuleEngine {
    fn default() -> Self {
        RuleEngine::new(&[])
    }
}

static LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \bhttps?://[^\s]+
        | \b(?:[a-z0-9-]+\.)+(?:com|net|org|gg|tv|io|ly|co|me|xyz|ru|info|biz|link|site|online|shop|app|dev|live|stream|fun|club|top|to|cc|us|uk|de|store|pro|icu|vip)\b(?:/[^\s]*)?
        | \b[a-z0-9-]+\s*[\(\[]?\s*dot\s*[\)\]]?\s*(?:com|net|gg|ru|tv|xyz)\b",
    )
    .unwrap()
});

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
    match rule.kind {
        RuleKind::Keyword => {
            if patterns.is_empty() {
                return Err("Add at least one word or phrase.".into());
            }
            let patterns: Vec<String> = if rule.normalize {
                patterns.iter().map(|p| normalize_pattern(p)).filter(|p| !p.is_empty()).collect()
            } else if rule.case_sensitive {
                patterns
            } else {
                patterns.iter().map(|p| p.to_lowercase()).collect()
            };
            let ac = AhoCorasickBuilder::new()
                .match_kind(MatchKind::Standard)
                .build(&patterns)
                .map_err(|e| e.to_string())?;
            Ok(Matcher::Keywords { ac, patterns })
        }
        RuleKind::Regex => {
            if patterns.is_empty() {
                return Err("Add at least one pattern.".into());
            }
            for p in &patterns {
                Regex::new(p).map_err(|e| format!("Invalid pattern `{p}`: {e}"))?;
            }
            let joined = patterns.iter().map(|p| format!("(?:{p})")).collect::<Vec<_>>().join("|");
            let source = if rule.whole_word { format!(r"\b(?:{joined})\b") } else { joined };
            let re = RegexBuilder::new(&source)
                .case_insensitive(!rule.case_sensitive || rule.normalize)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Matcher::Regex(re))
        }
        RuleKind::Links => Ok(Matcher::Links {
            allow: patterns
                .iter()
                .map(|p| {
                    p.to_lowercase()
                        .trim_start_matches("https://")
                        .trim_start_matches("http://")
                        .trim_start_matches("www.")
                        .trim_end_matches('/')
                        .to_string()
                })
                .collect(),
        }),
        RuleKind::Caps => {
            let p = &rule.params;
            if !(1..=100).contains(&p.caps_percent) {
                return Err("Caps percentage must be between 1 and 100.".into());
            }
            Ok(Matcher::Builtin)
        }
        RuleKind::Spam => {
            if rule.params.max_repeated_chars < 3 {
                return Err("Repeated-character limit must be at least 3.".into());
            }
            Ok(Matcher::Builtin)
        }
        RuleKind::Repeat => {
            let p = &rule.params;
            if p.repeat_count < 2 || p.repeat_window_secs == 0 {
                return Err("Repeat count must be at least 2 and the time window above 0.".into());
            }
            Ok(Matcher::Builtin)
        }
        RuleKind::FirstMessage => Ok(Matcher::Builtin),
        RuleKind::NewAccount => {
            if rule.params.max_account_age_days == 0 {
                return Err("Account age must be at least 1 day.".into());
            }
            Ok(Matcher::Builtin)
        }
    }
}

pub fn validate(rule: &Rule) -> Result<(), String> {
    if rule.name.trim().is_empty() {
        return Err("Give the rule a name.".into());
    }
    compile(rule).map(|_| ())
}

/// Normalized form used to decide whether two messages are "the same".
fn repeat_key(text: &str) -> String {
    normalize(text).text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn link_host(link: &str) -> String {
    let l = link.to_lowercase();
    let l = l.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.");
    l.split(['/', '?', '#']).next().unwrap_or_default().to_string()
}

fn domain_allowed(host: &str, allow: &[String]) -> bool {
    allow.iter().any(|a| host == a || host.ends_with(&format!(".{a}")))
}

fn longest_char_run(text: &str) -> Option<(char, usize)> {
    let mut best: Option<(char, usize)> = None;
    let mut cur: Option<(char, usize)> = None;
    for c in text.chars() {
        cur = match cur {
            Some((p, n)) if p == c => Some((p, n + 1)),
            _ => Some((c, 1)),
        };
        if let Some((ch, n)) = cur {
            if !ch.is_whitespace() && best.is_none_or(|(_, b)| n > b) {
                best = Some((ch, n));
            }
        }
    }
    best
}

fn is_art_char(c: char) -> bool {
    matches!(c, '\u{2800}'..='\u{28FF}' | '\u{2580}'..='\u{259F}' | '\u{2500}'..='\u{257F}')
}

fn push_unique(terms: &mut Vec<String>, t: String) {
    if !t.is_empty() && !terms.contains(&t) {
        terms.push(t);
    }
}

/// Extra facts available when checking a message.
#[derive(Debug, Clone, Copy, Default)]
pub struct EvalCtx {
    pub raid_mode: bool,
    /// When the sender's account was created (ms), if known.
    pub account_created_at: Option<i64>,
}

fn applies(rule: &Rule, msg: &ChatMessage, raid_mode: bool) -> bool {
    let privileged = msg.is_mod || msg.is_broadcaster || msg.is_vip;
    !(rule.exempt_privileged && privileged)
        && !(rule.active_during == ActiveDuring::Raid && !raid_mode)
        && (rule.channels.is_empty() || rule.channels.iter().any(|c| c.eq_ignore_ascii_case(&msg.channel)))
}

impl RuleEngine {
    pub fn kind_of(&self, rule_id: i64) -> Option<RuleKind> {
        self.rules.iter().find(|c| c.rule.id == rule_id).map(|c| c.rule.kind)
    }

    /// Whether an enabled new-account rule applies to this message (so its sender's age is worth looking up).
    pub fn needs_account_age(&self, msg: &ChatMessage, raid_mode: bool) -> bool {
        self.rules.iter().any(|c| c.rule.kind == RuleKind::NewAccount && applies(&c.rule, msg, raid_mode))
    }

    /// Check a message. `raid_mode` is whether its channel is currently in raid mode.
    pub fn evaluate(&self, msg: &ChatMessage, raid_mode: bool) -> Vec<RuleMatch> {
        self.evaluate_with(msg, EvalCtx { raid_mode, account_created_at: None })
    }

    /// Build an engine from saved rules. Disabled or invalid rules are skipped.
    pub fn new(rules: &[Rule]) -> Self {
        let compiled: Vec<CompiledRule> = rules
            .iter()
            .filter(|r| r.enabled)
            .filter_map(|r| compile(r).ok().map(|matcher| CompiledRule { rule: r.clone(), matcher }))
            .collect();
        let repeat_rules: Vec<&Rule> = compiled.iter().map(|c| &c.rule).filter(|r| r.kind == RuleKind::Repeat).collect();
        let max_window_ms = repeat_rules.iter().map(|r| r.params.repeat_window_secs as i64 * 1000).max().unwrap_or(0);
        RuleEngine {
            has_repeat_rules: !repeat_rules.is_empty(),
            repeats: Mutex::new(RepeatTracker { recent: HashMap::new(), max_window_ms }),
            rules: compiled,
        }
    }

    pub fn evaluate_with(&self, msg: &ChatMessage, ctx: EvalCtx) -> Vec<RuleMatch> {
        let raid_mode = ctx.raid_mode;
        let mut lowered: Option<String> = None;
        let mut normalized: Option<Normalized> = None;
        let mut out = Vec::new();

        let repeat_text = self.has_repeat_rules.then(|| repeat_key(&msg.text));
        let mut tracker = self.has_repeat_rules.then(|| self.repeats.lock().unwrap());
        if let (Some(t), Some(key)) = (tracker.as_mut(), repeat_text.as_ref()) {
            t.record(msg, key);
        }

        for cr in &self.rules {
            let rule = &cr.rule;
            if !applies(rule, msg, raid_mode) {
                continue;
            }

            let mut terms: Vec<String> = Vec::new();
            let mut reason: Option<String> = None;
            let by_username = rule.target == Target::Username;

            match &cr.matcher {
                Matcher::Keywords { ac, patterns } => {
                    if by_username {
                        let name = if rule.normalize { normalize(&msg.user_login).text } else { msg.user_login.clone() };
                        for m in ac.find_overlapping_iter(&name) {
                            if rule.whole_word && !at_word_boundaries(&name, m.start(), m.end()) {
                                continue;
                            }
                            push_unique(&mut terms, patterns[m.pattern().as_usize()].clone());
                        }
                        if !terms.is_empty() {
                            reason = Some(format!("Username contains “{}”", terms.join("”, “")));
                            terms.clear();
                        }
                    } else if rule.normalize {
                        let n = normalized.get_or_insert_with(|| normalize(&msg.text));
                        for m in ac.find_overlapping_iter(&n.text) {
                            if rule.whole_word && !at_word_boundaries(&n.text, m.start(), m.end()) {
                                continue;
                            }
                            let (a, b) = n.original_range(m.start(), m.end());
                            push_unique(&mut terms, msg.text[a..b].to_string());
                        }
                    } else {
                        let hay: &str = if rule.case_sensitive {
                            &msg.text
                        } else {
                            lowered.get_or_insert_with(|| msg.text.to_lowercase())
                        };
                        for m in ac.find_overlapping_iter(hay) {
                            if rule.whole_word && !at_word_boundaries(hay, m.start(), m.end()) {
                                continue;
                            }
                            push_unique(&mut terms, patterns[m.pattern().as_usize()].clone());
                        }
                    }
                }
                Matcher::Regex(re) => {
                    if by_username {
                        let name = if rule.normalize { normalize(&msg.user_login).text } else { msg.user_login.clone() };
                        if let Some(m) = re.find(&name) {
                            reason = Some(format!("Username matches “{}”", m.as_str()));
                        }
                    } else if rule.normalize {
                        let n = normalized.get_or_insert_with(|| normalize(&msg.text));
                        for m in re.find_iter(&n.text) {
                            let (a, b) = n.original_range(m.start(), m.end());
                            push_unique(&mut terms, msg.text[a..b].to_string());
                        }
                    } else {
                        for m in re.find_iter(&msg.text) {
                            push_unique(&mut terms, m.as_str().to_string());
                        }
                    }
                }
                Matcher::Links { allow } => {
                    for m in LINK_RE.find_iter(&msg.text) {
                        if !domain_allowed(&link_host(m.as_str()), allow) {
                            push_unique(&mut terms, m.as_str().to_string());
                        }
                    }
                }
                Matcher::Builtin => match rule.kind {
                    RuleKind::Caps => {
                        let letters = msg.text.chars().filter(|c| c.is_alphabetic()).count();
                        let upper = msg.text.chars().filter(|c| c.is_uppercase()).count();
                        if letters >= rule.params.min_length && letters > 0 {
                            let pct = upper * 100 / letters;
                            if pct >= rule.params.caps_percent as usize {
                                reason = Some(format!("{pct}% capital letters"));
                            }
                        }
                    }
                    RuleKind::Spam => {
                        if let Some((c, n)) = longest_char_run(&msg.text) {
                            if n >= rule.params.max_repeated_chars {
                                reason = Some(format!("“{c}” repeated {n} times"));
                                push_unique(&mut terms, c.to_string().repeat(n));
                            }
                        }
                        if reason.is_none() && msg.emote_count > rule.params.max_emotes {
                            reason = Some(format!("{} emotes", msg.emote_count));
                        }
                        let art = msg.text.chars().filter(|c| is_art_char(*c)).count();
                        if reason.is_none() && art >= 20 {
                            reason = Some("ASCII art".into());
                        }
                    }
                    RuleKind::Repeat => {
                        if let (Some(t), Some(key)) = (tracker.as_ref(), repeat_text.as_ref()) {
                            if !key.is_empty() {
                                let n = t.count(msg, key, rule.params.repeat_window_secs as i64 * 1000);
                                if n >= rule.params.repeat_count {
                                    reason = Some(format!("Sent {n} times in {}s", rule.params.repeat_window_secs));
                                }
                            }
                        }
                    }
                    RuleKind::FirstMessage => {
                        if msg.is_first_msg {
                            reason = Some("First message in this channel".into());
                        }
                    }
                    RuleKind::NewAccount => {
                        if let Some(created) = ctx.account_created_at {
                            let days = (msg.timestamp - created).max(0) / 86_400_000;
                            if days < rule.params.max_account_age_days as i64 {
                                reason = Some(match days {
                                    0 => "Account created today".to_string(),
                                    1 => "Account is 1 day old".to_string(),
                                    d => format!("Account is {d} days old"),
                                });
                            }
                        }
                    }
                    _ => {}
                },
            }

            if !terms.is_empty() || reason.is_some() {
                out.push(RuleMatch {
                    rule_id: rule.id,
                    rule_name: rule.name.clone(),
                    severity: rule.severity,
                    terms,
                    reason,
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
            target: Target::Message,
            normalize: false,
            active_during: ActiveDuring::Always,
            params: RuleParams::default(),
        }
    }

    fn msg(channel: &str, text: &str) -> ChatMessage {
        ChatMessage {
            id: "1".into(),
            channel: channel.into(),
            room_id: None,
            user_id: None,
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
            emote_count: 0,
            timestamp: 0,
        }
    }

    fn check(r: Rule, text: &str) -> Vec<RuleMatch> {
        RuleEngine::new(&[r]).evaluate(&msg("a", text), false)
    }

    #[test]
    fn keyword_is_case_insensitive_and_whole_word() {
        let engine = RuleEngine::new(&[rule(RuleKind::Keyword, &["scam", "free followers"])]);
        let hits = engine.evaluate(&msg("a", "Get FREE Followers now, totally not a SCAM"), false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].terms, vec!["free followers", "scam"]);
        assert!(engine.evaluate(&msg("a", "scampi for dinner"), false).is_empty());
    }

    #[test]
    fn whole_word_prefers_longer_overlapping_term() {
        let hits = check(rule(RuleKind::Keyword, &["bad", "badword"]), "that is a badword");
        assert_eq!(hits[0].terms, vec!["badword"]);
    }

    #[test]
    fn substring_matching_when_whole_word_off() {
        let mut r = rule(RuleKind::Keyword, &["scam"]);
        r.whole_word = false;
        assert_eq!(check(r, "scampi").len(), 1);
    }

    #[test]
    fn case_sensitive_keyword() {
        let mut r = rule(RuleKind::Keyword, &["NO"]);
        r.case_sensitive = true;
        let engine = RuleEngine::new(&[r]);
        assert!(engine.evaluate(&msg("a", "no way"), false).is_empty());
        assert_eq!(engine.evaluate(&msg("a", "NO way"), false).len(), 1);
    }

    #[test]
    fn normalization_catches_evasion_and_highlights_original() {
        let mut r = rule(RuleKind::Keyword, &["free followers"]);
        r.normalize = true;
        let hits = check(r.clone(), "get fr33 f0ll0wers!!");
        assert_eq!(hits[0].terms, vec!["fr33 f0ll0wers"]);
        assert_eq!(check(r.clone(), "ｆｒｅｅ fоllowers").len(), 1); // full-width + Cyrillic о
        assert!(check(r, "freedom followers").is_empty());
    }

    #[test]
    fn regex_rules_match_and_validate() {
        let hits = check(rule(RuleKind::Regex, &[r"bit\.ly/\w+"]), "check bit.ly/abc123 now");
        assert_eq!(hits[0].terms, vec!["bit.ly/abc123"]);
        assert!(validate(&rule(RuleKind::Regex, &["(unclosed"])).is_err());
        assert!(validate(&rule(RuleKind::Keyword, &["  "])).is_err());
    }

    #[test]
    fn username_rules() {
        let mut r = rule(RuleKind::Keyword, &["hoss"]);
        r.target = Target::Username;
        r.whole_word = false;
        let engine = RuleEngine::new(&[r]);
        let mut m = msg("a", "hello");
        m.user_login = "hoss00312_bot".into();
        let hits = engine.evaluate(&m, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].terms.is_empty());
        assert!(hits[0].reason.as_deref().unwrap().contains("hoss"));
        assert!(engine.evaluate(&msg("a", "hoss is a word"), false).is_empty());
    }

    #[test]
    fn link_rule_with_allowlist() {
        let r = rule(RuleKind::Links, &["youtube.com", "twitch.tv"]);
        assert_eq!(check(r.clone(), "go to https://evil.example.ru/x now")[0].terms, vec!["https://evil.example.ru/x"]);
        assert_eq!(check(r.clone(), "cheap stuff at shop.xyz")[0].terms, vec!["shop.xyz"]);
        assert_eq!(check(r.clone(), "visit bigdeals dot com")[0].terms.len(), 1);
        assert!(check(r.clone(), "clip: https://www.youtube.com/watch?v=1 and clips.twitch.tv/abc").is_empty());
        assert!(check(r, "this is fine. no links").is_empty());
    }

    #[test]
    fn caps_rule() {
        let r = rule(RuleKind::Caps, &[]);
        assert!(check(r.clone(), "WHY IS THIS HAPPENING TO ME")[0].reason.as_deref().unwrap().contains("100%"));
        assert!(check(r.clone(), "LOL OK").is_empty()); // too short
        assert!(check(r, "This Is A Normal Title Case Sentence").is_empty());
    }

    #[test]
    fn spam_rule() {
        let r = rule(RuleKind::Spam, &[]);
        assert_eq!(check(r.clone(), "nooooooooooooooooo")[0].terms, vec!["o".repeat(17)]);
        assert!(check(r.clone(), "noooo way").is_empty());
        let mut m = msg("a", "Kappa Kappa");
        m.emote_count = 30;
        assert_eq!(RuleEngine::new(&[r.clone()]).evaluate(&m, false).len(), 1);
        assert_eq!(check(r, &"⣿⣿⣿⣿⣿⡿⠿⠛⠋⠉⠉⠉⠛⠻⢿⣿⣿⣿⣿⣿⣿⡿⠋".repeat(1)).len(), 1);
    }

    #[test]
    fn repeat_rule() {
        let mut r = rule(RuleKind::Repeat, &[]);
        r.params.repeat_count = 3;
        r.params.repeat_window_secs = 30;
        let engine = RuleEngine::new(&[r]);
        let at = |ts: i64, text: &str| {
            let mut m = msg("a", text);
            m.timestamp = ts;
            m
        };
        assert!(engine.evaluate(&at(0, "buy my stuff"), false).is_empty());
        assert!(engine.evaluate(&at(1000, "BUY  my stuff"), false).is_empty());
        assert_eq!(engine.evaluate(&at(2000, "buy my stuff"), false).len(), 1);
        // Outside the window, the count starts over.
        assert!(engine.evaluate(&at(100_000, "buy my stuff"), false).is_empty());
    }

    #[test]
    fn first_message_and_raid_only_rules() {
        let mut r = rule(RuleKind::FirstMessage, &[]);
        r.active_during = ActiveDuring::Raid;
        let engine = RuleEngine::new(&[r]);
        let mut m = msg("a", "hi");
        m.is_first_msg = true;
        assert!(engine.evaluate(&m, false).is_empty());
        assert_eq!(engine.evaluate(&m, true).len(), 1);
    }

    #[test]
    fn new_account_rule_uses_known_age_only() {
        let mut r = rule(RuleKind::NewAccount, &[]);
        r.params.max_account_age_days = 7;
        let engine = RuleEngine::new(&[r]);
        let mut m = msg("a", "hi");
        m.timestamp = 10 * 86_400_000;
        assert!(engine.needs_account_age(&m, false));
        assert!(engine.evaluate(&m, false).is_empty()); // age unknown
        let young = EvalCtx { raid_mode: false, account_created_at: Some(8 * 86_400_000) };
        assert_eq!(engine.evaluate_with(&m, young)[0].reason.as_deref(), Some("Account is 2 days old"));
        let old = EvalCtx { raid_mode: false, account_created_at: Some(0) };
        assert!(engine.evaluate_with(&m, old).is_empty());
        assert!(!RuleEngine::new(&[rule(RuleKind::Caps, &[])]).needs_account_age(&m, false));
    }

    #[test]
    fn channel_scope_and_exemptions() {
        let mut r = rule(RuleKind::Keyword, &["spoiler"]);
        r.channels = vec!["gamer".into()];
        r.exempt_privileged = true;
        let engine = RuleEngine::new(&[r]);
        assert_eq!(engine.evaluate(&msg("gamer", "spoiler alert"), false).len(), 1);
        assert!(engine.evaluate(&msg("other", "spoiler alert"), false).is_empty());
        let mut from_mod = msg("gamer", "spoiler alert");
        from_mod.is_mod = true;
        assert!(engine.evaluate(&from_mod, false).is_empty());
    }

    #[test]
    fn old_rules_without_new_fields_still_load() {
        let json = r#"{"id":0,"name":"x","enabled":true,"kind":"keyword","patterns":["a"],"wholeWord":true,"caseSensitive":false,"channels":[],"severity":"high","exemptPrivileged":true}"#;
        let r: Rule = serde_json::from_str(json).unwrap();
        assert_eq!(r.target, Target::Message);
        assert!(!r.normalize);
        assert_eq!(r.params, RuleParams::default());
    }

    #[test]
    fn disabled_rules_are_ignored() {
        let mut r = rule(RuleKind::Keyword, &["x"]);
        r.enabled = false;
        assert!(check(r, "x").is_empty());
    }
}
