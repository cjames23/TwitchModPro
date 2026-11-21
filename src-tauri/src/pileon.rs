//! Pile-on detection: several chatters suddenly replying to or @-ing the same person
//! with hostile messages. No single message has to break a rule for this to show up.

use crate::irc::ChatMessage;
use crate::normalize::normalize;
use crate::waves::WaveMember;
use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
pub struct PileOnConfig {
    /// Distinct chatters needed.
    pub min_users: usize,
    pub window_ms: i64,
}

/// Dismissive or insulting words that make a reply count as hostile. Rule hits and the
/// toxicity model also count; this list catches the common pile-on words those miss.
const RUDE: &[&str] = &[
    "stfu", "shut up", "shut it", "idiot", "stupid", "dumb", "dumbass", "moron", "loser", "clown", "bozo", "trash",
    "garbage", "pathetic", "cringe", "ratio", "nobody asked", "no one asked", "nobody cares", "no one cares",
    "cry about it", "cope", "seethe", "touch grass", "get out", "go away", "kys", "ugly", "fatass", "l bozo",
    "who asked", "get lost", "clueless", "braindead", "brain dead", "rent free", "go cry", "deal with it",
];

static RUDE_AC: LazyLock<AhoCorasick> = LazyLock::new(|| {
    let pats: Vec<String> = RUDE.iter().map(|p| normalize(p).text).collect();
    AhoCorasickBuilder::new().match_kind(MatchKind::LeftmostLongest).build(pats).unwrap()
});

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether a message contains one of the built-in dismissive or insulting words.
pub fn rude(text: &str) -> bool {
    let n = normalize(text).text;
    RUDE_AC.find_iter(&n).any(|m| {
        let before = n[..m.start()].chars().next_back().is_none_or(|c| !word_char(c));
        let after = n[m.end()..].chars().next().is_none_or(|c| !word_char(c));
        before && after
    })
}

/// Who a message is aimed at: the person it replies to, and anyone it @-mentions.
pub fn targets(msg: &ChatMessage) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(parent) = &msg.reply_to {
        out.push(parent.to_lowercase());
    }
    for word in msg.text.split_whitespace().filter(|w| w.starts_with('@')) {
        let name: String = word[1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if (3..=25).contains(&name.len()) {
            out.push(name.to_lowercase());
        }
    }
    out.retain(|t| t != &msg.user_login);
    out.sort();
    out.dedup();
    out
}

#[derive(Debug, Clone)]
struct Entry {
    member: WaveMember,
    hostile: bool,
}

/// A pile-on that crossed the threshold.
#[derive(Debug, Clone)]
pub struct PileOn {
    pub channel: String,
    pub target: String,
    /// Hostile senders (the ones worth acting on).
    pub members: Vec<WaveMember>,
    pub senders: usize,
    pub hostile: usize,
}

#[derive(Default)]
pub struct PileOnDetector {
    recent: HashMap<(String, String), VecDeque<Entry>>,
    /// message id → (channel, target) keys it was recorded under, so a late toxicity score can update it.
    by_message: HashMap<String, Vec<(String, String)>>,
    message_order: VecDeque<String>,
}

impl PileOnDetector {
    /// Record a message. Returns pile-ons it completes or grows.
    pub fn observe(&mut self, msg: &ChatMessage, hostile: bool, cfg: PileOnConfig) -> Vec<PileOn> {
        if msg.is_mod || msg.is_broadcaster || msg.is_vip {
            return vec![];
        }
        let targets = targets(msg);
        if targets.is_empty() {
            return vec![];
        }
        let member = WaveMember {
            login: msg.user_login.clone(),
            display_name: msg.display_name.clone(),
            user_id: msg.user_id.clone(),
            channel: msg.channel.clone(),
            message_id: msg.id.clone(),
            text: msg.text.clone(),
            timestamp: msg.timestamp,
            first_message: msg.is_first_msg,
            outcome: None,
        };
        let keys: Vec<(String, String)> = targets.into_iter().map(|t| (msg.channel.clone(), t)).collect();
        for key in &keys {
            self.recent.entry(key.clone()).or_default().push_back(Entry { member: member.clone(), hostile });
        }
        self.by_message.insert(msg.id.clone(), keys.clone());
        self.message_order.push_back(msg.id.clone());
        while self.message_order.len() > 20_000 {
            if let Some(old) = self.message_order.pop_front() {
                self.by_message.remove(&old);
            }
        }
        if self.recent.len() > 20_000 {
            let cutoff = msg.timestamp - cfg.window_ms;
            self.recent.retain(|_, q| q.back().is_some_and(|e| e.member.timestamp >= cutoff));
        }
        keys.iter().filter_map(|k| self.check(k, msg.timestamp, cfg)).collect()
    }

