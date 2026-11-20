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
    /// One user addressing the streamer many times in a short period.
    Fixation,
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

/// Active rules create flags; test rules only record what they would have flagged.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleMode {
    #[default]
    Active,
    Test,
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
    /// Fixation: flag on the Nth message addressed to the streamer...
    pub fixation_count: usize,
    /// ...within this many minutes.
    pub fixation_window_mins: u64,
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
            fixation_count: 8,
            fixation_window_mins: 10,
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
    /// Keyword/regex rules: the message must ALSO contain one of these.
    #[serde(default)]
    pub also_patterns: Vec<String>,
    /// Strict: only messages clearly talking to the streamer (their name, @, or "you").
    #[serde(default)]
    pub addressed_to_streamer: bool,
    /// Lenient: skip only messages aimed at another chatter (replies, or starting with @someone else).
    #[serde(default)]
    pub not_to_other_chatters: bool,
    /// Phrases that stop this rule from firing.
    #[serde(default)]
    pub exceptions: Vec<String>,
    #[serde(default)]
    pub skip_subscribers: bool,
    /// Skip accounts at least this many days old (0 = off). Only applies once the age is known.
    #[serde(default)]
    pub skip_accounts_older_than_days: u32,
    #[serde(default)]
    pub mode: RuleMode,
    /// A check-in, not an offense: shown without a suggested punishment.
    #[serde(default)]
    pub wellbeing: bool,
    /// Which default pack this came from, if any.
    #[serde(default)]
    pub pack: Option<String>,
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
    /// The rule patterns (as the mod typed them) that matched, for per-word tuning stats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub test_mode: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wellbeing: bool,
}

// ---------------- term matching ----------------

/// Lazily computed forms of the text being checked.
struct Text<'a> {
    raw: &'a str,
    lowered: Option<String>,
    normalized: Option<Normalized>,
}

impl<'a> Text<'a> {
    fn new(raw: &'a str) -> Self {
        Text { raw, lowered: None, normalized: None }
    }
    fn lowered(&mut self) -> &str {
        self.lowered.get_or_insert_with(|| self.raw.to_lowercase())
    }
    fn normalized(&mut self) -> &Normalized {
        self.normalized.get_or_insert_with(|| normalize(self.raw))
    }
}

enum TermKind {
    Keywords(AhoCorasick),
    Regex(Regex),
}

/// A compiled word/phrase or regex list with the rule's matching options.
struct Terms {
    kind: TermKind,
    /// Compiled (normalized or lowercased) keyword patterns.
    compiled: Vec<String>,
    /// The patterns as the mod typed them.
    originals: Vec<String>,
    normalize: bool,
    whole_word: bool,
    case_sensitive: bool,
}

/// A match: the snippet of the original text, and the pattern that produced it (if known).
type Hit = (String, Option<String>);

