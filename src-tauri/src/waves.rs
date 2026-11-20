//! Spam-wave detection: many different accounts doing the same thing at once.
//!
//! Three signals, each over a sliding time window:
//! - the same (normalized) message from N+ accounts,
//! - N+ accounts whose names share a stem (`hoss00312_1`, `hoss00312_2`, …),
//! - a burst of first-time chatters in one channel.

use crate::irc::ChatMessage;
use crate::normalize::normalize;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, VecDeque};

/// Messages shorter than this (after normalizing) are too common to mean anything ("lol", "W").
const MIN_SIGNATURE_LEN: usize = 12;
const MIN_STEM_LEN: usize = 6;
/// A wave closes after this long without new members.
const WAVE_IDLE_MS: i64 = 5 * 60 * 1000;
/// After a wave is dismissed, ignore the same pattern for this long.
const SUPPRESS_MS: i64 = 30 * 60 * 1000;
const MAX_MEMBERS: usize = 500;
const MAX_WAVES: usize = 50;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum WaveKind {
    SameMessage,
    SimilarNames,
    FirstTimeBurst,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveMember {
    pub login: String,
    pub display_name: String,
    pub user_id: Option<String>,
    pub channel: String,
    pub message_id: String,
    pub text: String,
    pub timestamp: i64,
    pub first_message: bool,
    /// "banned", "timedOut", "failed: …", "unbanned" once acted on.
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wave {
    pub id: i64,
    pub kind: WaveKind,
    /// The shared message, name stem, or channel.
    pub key: String,
    pub channels: BTreeSet<String>,
    pub members: Vec<WaveMember>,
    pub first_seen: i64,
    pub last_seen: i64,
    /// "open", "actioned" or "dismissed"
    pub status: String,
    pub severity: crate::rules::Severity,
}

impl Wave {
    fn refresh_severity(&mut self) {
        let firsts = self.members.iter().filter(|m| m.first_message).count();
        self.severity = if self.channels.len() > 1 || firsts * 2 >= self.members.len() {
            crate::rules::Severity::High
        } else {
            crate::rules::Severity::Medium
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WaveConfig {
    pub min_accounts: usize,
    pub window_ms: i64,
}

/// Normalized form used to decide two messages are "the same": no digits, symbols or extra spaces,
/// so `buy followers 123!!` and `buy followers 456` match.
pub fn signature(text: &str) -> String {
    // Drop words with no letters first ("123", "!!!"), so numbers don't turn into leetspeak letters.
    let kept: Vec<&str> = text.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).collect();
    let n = normalize(&kept.join(" ")).text;
    let words: Vec<String> = n
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let s = words.join(" ");
    s.chars().take(160).collect()
}

/// `hoss00312_bot7` → `hoss00312_bot`; `coolguy_123` → `coolguy`.
pub fn name_stem(login: &str) -> Option<String> {
    let stem = login.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
    (stem.len() >= MIN_STEM_LEN && stem.len() < login.len()).then(|| stem.to_string())
}

#[derive(Default)]
pub struct WaveDetector {
    /// Recent sightings per signal: (timestamp, channel, login) plus the member details.
    recent: HashMap<(WaveKind, String), VecDeque<WaveMember>>,
    waves: Vec<Wave>,
    suppressed: HashMap<(WaveKind, String), i64>,
    next_id: i64,
}

impl WaveDetector {
    pub fn waves(&self) -> &[Wave] {
        &self.waves
    }

    pub fn get_mut(&mut self, id: i64) -> Option<&mut Wave> {
        self.waves.iter_mut().find(|w| w.id == id)
    }

    /// Mark a wave dismissed and ignore its pattern for a while.
    pub fn dismiss(&mut self, id: i64, now: i64) -> Option<Wave> {
        let w = self.get_mut(id)?;
        w.status = "dismissed".into();
        let key = (w.kind, w.key.clone());
        let out = w.clone();
        self.suppressed.insert(key, now + SUPPRESS_MS);
        Some(out)
    }

    /// Feed a chat message. Returns any waves that were created or grew.
    pub fn observe(&mut self, msg: &ChatMessage, cfg: WaveConfig) -> Vec<Wave> {
        if msg.is_mod || msg.is_broadcaster || msg.is_vip {
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

        let mut signals = Vec::new();
        let sig = signature(&msg.text);
        if sig.len() >= MIN_SIGNATURE_LEN {
            signals.push((WaveKind::SameMessage, sig, cfg.min_accounts));
        }
        if let Some(stem) = name_stem(&msg.user_login) {
            signals.push((WaveKind::SimilarNames, stem, cfg.min_accounts));
        }
        if msg.is_first_msg {
            // First-time chatters are normal, so a burst needs twice as many.
            signals.push((WaveKind::FirstTimeBurst, msg.channel.clone(), cfg.min_accounts * 2));
        }

        let mut changed = Vec::new();
        for (kind, key, threshold) in signals {
            if let Some(w) = self.observe_signal(kind, key, threshold, &member, cfg.window_ms) {
                changed.push(w);
            }
        }
        changed
    }

    fn observe_signal(&mut self, kind: WaveKind, key: String, threshold: usize, m: &WaveMember, window: i64) -> Option<Wave> {
        let now = m.timestamp;
        if self.suppressed.get(&(kind, key.clone())).is_some_and(|until| *until > now) {
            return None;
        }

        // Join an active wave for the same pattern if there is one.
        if let Some(w) = self
            .waves
            .iter_mut()
            .find(|w| w.kind == kind && w.key == key && w.status == "open" && now - w.last_seen < WAVE_IDLE_MS)
        {
            if w.members.iter().any(|x| x.login == m.login && x.channel == m.channel) || w.members.len() >= MAX_MEMBERS {
                return None;
            }
            w.members.push(m.clone());
            w.channels.insert(m.channel.clone());
            w.last_seen = now;
            w.refresh_severity();
            return Some(w.clone());
        }

        let q = self.recent.entry((kind, key.clone())).or_default();
        q.retain(|x| now - x.timestamp <= window);
        if !q.iter().any(|x| x.login == m.login) {
            q.push_back(m.clone());
        }
        let distinct: BTreeSet<&str> = q.iter().map(|x| x.login.as_str()).collect();
        if distinct.len() < threshold.max(2) {
            self.prune(now, window);
            return None;
        }

        let members: Vec<WaveMember> = q.drain(..).collect();
        self.recent.remove(&(kind, key.clone()));
        self.next_id += 1;
        let mut wave = Wave {
            id: self.next_id,
            kind,
            channels: members.iter().map(|x| x.channel.clone()).collect(),
            first_seen: members.first().map(|x| x.timestamp).unwrap_or(now),
            last_seen: now,
            key,
            members,
            status: "open".into(),
            severity: crate::rules::Severity::Medium,
        };
        wave.refresh_severity();
        self.waves.insert(0, wave.clone());
        self.waves.truncate(MAX_WAVES);
        Some(wave)
    }

    fn prune(&mut self, now: i64, window: i64) {
        if self.recent.len() > 20_000 {
            self.recent.retain(|_, q| q.back().is_some_and(|x| now - x.timestamp <= window));
        }
        self.suppressed.retain(|_, until| *until > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(login: &str, channel: &str, text: &str, ts: i64) -> ChatMessage {
        ChatMessage {
            id: format!("{login}-{ts}"),
            channel: channel.into(),
            room_id: None,
            user_id: Some(format!("id-{login}")),
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
            timestamp: ts,
        }
    }

    const CFG: WaveConfig = WaveConfig { min_accounts: 3, window_ms: 60_000 };

    #[test]
    fn signature_ignores_numbers_symbols_and_disguises() {
        assert_eq!(signature("Buy F0LLOWERS at site 123!!"), signature("buy followers at site 456"));
        assert_eq!(name_stem("hoss00312_bot7").as_deref(), Some("hoss00312_bot"));
        assert_eq!(name_stem("alex12"), None); // stem too short
        assert_eq!(name_stem("plainname"), None); // no suffix
    }

    #[test]
    fn same_message_from_many_accounts_across_channels() {
        let mut d = WaveDetector::default();
        let text = "check out my channel for free stuff";
        assert!(d.observe(&msg("aaa", "one", text, 0), CFG).is_empty());
        assert!(d.observe(&msg("aaa", "one", text, 1), CFG).is_empty()); // same user doesn't count twice
        assert!(d.observe(&msg("bbb", "two", text, 2), CFG).is_empty());
        let w = d.observe(&msg("ccc", "one", &format!("{text} 99"), 3), CFG);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, WaveKind::SameMessage);
        assert_eq!(w[0].members.len(), 3);
        assert_eq!(w[0].channels.len(), 2);
        assert_eq!(w[0].severity, crate::rules::Severity::High);
        // Later accounts join the existing wave instead of starting a new one.
        let grown = d.observe(&msg("ddd", "two", text, 10), CFG);
        assert_eq!(grown[0].id, w[0].id);
        assert_eq!(grown[0].members.len(), 4);
        assert_eq!(d.waves().len(), 1);
    }

    #[test]
    fn window_expires_and_short_messages_ignored() {
        let mut d = WaveDetector::default();
        let text = "this is a long enough message";
        d.observe(&msg("aaa", "one", text, 0), CFG);
        d.observe(&msg("bbb", "one", text, 1_000), CFG);
        assert!(d.observe(&msg("ccc", "one", text, 120_000), CFG).is_empty());
        for (i, u) in ["x1y", "x2y", "x3y", "x4y"].iter().enumerate() {
            assert!(d.observe(&msg(u, "one", "lol", i as i64), CFG).is_empty());
        }
    }

    #[test]
    fn similar_names_and_first_time_burst() {
        let mut d = WaveDetector::default();
        let mut found = vec![];
        for i in 0..3 {
            found = d.observe(&msg(&format!("hatebot_{i}"), "one", &format!("hello {i}"), i), CFG);
        }
        assert_eq!(found[0].kind, WaveKind::SimilarNames);
        assert_eq!(found[0].key, "hatebot");

        let mut d = WaveDetector::default();
        let mut last = vec![];
        for i in 0..6 {
            let mut m = msg(&format!("newbie{i}x"), "one", &format!("hi {i}"), i);
            m.is_first_msg = true;
            last = d.observe(&m, CFG);
        }
        assert_eq!(last[0].kind, WaveKind::FirstTimeBurst); // threshold is doubled: 6
    }

    #[test]
    fn mods_are_ignored_and_dismiss_suppresses() {
        let mut d = WaveDetector::default();
        let text = "same message from everyone here";
        let mut m = msg("modguy", "one", text, 0);
        m.is_mod = true;
        d.observe(&m, CFG);
        d.observe(&msg("aaa", "one", text, 1), CFG);
        assert!(d.observe(&msg("bbb", "one", text, 2), CFG).is_empty());
        let w = d.observe(&msg("ccc", "one", text, 3), CFG);
        let id = w[0].id;
        d.dismiss(id, 4).unwrap();
        for (i, u) in ["d1d", "e1e", "f1f", "g1g"].iter().enumerate() {
            assert!(d.observe(&msg(u, "one", text, 10 + i as i64), CFG).is_empty());
        }
    }
}