    /// A message turned out to be hostile after all (e.g. the toxicity model scored it later).
    pub fn mark_hostile(&mut self, message_id: &str, now: i64, cfg: PileOnConfig) -> Vec<PileOn> {
        let Some(keys) = self.by_message.get(message_id).cloned() else { return vec![] };
        for key in &keys {
            if let Some(q) = self.recent.get_mut(key) {
                q.iter_mut().filter(|e| e.member.message_id == message_id).for_each(|e| e.hostile = true);
            }
        }
        keys.iter().filter_map(|k| self.check(k, now, cfg)).collect()
    }

    fn check(&mut self, key: &(String, String), now: i64, cfg: PileOnConfig) -> Option<PileOn> {
        let q = self.recent.get_mut(key)?;
        q.retain(|e| now - e.member.timestamp <= cfg.window_ms);
        let mut senders: Vec<&str> = q.iter().map(|e| e.member.login.as_str()).collect();
        senders.sort();
        senders.dedup();
        let mut hostile: Vec<&Entry> = Vec::new();
        for e in q.iter().filter(|e| e.hostile) {
            if !hostile.iter().any(|h| h.member.login == e.member.login) {
                hostile.push(e);
            }
        }
        let (channel, target) = key;
        let min = cfg.min_users.max(2);
        // Lots of people talk to the streamer, so there every sender has to be hostile.
        let fired = if target == channel {
            hostile.len() >= min
        } else {
            senders.len() >= min && hostile.len() >= (min.div_ceil(2)).max(2)
        };
        if !fired {
            return None;
        }
        Some(PileOn {
            channel: channel.clone(),
            target: target.clone(),
            members: hostile.iter().map(|e| e.member.clone()).collect(),
            senders: senders.len(),
            hostile: hostile.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(login: &str, text: &str, ts: i64) -> ChatMessage {
        ChatMessage {
            id: format!("{login}-{ts}"),
            channel: "chan".into(),
            room_id: None,
            user_id: None,
            user_login: login.into(),
            display_name: login.into(),
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
            timestamp: ts,
        }
    }

    const CFG: PileOnConfig = PileOnConfig { min_users: 4, window_ms: 120_000 };

    #[test]
    fn finds_targets_and_rude_words() {
        let mut m = msg("a", "@Bob_99, stfu lol and @carol too", 0);
        assert_eq!(targets(&m), vec!["bob_99", "carol"]);
        m.reply_to = Some("dave".into());
        assert_eq!(targets(&m), vec!["bob_99", "carol", "dave"]);
        assert!(rude("@bob nobody asked"));
        assert!(rude("L BOZO"));
        assert!(!rude("scope creep")); // "cope" inside a word doesn't count
        assert!(!rude("@bob nice clip"));
    }

    #[test]
    fn hostile_replies_from_several_people_fire() {
        let mut d = PileOnDetector::default();
        assert!(d.observe(&msg("a", "@bob stfu", 0), rude("@bob stfu"), CFG).is_empty());
        assert!(d.observe(&msg("b", "@bob lol", 1), false, CFG).is_empty());
        assert!(d.observe(&msg("c", "@bob ratio", 2), true, CFG).is_empty());
        let p = d.observe(&msg("e", "@bob who asked", 3), true, CFG);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].senders, p[0].hostile, p[0].target.as_str()), (4, 3, "bob"));
        // Only the hostile senders are offered for action.
        assert!(p[0].members.iter().all(|m| m.login != "b"));
    }

    #[test]
    fn friendly_replies_dont_fire_and_window_expires() {
        let mut d = PileOnDetector::default();
        for (i, u) in ["a", "b", "c", "e", "f"].iter().enumerate() {
            assert!(d.observe(&msg(u, "@bob great clip!", i as i64), false, CFG).is_empty());
        }
        let mut d = PileOnDetector::default();
        d.observe(&msg("a", "@bob stfu", 0), true, CFG);
        d.observe(&msg("b", "@bob stfu", 1), true, CFG);
        d.observe(&msg("c", "@bob stfu", 2), true, CFG);
        assert!(d.observe(&msg("e", "@bob stfu", 200_000), true, CFG).is_empty());
    }

    #[test]
    fn streamer_needs_all_hostile_and_late_scores_count() {
        let mut d = PileOnDetector::default();
        for (i, u) in ["a", "b", "c"].iter().enumerate() {
            d.observe(&msg(u, "@chan you suck", i as i64), true, CFG);
        }
        assert!(d.observe(&msg("e", "@chan hi", 3), false, CFG).is_empty());
        let p = d.mark_hostile("e-3", 4, CFG);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].hostile, 4);
    }
}