impl Terms {
    fn build(regex: bool, patterns: &[String], normalize: bool, whole_word: bool, case_sensitive: bool) -> Result<Self, String> {
        let originals: Vec<String> = patterns.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).map(str::to_string).collect();
        if originals.is_empty() {
            return Err("empty".into());
        }
        if regex {
            for p in &originals {
                Regex::new(p).map_err(|e| format!("Invalid pattern `{p}`: {e}"))?;
            }
            let joined = originals.iter().map(|p| format!("(?:{p})")).collect::<Vec<_>>().join("|");
            let source = if whole_word { format!(r"\b(?:{joined})\b") } else { joined };
            let re = RegexBuilder::new(&source)
                .case_insensitive(!case_sensitive || normalize)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| e.to_string())?;
            return Ok(Terms { kind: TermKind::Regex(re), compiled: vec![], originals, normalize, whole_word, case_sensitive });
        }
        let compiled: Vec<String> = originals
            .iter()
            .map(|p| {
                if normalize {
                    normalize_pattern(p)
                } else if case_sensitive {
                    p.clone()
                } else {
                    p.to_lowercase()
                }
            })
            .collect();
        if compiled.iter().all(|p| p.is_empty()) {
            return Err("empty".into());
        }
        let ac = AhoCorasickBuilder::new().match_kind(MatchKind::Standard).build(&compiled).map_err(|e| e.to_string())?;
        Ok(Terms { kind: TermKind::Keywords(ac), compiled, originals, normalize, whole_word, case_sensitive })
    }

    fn find(&self, text: &mut Text) -> Vec<Hit> {
        let mut out: Vec<Hit> = Vec::new();
        let mut push = |snippet: String, pattern: Option<String>| {
            if !snippet.is_empty() && !out.iter().any(|(s, _)| s == &snippet) {
                out.push((snippet, pattern));
            }
        };
        match &self.kind {
            TermKind::Keywords(ac) => {
                if self.normalize {
                    let raw = text.raw;
                    let n = text.normalized();
                    for m in ac.find_overlapping_iter(&n.text) {
                        if self.whole_word && !at_word_boundaries(&n.text, m.start(), m.end()) {
                            continue;
                        }
                        let (a, b) = n.original_range(m.start(), m.end());
                        push(raw[a..b].to_string(), Some(self.originals[m.pattern().as_usize()].clone()));
                    }
                } else {
                    let hay: &str = if self.case_sensitive { text.raw } else { text.lowered() };
                    for m in ac.find_overlapping_iter(hay) {
                        if self.whole_word && !at_word_boundaries(hay, m.start(), m.end()) {
                            continue;
                        }
                        let i = m.pattern().as_usize();
                        push(self.compiled[i].clone(), Some(self.originals[i].clone()));
                    }
                }
            }
            TermKind::Regex(re) => {
                if self.normalize {
                    let raw = text.raw;
                    let n = text.normalized();
                    for m in re.find_iter(&n.text) {
                        let (a, b) = n.original_range(m.start(), m.end());
                        push(raw[a..b].to_string(), None);
                    }
                } else {
                    for m in re.find_iter(text.raw) {
                        push(m.as_str().to_string(), None);
                    }
                }
            }
        }
        out
    }
}

// ---------------- rule compilation ----------------

enum Matcher {
    Terms(Terms),
    Links { allow: Vec<String> },
    Builtin,
}

struct CompiledRule {
    rule: Rule,
    matcher: Matcher,
    also: Option<Terms>,
    exceptions: Option<Terms>,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn at_word_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before_ok = text[..start].chars().next_back().is_none_or(|c| !is_word_char(c));
    let after_ok = text[end..].chars().next().is_none_or(|c| !is_word_char(c));
    before_ok && after_ok
}

fn clean(list: &[String]) -> Vec<String> {
    list.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).map(str::to_string).collect()
}

