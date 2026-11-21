//! Raid risk score: after a raid, look at the raiding channel and at who shows up in chat,
//! and say how likely it is to be a hate raid or bot raid.
//!
//! Twitch doesn't say which chatters came with a raid, so "raiders" here means people who
//! start chatting in the channel during the watch window and weren't seen there before it.

use crate::rules::Severity;
use crate::waves::name_stem;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// How long after a raid its chatters are watched.
pub const WATCH_MS: i64 = 90_000;
/// Accounts younger than this count as new.
const NEW_ACCOUNT_MS: i64 = 7 * 86_400_000;
/// Fewest raiders with known account ages before their ages count toward the score.
const MIN_AGE_SAMPLE: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RiskReason {
    pub text: String,
    /// Positive makes the raid riskier, negative is reassuring.
    pub weight: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RaidRisk {
    pub level: Severity,
    pub score: i32,
    pub reasons: Vec<RiskReason>,
    /// False while chat is still being watched.
    pub done: bool,
    pub raider_created_at: Option<i64>,
    pub raider_followers: Option<i64>,
    pub prior_raids: i64,
    /// New chatters seen during the window.
    pub chatters: usize,
    pub new_accounts: usize,
    pub known_ages: usize,
    pub first_timers: usize,
    pub flagged: usize,
    pub similar_names: usize,
    /// A spam-wave card with the suspicious raiders, for bulk actions.
    #[serde(default)]
    pub wave_id: Option<i64>,
}

/// What's known about the raiding channel.
#[derive(Debug, Clone, Default)]
pub struct RaiderInfo {
    pub created_at: Option<i64>,
    pub followers: Option<i64>,
    /// Earlier raids from this channel into any of yours.
    pub prior_raids: i64,
}

#[derive(Debug, Clone)]
pub struct Chatter {
    pub login: String,
    pub display_name: String,
    pub user_id: Option<String>,
    pub message_id: String,
    pub text: String,
    pub timestamp: i64,
    pub first_message: bool,
    /// Set off one of your active rules during the window.
    pub flagged: bool,
}

/// One raid being watched.
pub struct RaidWatch {
    pub raid_id: i64,
    pub channel: String,
    pub raider_login: String,
    pub started: i64,
    pub info: RaiderInfo,
    pub chatters: BTreeMap<String, Chatter>,
    pub last: Option<RaidRisk>,
    pub alerted_high: bool,
    pub wave_id: Option<i64>,
}

impl RaidWatch {
    pub fn new(raid_id: i64, channel: &str, raider_login: &str, started: i64, prior_raids: i64) -> Self {
        RaidWatch {
            raid_id,
            channel: channel.to_string(),
            raider_login: raider_login.to_string(),
            started,
            info: RaiderInfo { prior_raids, ..Default::default() },
            chatters: BTreeMap::new(),
            last: None,
            alerted_high: false,
            wave_id: None,
        }
    }

    pub fn active(&self, now: i64) -> bool {
        now - self.started < WATCH_MS
    }

    /// Logins whose names share a stem with at least two other raiders.
    pub fn similar_name_logins(&self) -> Vec<String> {
        let mut groups: HashMap<String, Vec<&str>> = HashMap::new();
        for login in self.chatters.keys() {
            if let Some(stem) = name_stem(login) {
                groups.entry(stem).or_default().push(login);
            }
        }
        groups.into_values().filter(|g| g.len() >= 3).flatten().map(str::to_string).collect()
    }

    /// Raiders worth acting on: new accounts, rule hits, or bot-like names.
    pub fn suspicious(&self, ages: &HashMap<String, i64>, now: i64) -> Vec<&Chatter> {
        let similar = self.similar_name_logins();
        self.chatters
            .values()
            .filter(|c| {
                c.flagged
                    || similar.contains(&c.login)
                    || ages.get(&c.login).is_some_and(|t| now - t < NEW_ACCOUNT_MS)
            })
            .collect()
    }

    pub fn assess(&self, ages: &HashMap<String, i64>, now: i64) -> RaidRisk {
        let known: Vec<i64> = self.chatters.keys().filter_map(|l| ages.get(l).copied()).collect();
        let stats = WindowStats {
            chatters: self.chatters.len(),
            known_ages: known.len(),
            new_accounts: known.iter().filter(|t| now - **t < NEW_ACCOUNT_MS).count(),
            first_timers: self.chatters.values().filter(|c| c.first_message).count(),
            flagged: self.chatters.values().filter(|c| c.flagged).count(),
            similar_names: self.similar_name_logins().len(),
        };
        let mut risk = assess(&self.info, &stats, now);
        risk.done = !self.active(now);
        risk.wave_id = self.wave_id;
        risk
    }
}

#[derive(Debug, Clone, Default)]
pub struct WindowStats {
    pub chatters: usize,
    pub known_ages: usize,
    pub new_accounts: usize,
    pub first_timers: usize,
    pub flagged: usize,
    pub similar_names: usize,
}

fn plural(n: impl Into<i64>, one: &str, many: &str) -> String {
    let n = n.into();
    format!("{n} {}", if n == 1 { one } else { many })
}

fn age_text(ms: i64) -> String {
    let days = ms / 86_400_000;
    match days {
        0 => "less than a day".into(),
        d if d < 60 => plural(d, "day", "days"),
        d if d < 730 => plural(d / 30, "month", "months"),
        d => plural(d / 365, "year", "years"),
    }
}

/// Score a raid from what's known so far. Higher is riskier.
pub fn assess(info: &RaiderInfo, s: &WindowStats, now: i64) -> RaidRisk {
    let mut reasons = Vec::new();
    let mut add = |weight: i32, text: String| reasons.push(RiskReason { text, weight });

    if let Some(created) = info.created_at {
        let age = now - created;
        if age < 7 * 86_400_000 {
            add(3, format!("The raiding channel is only {} old", age_text(age)));
        } else if age < 30 * 86_400_000 {
            add(2, format!("The raiding channel is only {} old", age_text(age)));
        } else if age >= 365 * 86_400_000 {
            add(-1, format!("The raiding channel is {} old", age_text(age)));
        }
    }
    if let Some(f) = info.followers {
        if f < 10 {
            add(2, format!("The raiding channel has only {}", plural(f, "follower", "followers")));
        } else if f < 50 {
            add(1, format!("The raiding channel has only {f} followers"));
        } else if f >= 1000 {
            add(-1, format!("The raiding channel has {f} followers"));
        }
    }
    if info.prior_raids > 0 {
        add(-2, format!("They've raided your channels {} before", plural(info.prior_raids, "time", "times")));
    }
    if s.known_ages >= MIN_AGE_SAMPLE {
        let share = s.new_accounts as f64 / s.known_ages as f64;
        let text = format!("{} of {} raiders checked have accounts under a week old", s.new_accounts, s.known_ages);
        if share >= 0.5 {
            add(4, text);
        } else if share >= 0.25 {
            add(2, text);
        }
    }
    if s.similar_names >= 3 {
        add(3, format!("{} raiders have near-identical names", s.similar_names));
    }
    if s.flagged >= 3 {
        add(3, format!("{} raiders set off your rules", s.flagged));
    } else if s.flagged > 0 {
        add(1, format!("{} set off your rules", plural(s.flagged as i64, "raider", "raiders")));
    }

    let score: i32 = reasons.iter().map(|r| r.weight).sum();
    let level = match score {
        s if s >= 5 => Severity::High,
        s if s >= 2 => Severity::Medium,
        _ => Severity::Low,
    };
    reasons.sort_by_key(|r| -r.weight);
    RaidRisk {
        level,
        score,
        reasons,
        done: false,
        raider_created_at: info.created_at,
        raider_followers: info.followers,
        prior_raids: info.prior_raids,
        chatters: s.chatters,
        new_accounts: s.new_accounts,
        known_ages: s.known_ages,
        first_timers: s.first_timers,
        flagged: s.flagged,
        similar_names: s.similar_names,
        wave_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_000 * DAY;

    #[test]
    fn friendly_raid_from_a_known_channel_is_low() {
        let info = RaiderInfo { created_at: Some(NOW - 900 * DAY), followers: Some(5_000), prior_raids: 3 };
        let s = WindowStats { chatters: 30, known_ages: 25, new_accounts: 1, first_timers: 20, ..Default::default() };
        let r = assess(&info, &s, NOW);
        assert_eq!(r.level, Severity::Low);
        assert!(r.score < 0);
    }

    #[test]
    fn brand_new_channel_with_new_accounts_is_high() {
        let info = RaiderInfo { created_at: Some(NOW - 2 * DAY), followers: Some(1), prior_raids: 0 };
        let s = WindowStats { chatters: 12, known_ages: 10, new_accounts: 8, flagged: 4, ..Default::default() };
        let r = assess(&info, &s, NOW);
        assert_eq!(r.level, Severity::High);
        assert_eq!(r.reasons[0].weight, 4); // strongest reason first
        assert!(r.reasons.iter().any(|x| x.text.contains("2 days old")));
    }

    #[test]
    fn small_age_samples_are_ignored() {
        let info = RaiderInfo::default();
        let s = WindowStats { chatters: 3, known_ages: 3, new_accounts: 3, ..Default::default() };
        assert_eq!(assess(&info, &s, NOW).level, Severity::Low);
    }

    #[test]
    fn watch_tracks_bot_names_and_suspicious_raiders() {
        let mut w = RaidWatch::new(1, "chan", "raider", NOW, 0);
        for (i, login) in ["hatebot_1", "hatebot_2", "hatebot_3", "normalviewer", "oldfriend"].iter().enumerate() {
            w.chatters.insert(
                login.to_string(),
                Chatter {
                    login: login.to_string(),
                    display_name: login.to_string(),
                    user_id: None,
                    message_id: format!("m{i}"),
                    text: "hi".into(),
                    timestamp: NOW,
                    first_message: true,
                    flagged: false,
                },
            );
        }
        let mut similar = w.similar_name_logins();
        similar.sort();
        assert_eq!(similar, vec!["hatebot_1", "hatebot_2", "hatebot_3"]);
        let ages: HashMap<String, i64> =
            [("normalviewer".to_string(), NOW - DAY), ("oldfriend".to_string(), NOW - 800 * DAY)].into_iter().collect();
        let sus: Vec<&str> = w.suspicious(&ages, NOW).iter().map(|c| c.login.as_str()).collect();
        assert_eq!(sus, vec!["hatebot_1", "hatebot_2", "hatebot_3", "normalviewer"]);
        let r = w.assess(&ages, NOW + WATCH_MS);
        assert!(r.done);
        assert_eq!(r.similar_names, 3);
    }
}