/// Check a rule definition and build its matchers. Errors are human-readable.
fn compile(rule: &Rule) -> Result<CompiledRule, String> {
    let patterns = clean(&rule.patterns);
    let uses_terms = matches!(rule.kind, RuleKind::Keyword | RuleKind::Regex);
    let matcher = match rule.kind {
        RuleKind::Keyword | RuleKind::Regex => {
            let regex = rule.kind == RuleKind::Regex;
            Matcher::Terms(
                Terms::build(regex, &patterns, rule.normalize, rule.whole_word, rule.case_sensitive).map_err(|e| {
                    if e == "empty" {
                        if regex { "Add at least one pattern.".into() } else { "Add at least one word or phrase.".into() }
                    } else {
                        e
                    }
                })?,
            )
        }
        RuleKind::Links => Matcher::Links {
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
        },
        RuleKind::Caps => {
            if !(1..=100).contains(&rule.params.caps_percent) {
                return Err("Caps percentage must be between 1 and 100.".into());
            }
            Matcher::Builtin
        }
        RuleKind::Spam => {
            if rule.params.max_repeated_chars < 3 {
                return Err("Repeated-character limit must be at least 3.".into());
            }
            Matcher::Builtin
        }
        RuleKind::Repeat => {
            if rule.params.repeat_count < 2 || rule.params.repeat_window_secs == 0 {
                return Err("Repeat count must be at least 2 and the time window above 0.".into());
            }
            Matcher::Builtin
        }
        RuleKind::FirstMessage => Matcher::Builtin,
        RuleKind::NewAccount => {
            if rule.params.max_account_age_days == 0 {
                return Err("Account age must be at least 1 day.".into());
            }
            Matcher::Builtin
        }
        RuleKind::Fixation => {
            if rule.params.fixation_count < 2 || rule.params.fixation_window_mins == 0 {
                return Err("Fixation needs at least 2 messages and a time window above 0.".into());
            }
            Matcher::Builtin
        }
    };
    let also = match (uses_terms && rule.target == Target::Message, clean(&rule.also_patterns)) {
        (true, list) if !list.is_empty() => Some(
            Terms::build(rule.kind == RuleKind::Regex, &list, rule.normalize, rule.whole_word, rule.case_sensitive)
                .map_err(|e| format!("“Must also contain”: {e}"))?,
        ),
        _ => None,
    };
    // Exceptions are always plain phrases, matched leniently.
    let exceptions = match clean(&rule.exceptions) {
        list if list.is_empty() => None,
        list => Some(Terms::build(false, &list, true, true, false).map_err(|e| format!("Exceptions: {e}"))?),
    };
    Ok(CompiledRule { rule: rule.clone(), matcher, also, exceptions })
}

pub fn validate(rule: &Rule) -> Result<(), String> {
    if rule.name.trim().is_empty() {
        return Err("Give the rule a name.".into());
    }
    compile(rule).map(|_| ())
}

// ---------------- helpers ----------------

static LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ix)
        \bhttps?://[^\s]+
        | \b(?:[a-z0-9-]+\.)+(?:com|net|org|gg|tv|io|ly|co|me|xyz|ru|info|biz|link|site|online|shop|app|dev|live|stream|fun|club|top|to|cc|us|uk|de|store|pro|icu|vip)\b(?:/[^\s]*)?
        | \b[a-z0-9-]+\s*[\(\[]?\s*dot\s*[\)\]]?\s*(?:com|net|gg|ru|tv|xyz)\b",
    )
    .unwrap()
});

const SECOND_PERSON: &[&str] = &["you", "your", "youre", "yours", "yourself", "ur", "u", "ya", "yall", "youve", "youd", "youll"];

/// Whether a message is aimed at another chatter: a reply to them, or starting with @them.
pub fn to_other_chatter(msg: &ChatMessage) -> bool {
    let channel = msg.channel.as_str();
    if let Some(parent) = &msg.reply_to {
        return parent != channel;
    }
    let first = msg.text.split_whitespace().next().unwrap_or_default();
    first.starts_with('@') && !first.trim_start_matches('@').trim_end_matches([',', ':']).eq_ignore_ascii_case(channel)
}

/// Whether a message is clearly talking to the streamer: names or @s them, or says "you".
pub fn addressed_to_streamer(msg: &ChatMessage) -> bool {
    let channel = msg.channel.as_str();
    if let Some(parent) = &msg.reply_to {
        return parent == channel;
    }
    let lower = msg.text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '@' || c == '\''))
        .filter(|w| !w.is_empty())
        .collect();
    if words.iter().any(|w| w.trim_start_matches('@') == channel) {
        return true;
    }
    // Starting with @someone else means they're talking to that person.
    if words.first().is_some_and(|w| w.starts_with('@')) {
        return false;
    }
    words.iter().any(|w| SECOND_PERSON.contains(&w.replace('\'', "").as_str()))
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

// ---------------- per-user state ----------------

/// Recent messages per (channel, user), for repeat detection.
#[derive(Default)]
struct RepeatTracker {
    recent: HashMap<(String, String), VecDeque<(i64, String)>>,
    max_window_ms: i64,
}

impl RepeatTracker {
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
            .map(|q| q.iter().filter(|(t, text)| msg.timestamp - t <= window_ms && text == key_text).count())
            .unwrap_or(0)
    }
}

/// Messages addressed to the streamer per (channel, user), for fixation rules.
#[derive(Default)]
struct FixationTracker {
    /// Timestamps of addressed messages, and when each rule last flagged this user.
    users: HashMap<(String, String), (VecDeque<i64>, HashMap<i64, i64>)>,
    max_window_ms: i64,
}

impl FixationTracker {
    fn record(&mut self, msg: &ChatMessage) {
        let (times, _) = self.users.entry((msg.channel.clone(), msg.user_login.clone())).or_default();
        times.push_back(msg.timestamp);
        while times.front().is_some_and(|t| msg.timestamp - t > self.max_window_ms) || times.len() > 200 {
            times.pop_front();
        }
        if self.users.len() > 20_000 {
            let cutoff = msg.timestamp - self.max_window_ms;
            self.users.retain(|_, (q, _)| q.back().is_some_and(|t| *t >= cutoff));
        }
    }

    /// Count within the window; fires at most once per window per rule.
    fn check(&mut self, msg: &ChatMessage, rule_id: i64, count: usize, window_ms: i64) -> Option<usize> {
        let (times, last) = self.users.get_mut(&(msg.channel.clone(), msg.user_login.clone()))?;
        let n = times.iter().filter(|t| msg.timestamp - **t <= window_ms).count();
        if n < count || last.get(&rule_id).is_some_and(|t| msg.timestamp - t < window_ms) {
            return None;
        }
        last.insert(rule_id, msg.timestamp);
        Some(n)
    }
}

// ---------------- engine ----------------

pub struct RuleEngine {
    rules: Vec<CompiledRule>,
    repeats: Mutex<RepeatTracker>,
    fixations: Mutex<FixationTracker>,
    has_repeat_rules: bool,
    has_fixation_rules: bool,
}

impl Default for RuleEngine {
    fn default() -> Self {
        RuleEngine::new(&[])
    }
}

/// Extra facts available when checking a message.
#[derive(Debug, Clone, Copy, Default)]
pub struct EvalCtx {
    pub raid_mode: bool,
    /// When the sender's account was created (ms), if known.
    pub account_created_at: Option<i64>,
}

fn applies(rule: &Rule, msg: &ChatMessage, ctx: EvalCtx) -> bool {
    let privileged = msg.is_mod || msg.is_broadcaster || msg.is_vip;
    if (rule.exempt_privileged && privileged) || (rule.skip_subscribers && msg.is_subscriber) {
        return false;
    }
    if rule.active_during == ActiveDuring::Raid && !ctx.raid_mode {
        return false;
    }
    if !rule.channels.is_empty() && !rule.channels.iter().any(|c| c.eq_ignore_ascii_case(&msg.channel)) {
        return false;
    }
    if rule.skip_accounts_older_than_days > 0 {
        if let Some(created) = ctx.account_created_at {
            if (msg.timestamp - created) / 86_400_000 >= rule.skip_accounts_older_than_days as i64 {
                return false;
            }
        }
    }
    true
}

impl RuleEngine {
    /// Build an engine from saved rules. Disabled or invalid rules are skipped.
    pub fn new(rules: &[Rule]) -> Self {
        let compiled: Vec<CompiledRule> = rules.iter().filter(|r| r.enabled).filter_map(|r| compile(r).ok()).collect();
        let window = |kind: RuleKind, f: fn(&RuleParams) -> i64| {
            compiled.iter().filter(|c| c.rule.kind == kind).map(|c| f(&c.rule.params)).max()
        };
        let repeat_window = window(RuleKind::Repeat, |p| p.repeat_window_secs as i64 * 1000);
        let fixation_window = window(RuleKind::Fixation, |p| p.fixation_window_mins as i64 * 60_000);
        RuleEngine {
            has_repeat_rules: repeat_window.is_some(),
            has_fixation_rules: fixation_window.is_some(),
            repeats: Mutex::new(RepeatTracker { recent: HashMap::new(), max_window_ms: repeat_window.unwrap_or(0) }),
            fixations: Mutex::new(FixationTracker { users: HashMap::new(), max_window_ms: fixation_window.unwrap_or(0) }),
            rules: compiled,
        }
    }

    pub fn kind_of(&self, rule_id: i64) -> Option<RuleKind> {
        self.rules.iter().find(|c| c.rule.id == rule_id).map(|c| c.rule.kind)
    }

    /// Whether an enabled new-account rule applies to this message (so its sender's age is worth looking up).
    pub fn needs_account_age(&self, msg: &ChatMessage, raid_mode: bool) -> bool {
        let ctx = EvalCtx { raid_mode, account_created_at: None };
        self.rules.iter().any(|c| {
            (c.rule.kind == RuleKind::NewAccount || c.rule.skip_accounts_older_than_days > 0) && applies(&c.rule, msg, ctx)
        })
    }

    /// Check a message. `raid_mode` is whether its channel is currently in raid mode.
    pub fn evaluate(&self, msg: &ChatMessage, raid_mode: bool) -> Vec<RuleMatch> {
        self.evaluate_with(msg, EvalCtx { raid_mode, account_created_at: None })
    }

    pub fn evaluate_with(&self, msg: &ChatMessage, ctx: EvalCtx) -> Vec<RuleMatch> {
        let mut text = Text::new(&msg.text);
        let mut out = Vec::new();
        let addressed = addressed_to_streamer(msg);

        let repeat_text = self.has_repeat_rules.then(|| repeat_key(&msg.text));
        let tracker = self.has_repeat_rules.then(|| {
            let mut t = self.repeats.lock().unwrap();
            if let Some(key) = &repeat_text {
                t.record(msg, key);
            }
            t
        });
        let mut fixations = (self.has_fixation_rules && addressed && !msg.is_broadcaster).then(|| {
            let mut f = self.fixations.lock().unwrap();
            f.record(msg);
            f
        });

        for cr in &self.rules {
            let rule = &cr.rule;
            if !applies(rule, msg, ctx) {
                continue;
            }
            if rule.addressed_to_streamer && !addressed {
                continue;
            }
            if rule.not_to_other_chatters && to_other_chatter(msg) {
                continue;
            }
            if let Some(ex) = &cr.exceptions {
                if !ex.find(&mut text).is_empty() {
                    continue;
                }
            }

            let mut terms: Vec<String> = Vec::new();
            let mut patterns: Vec<String> = Vec::new();
            let mut reason: Option<String> = None;
            let add = |hits: Vec<Hit>, terms: &mut Vec<String>, patterns: &mut Vec<String>| {
                for (snippet, pattern) in hits {
                    if !terms.contains(&snippet) {
                        terms.push(snippet);
                    }
                    if let Some(p) = pattern {
                        if !patterns.contains(&p) {
                            patterns.push(p);
                        }
                    }
                }
            };

            match &cr.matcher {
                Matcher::Terms(t) => {
                    if rule.target == Target::Username {
                        let mut name = Text::new(&msg.user_login);
                        let hits = t.find(&mut name);
                        if !hits.is_empty() {
                            let shown: Vec<&str> = hits.iter().map(|(s, _)| s.as_str()).collect();
                            reason = Some(format!("Username contains “{}”", shown.join("”, “")));
                            patterns.extend(hits.into_iter().filter_map(|(_, p)| p));
                        }
                    } else {
                        let hits = t.find(&mut text);
                        if !hits.is_empty() {
                            match &cr.also {
                                Some(also) => {
                                    let extra = also.find(&mut text);
                                    if !extra.is_empty() {
                                        add(hits, &mut terms, &mut patterns);
                                        // Highlight the "also" words too, but don't count them as rule patterns.
                                        add(extra.into_iter().map(|(s, _)| (s, None)).collect(), &mut terms, &mut patterns);
                                    }
                                }
                                None => add(hits, &mut terms, &mut patterns),
                            }
                        }
                    }
                }
                Matcher::Links { allow } => {
                    for m in LINK_RE.find_iter(&msg.text) {
                        let host = link_host(m.as_str());
                        if !domain_allowed(&host, allow) {
                            add(vec![(m.as_str().to_string(), Some(host))], &mut terms, &mut patterns);
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
                                terms.push(c.to_string().repeat(n));
                            }
                        }
                        if reason.is_none() && msg.emote_count > rule.params.max_emotes {
                            reason = Some(format!("{} emotes", msg.emote_count));
                        }
                        if reason.is_none() && msg.text.chars().filter(|c| is_art_char(*c)).count() >= 20 {
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
                    RuleKind::Fixation => {
                        if let Some(f) = fixations.as_mut() {
                            let window = rule.params.fixation_window_mins;
                            if let Some(n) = f.check(msg, rule.id, rule.params.fixation_count, window as i64 * 60_000) {
                                reason = Some(format!("{n} messages to the streamer in {window} min"));
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
                    patterns,
                    test_mode: rule.mode == RuleMode::Test,
                    wellbeing: rule.wellbeing,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn rule(kind: RuleKind, patterns: &[&str]) -> Rule {
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
            also_patterns: vec![],
            addressed_to_streamer: false,
            not_to_other_chatters: false,
            exceptions: vec![],
            skip_subscribers: false,
            skip_accounts_older_than_days: 0,
            mode: RuleMode::Active,
            wellbeing: false,
            pack: None,
        }
    }

    pub(crate) fn msg(channel: &str, text: &str) -> ChatMessage {
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
            reply_to: None,
            timestamp: 0,
        }
    }

    fn check(r: Rule, text: &str) -> Vec<RuleMatch> {
        RuleEngine::new(&[r]).evaluate(&msg("streamer", text), false)
    }

    #[test]
    fn keyword_is_case_insensitive_and_whole_word() {
        let engine = RuleEngine::new(&[rule(RuleKind::Keyword, &["scam", "Free Followers"])]);
        let hits = engine.evaluate(&msg("a", "Get FREE Followers now, totally not a SCAM"), false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].terms, vec!["free followers", "scam"]);
        assert_eq!(hits[0].patterns, vec!["Free Followers", "scam"]);
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
        assert_eq!(hits[0].patterns, vec!["free followers"]);
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
        let hits = check(r.clone(), "go to https://evil.example.ru/x now");
        assert_eq!(hits[0].terms, vec!["https://evil.example.ru/x"]);
        assert_eq!(hits[0].patterns, vec!["evil.example.ru"]);
        assert_eq!(check(r.clone(), "cheap stuff at shop.xyz")[0].terms, vec!["shop.xyz"]);
        assert_eq!(check(r.clone(), "visit bigdeals dot com")[0].terms.len(), 1);
        assert!(check(r.clone(), "clip: https://www.youtube.com/watch?v=1 and clips.twitch.tv/abc").is_empty());
        assert!(check(r, "this is fine. no links").is_empty());
    }

    #[test]
    fn caps_spam_repeat_first_message() {
        let r = rule(RuleKind::Caps, &[]);
        assert!(check(r.clone(), "WHY IS THIS HAPPENING TO ME")[0].reason.as_deref().unwrap().contains("100%"));
        assert!(check(r, "LOL OK").is_empty());

        let s = rule(RuleKind::Spam, &[]);
        assert_eq!(check(s.clone(), "nooooooooooooooooo")[0].terms, vec!["o".repeat(17)]);
        assert!(check(s.clone(), "noooo way").is_empty());
        assert_eq!(check(s, "⣿⣿⣿⣿⣿⡿⠿⠛⠋⠉⠉⠉⠛⠻⢿⣿⣿⣿⣿⣿⣿⡿⠋").len(), 1);

        let mut rep = rule(RuleKind::Repeat, &[]);
        rep.params.repeat_window_secs = 30;
        let engine = RuleEngine::new(&[rep]);
        let at = |ts: i64, text: &str| {
            let mut m = msg("a", text);
            m.timestamp = ts;
            m
        };
        assert!(engine.evaluate(&at(0, "buy my stuff"), false).is_empty());
        assert!(engine.evaluate(&at(1000, "BUY  my stuff"), false).is_empty());
        assert_eq!(engine.evaluate(&at(2000, "buy my stuff"), false).len(), 1);
        assert!(engine.evaluate(&at(100_000, "buy my stuff"), false).is_empty());

        let mut first = rule(RuleKind::FirstMessage, &[]);
        first.active_during = ActiveDuring::Raid;
        let engine = RuleEngine::new(&[first]);
        let mut m = msg("a", "hi");
        m.is_first_msg = true;
        assert!(engine.evaluate(&m, false).is_empty());
        assert_eq!(engine.evaluate(&m, true).len(), 1);
    }

    #[test]
    fn new_account_rule_uses_known_age_only() {
        let engine = RuleEngine::new(&[rule(RuleKind::NewAccount, &[])]);
        let mut m = msg("a", "hi");
        m.timestamp = 10 * 86_400_000;
        assert!(engine.needs_account_age(&m, false));
        assert!(engine.evaluate(&m, false).is_empty());
        let young = EvalCtx { raid_mode: false, account_created_at: Some(8 * 86_400_000) };
        assert_eq!(engine.evaluate_with(&m, young)[0].reason.as_deref(), Some("Account is 2 days old"));
        let old = EvalCtx { raid_mode: false, account_created_at: Some(0) };
        assert!(engine.evaluate_with(&m, old).is_empty());
    }

    #[test]
    fn must_also_contain() {
        let mut r = rule(RuleKind::Keyword, &["show me your", "turn around"]);
        r.normalize = true;
        r.also_patterns = vec!["feet".into(), "body".into()];
        let hits = check(r.clone(), "show me your feet");
        assert_eq!(hits[0].terms, vec!["show me your", "feet"]);
        assert_eq!(hits[0].patterns, vec!["show me your"]); // "also" words aren't tuning patterns
        assert!(check(r, "show me your setup").is_empty());
    }

    #[test]
    fn addressed_to_streamer_detection() {
        let m = |text: &str| msg("pixelqueen", text);
        assert!(addressed_to_streamer(&m("marry me pixelqueen")));
        assert!(addressed_to_streamer(&m("@PixelQueen you're amazing")));
        assert!(addressed_to_streamer(&m("are u single")));
        assert!(!addressed_to_streamer(&m("@otherguy you're wrong")));
        assert!(!addressed_to_streamer(&m("this game is great")));
        let mut reply = m("you're so funny");
        reply.reply_to = Some("someone".into());
        assert!(!addressed_to_streamer(&reply));
        reply.reply_to = Some("pixelqueen".into());
        assert!(addressed_to_streamer(&reply));

        let mut r = rule(RuleKind::Keyword, &["marry me"]);
        r.addressed_to_streamer = true;
        let engine = RuleEngine::new(&[r]);
        assert_eq!(engine.evaluate(&m("marry me pixelqueen"), false).len(), 1);
        assert!(engine.evaluate(&m("@bob marry me lol"), false).is_empty());
    }

    #[test]
    fn not_to_other_chatters_is_lenient() {
        let m = |text: &str| msg("shadesofnate", text);
        // Commands with no "you" still count as aimed at the streamer...
        assert!(!to_other_chatter(&m("go left")));
        assert!(!to_other_chatter(&m("@ShadesOfNate go left")));
        // ...but messages aimed at another chatter don't.
        assert!(to_other_chatter(&m("@smokeyfireman go left")));
        let mut reply = m("go left");
        reply.reply_to = Some("smokeyfireman".into());
        assert!(to_other_chatter(&reply));
        reply.reply_to = Some("shadesofnate".into());
        assert!(!to_other_chatter(&reply));

        let mut r = rule(RuleKind::Keyword, &["go left"]);
        r.not_to_other_chatters = true;
        let engine = RuleEngine::new(&[r]);
        assert_eq!(engine.evaluate(&m("go left"), false).len(), 1);
        assert!(engine.evaluate(&m("@bob go left"), false).is_empty());
    }

    #[test]
    fn exceptions_skip_subscribers_and_older_accounts() {
        let mut r = rule(RuleKind::Keyword, &["die"]);
        r.exceptions = vec!["die hard".into(), "dark souls".into()];
        assert!(check(r.clone(), "I love Die Hard").is_empty());
        assert_eq!(check(r.clone(), "go die").len(), 1);

        r.skip_subscribers = true;
        let engine = RuleEngine::new(&[r.clone()]);
        let mut sub = msg("a", "go die");
        sub.is_subscriber = true;
        assert!(engine.evaluate(&sub, false).is_empty());

        r.skip_subscribers = false;
        r.skip_accounts_older_than_days = 30;
        let engine = RuleEngine::new(&[r]);
        let mut m = msg("a", "go die");
        m.timestamp = 100 * 86_400_000;
        assert!(engine.needs_account_age(&m, false));
        assert_eq!(engine.evaluate(&m, false).len(), 1); // age unknown: still flags
        let old = EvalCtx { raid_mode: false, account_created_at: Some(0) };
        assert!(engine.evaluate_with(&m, old).is_empty());
    }

    #[test]
    fn fixation_fires_once_per_window() {
        let mut r = rule(RuleKind::Fixation, &[]);
        r.params.fixation_count = 3;
        r.params.fixation_window_mins = 10;
        let engine = RuleEngine::new(&[r]);
        let at = |ts: i64, text: &str| {
            let mut m = msg("pixelqueen", text);
            m.timestamp = ts;
            m
        };
        assert!(engine.evaluate(&at(0, "hi pixelqueen"), false).is_empty());
        assert!(engine.evaluate(&at(1_000, "the boss is hard"), false).is_empty()); // not addressed
        assert!(engine.evaluate(&at(2_000, "you're so pretty"), false).is_empty());
        let hit = engine.evaluate(&at(3_000, "notice me pixelqueen"), false);
        assert_eq!(hit[0].reason.as_deref(), Some("3 messages to the streamer in 10 min"));
        assert!(engine.evaluate(&at(4_000, "pls answer me"), false).is_empty()); // already flagged this window
        assert_eq!(engine.evaluate(&at(20 * 60_000, "u there"), false).len(), 0); // window expired, count reset
    }

    #[test]
    fn test_mode_and_wellbeing_are_reported() {
        let mut r = rule(RuleKind::Keyword, &["i want to die"]);
        r.mode = RuleMode::Test;
        r.wellbeing = true;
        let hits = check(r, "honestly i want to die");
        assert!(hits[0].test_mode && hits[0].wellbeing);
    }

    #[test]
    fn channel_scope_exemptions_and_disabled() {
        let mut r = rule(RuleKind::Keyword, &["spoiler"]);
        r.channels = vec!["gamer".into()];
        r.exempt_privileged = true;
        let engine = RuleEngine::new(&[r]);
        assert_eq!(engine.evaluate(&msg("gamer", "spoiler alert"), false).len(), 1);
        assert!(engine.evaluate(&msg("other", "spoiler alert"), false).is_empty());
        let mut from_mod = msg("gamer", "spoiler alert");
        from_mod.is_mod = true;
        assert!(engine.evaluate(&from_mod, false).is_empty());

        let mut off = rule(RuleKind::Keyword, &["x"]);
        off.enabled = false;
        assert!(check(off, "x").is_empty());
    }

    #[test]
    fn old_rules_without_new_fields_still_load() {
        let json = r#"{"id":0,"name":"x","enabled":true,"kind":"keyword","patterns":["a"],"wholeWord":true,"caseSensitive":false,"channels":[],"severity":"high","exemptPrivileged":true}"#;
        let r: Rule = serde_json::from_str(json).unwrap();
        assert_eq!(r.target, Target::Message);
        assert_eq!(r.mode, RuleMode::Active);
        assert!(!r.normalize && r.also_patterns.is_empty());
        assert_eq!(r.params, RuleParams::default());
    }
}
