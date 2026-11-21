//! SQLite storage for channels, rules, flags, raids and settings.

use crate::alerts::ChannelAlertLevel;
use crate::irc::RaidNotice;
use crate::rules::{Rule, RuleKind, RuleMatch, Severity};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub type DbResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub name: String,
    /// True when alerts are off for this channel (kept for the quick bell toggle).
    pub muted: bool,
    /// Twitch ID of the channel, known once mod status has been verified.
    pub broadcaster_id: Option<String>,
    /// Whether the logged-in user moderates this channel (as of the last check).
    pub is_mod: bool,
    pub alert_level: ChannelAlertLevel,
    /// Play sounds for this channel's alerts.
    pub sound: bool,
    /// Tag color, e.g. "#4dabf7".
    pub color: Option<String>,
    /// Optional chat message sent with a shoutout, e.g. "!so @{raider}".
    pub shoutout_message: String,
}

/// Distinct tag colors, handed out in order so channels don't share one.
pub const CHANNEL_PALETTE: &[&str] = &[
    "#4dabf7", "#ff8787", "#69db7c", "#ffd43b", "#b197fc", "#ffa94d", "#3bc9db", "#f783ac", "#a9e34b", "#e599f7", "#74c0fc",
    "#ffc078",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLogEntry {
    pub id: i64,
    pub channel: String,
    pub moderator: String,
    pub action: String,
    pub target_login: Option<String>,
    pub summary: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: i64,
    pub channel: String,
    /// "follow", "sub", "resub", "subgift", "giftbomb", "upgrade", "bits", "announcement", "other"
    pub kind: String,
    pub user_login: String,
    pub display_name: String,
    pub text: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextLine {
    pub display_name: String,
    pub text: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    pub id: i64,
    pub channel: String,
    pub message_id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub user_login: String,
    pub display_name: String,
    pub text: String,
    pub severity: Severity,
    pub matches: Vec<RuleMatch>,
    pub context: Vec<ContextLine>,
    /// Messages sent in the channel right after the flagged one.
    #[serde(default)]
    pub context_after: Vec<ContextLine>,
    pub timestamp: i64,
    /// "open", "reviewed" or "dismissed"
    pub status: String,
    /// What was done about it, e.g. "Timed out 10m".
    #[serde(default)]
    pub resolution: Option<String>,
    /// "rule", "automod", "unbanRequest" or "suspicious".
    #[serde(default = "rule_kind")]
    pub kind: String,
    /// AutoMod message ID or unban request ID, for kinds that come from Twitch.
    #[serde(default)]
    pub external_id: Option<String>,
    /// Extra details shown on the card (AutoMod category, account age, ban-evasion rating…).
    #[serde(default)]
    pub meta: serde_json::Value,
}

fn rule_kind() -> String {
    "rule".into()
}

/// One step of the escalation ladder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LadderStep {
    /// "warn", "timeout" or "ban"
    pub action: String,
    #[serde(default)]
    pub duration_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModAction {
    pub id: i64,
    pub channel: String,
    pub target_login: String,
    /// "delete", "timeout", "ban" or "unban"
    pub action: String,
    pub duration_secs: Option<i64>,
    pub reason: String,
    pub ok: bool,
    pub error: Option<String>,
    pub timestamp: i64,
    /// False when another moderator did it (seen in chat).
    #[serde(default = "yes")]
    pub by_app: bool,
    #[serde(default)]
    pub target_user_id: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Raid {
    pub id: i64,
    pub channel: String,
    pub from_login: String,
    pub from_display_name: String,
    pub viewers: i64,
    pub timestamp: i64,
    pub acknowledged: bool,
    #[serde(default)]
    pub from_user_id: Option<String>,
    /// When you sent a shoutout for this raid, if you did.
    #[serde(default)]
    pub shoutout_at: Option<i64>,
    /// How likely this is a hate or bot raid (filled in during the first 90 seconds).
    #[serde(default)]
    pub risk: Option<crate::raidwatch::RaidRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub notify_flags: bool,
    pub notify_min_severity: Severity,
    pub notify_raids: bool,
    pub raid_min_viewers: i64,
    pub play_sounds: bool,
    /// Client ID of the user's Twitch developer application.
    pub twitch_client_id: String,
    /// How long raid mode lasts after a raid, in minutes. 0 turns it off.
    pub raid_mode_minutes: i64,
    /// Suggested actions for a user's 1st, 2nd, 3rd… offense in a channel.
    pub ladder: Vec<LadderStep>,
    /// Offenses older than this don't count toward the ladder.
    pub ladder_lookback_days: i64,
    /// Accounts needed before a spam wave is flagged.
    pub wave_min_accounts: i64,
    pub wave_window_secs: i64,
    /// Flag new chatters whose names look like recently banned users.
    pub evasion_check: bool,
    /// Detect several chatters going after one person.
    pub pile_on_enabled: bool,
    pub pile_on_min_users: i64,
    pub pile_on_window_secs: i64,
    /// Run the bundled toxicity model on chat (uses some CPU; off by default).
    pub toxicity_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            notify_flags: true,
            notify_min_severity: Severity::Medium,
            notify_raids: true,
            raid_min_viewers: 0,
            play_sounds: true,
            twitch_client_id: String::new(),
            raid_mode_minutes: 5,
            ladder: vec![
                LadderStep { action: "warn".into(), duration_secs: None },
                LadderStep { action: "timeout".into(), duration_secs: Some(600) },
                LadderStep { action: "timeout".into(), duration_secs: Some(86_400) },
                LadderStep { action: "ban".into(), duration_secs: None },
            ],
            ladder_lookback_days: 30,
            wave_min_accounts: 5,
            wave_window_secs: 60,
            evasion_check: true,
            pile_on_enabled: true,
            pile_on_min_users: 4,
            pile_on_window_secs: 120,
            toxicity_enabled: false,
        }
    }
}

/// What a rule in test mode would have flagged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleHit {
    pub rule_id: i64,
    pub channel: String,
    pub user_login: String,
    pub display_name: String,
    pub text: String,
    pub terms: Vec<String>,
    pub reason: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatternStats {
    pub pattern: String,
    pub flags: i64,
    pub actioned: i64,
    pub dismissed: i64,
}

/// How a rule has performed over a time window.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuleStats {
    pub rule_id: i64,
    pub flags: i64,
    /// Ended in a delete, warn, timeout or ban (by you or another mod).
    pub actioned: i64,
    /// Marked reviewed with no action.
    pub reviewed: i64,
    /// Dismissed: a false alarm.
    pub dismissed: i64,
    pub open: i64,
    pub last_fired: Option<i64>,
    pub patterns: Vec<PatternStats>,
    /// Test mode: messages it would have flagged.
    pub test_hits: i64,
    pub test_samples: Vec<RuleHit>,
}

#[derive(PartialEq)]
enum Outcome {
    Actioned,
    Reviewed,
    Dismissed,
    Open,
}

fn outcome(status: &str, resolution: Option<&str>) -> Outcome {
    match (status, resolution) {
        ("open", _) => Outcome::Open,
        ("dismissed", _) => Outcome::Dismissed,
        (_, Some(r)) if !r.is_empty() => Outcome::Actioned,
        _ => Outcome::Reviewed,
    }
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> DbResult<Self> {
        Self::init(Connection::open(path).map_err(err)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> DbResult<Self> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> DbResult<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS channels (
                 name TEXT PRIMARY KEY,
                 muted INTEGER NOT NULL DEFAULT 0,
                 added_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS rules (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 data TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS flags (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 channel TEXT NOT NULL,
                 message_id TEXT NOT NULL,
                 user_login TEXT NOT NULL,
                 display_name TEXT NOT NULL,
                 text TEXT NOT NULL,
                 severity TEXT NOT NULL,
                 matches TEXT NOT NULL,
                 context TEXT NOT NULL,
                 ts INTEGER NOT NULL,
                 status TEXT NOT NULL DEFAULT 'open'
             );
             CREATE INDEX IF NOT EXISTS flags_status_ts ON flags(status, ts);
             CREATE TABLE IF NOT EXISTS raids (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 channel TEXT NOT NULL,
                 from_login TEXT NOT NULL,
                 from_display_name TEXT NOT NULL,
                 viewers INTEGER NOT NULL,
                 ts INTEGER NOT NULL,
                 acknowledged INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS settings (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )
        .map_err(err)?;
        let db = Db { conn };
        db.migrate()?;
        db.seed_defaults()?;
        Ok(db)
    }

    fn migrate(&self) -> DbResult<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
        if version < 2 {
            self.conn
                .execute_batch(
                    "BEGIN;
                     ALTER TABLE channels ADD COLUMN broadcaster_id TEXT;
                     ALTER TABLE channels ADD COLUMN is_mod INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE flags ADD COLUMN user_id TEXT;
                     ALTER TABLE flags ADD COLUMN context_after TEXT NOT NULL DEFAULT '[]';
                     ALTER TABLE flags ADD COLUMN resolution TEXT;
                     CREATE INDEX IF NOT EXISTS flags_user ON flags(channel, user_login);
                     CREATE TABLE IF NOT EXISTS mod_actions (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         channel TEXT NOT NULL,
                         target_login TEXT NOT NULL,
                         action TEXT NOT NULL,
                         duration_secs INTEGER,
                         reason TEXT NOT NULL DEFAULT '',
                         ok INTEGER NOT NULL,
                         error TEXT,
                         ts INTEGER NOT NULL
                     );
                     CREATE INDEX IF NOT EXISTS mod_actions_user ON mod_actions(channel, target_login);
                     PRAGMA user_version = 2;
                     COMMIT;",
                )
                .map_err(err)?;
        }
        if version < 3 {
            self.conn
                .execute_batch(
                    "BEGIN;
                     ALTER TABLE flags ADD COLUMN kind TEXT NOT NULL DEFAULT 'rule';
                     ALTER TABLE flags ADD COLUMN external_id TEXT;
                     ALTER TABLE flags ADD COLUMN meta TEXT NOT NULL DEFAULT '{}';
                     CREATE INDEX IF NOT EXISTS flags_external ON flags(kind, external_id);
                     ALTER TABLE mod_actions ADD COLUMN by_app INTEGER NOT NULL DEFAULT 1;
                     ALTER TABLE mod_actions ADD COLUMN target_user_id TEXT;
                     CREATE INDEX IF NOT EXISTS mod_actions_ts ON mod_actions(ts);
                     PRAGMA user_version = 3;
                     COMMIT;",
                )
                .map_err(err)?;
        }
        if version < 4 {
            self.conn
                .execute_batch(
                    "BEGIN;
                     ALTER TABLE flags ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
                     CREATE INDEX IF NOT EXISTS flags_kind_ts ON flags(kind, ts);
                     CREATE TABLE IF NOT EXISTS rule_hits (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         rule_id INTEGER NOT NULL,
                         channel TEXT NOT NULL,
                         user_login TEXT NOT NULL,
                         display_name TEXT NOT NULL,
                         text TEXT NOT NULL,
                         terms TEXT NOT NULL,
                         reason TEXT,
                         ts INTEGER NOT NULL
                     );
                     CREATE INDEX IF NOT EXISTS rule_hits_rule_ts ON rule_hits(rule_id, ts);
                     PRAGMA user_version = 4;
                     COMMIT;",
                )
                .map_err(err)?;
        }
        if version < 5 {
            self.conn
                .execute_batch(
                    "BEGIN;
                     ALTER TABLE channels ADD COLUMN alert_level TEXT NOT NULL DEFAULT 'default';
                     ALTER TABLE channels ADD COLUMN sound INTEGER NOT NULL DEFAULT 1;
                     ALTER TABLE channels ADD COLUMN color TEXT;
                     ALTER TABLE channels ADD COLUMN shoutout_message TEXT NOT NULL DEFAULT '';
                     UPDATE channels SET alert_level = 'off' WHERE muted = 1;
                     ALTER TABLE raids ADD COLUMN from_user_id TEXT;
                     ALTER TABLE raids ADD COLUMN shoutout_at INTEGER;
                     CREATE TABLE IF NOT EXISTS mod_log (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         channel TEXT NOT NULL,
                         moderator TEXT NOT NULL,
                         action TEXT NOT NULL,
                         target_login TEXT,
                         summary TEXT NOT NULL,
                         ts INTEGER NOT NULL
                     );
                     CREATE INDEX IF NOT EXISTS mod_log_ts ON mod_log(ts);
                     CREATE TABLE IF NOT EXISTS activity (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         channel TEXT NOT NULL,
                         kind TEXT NOT NULL,
                         user_login TEXT NOT NULL,
                         display_name TEXT NOT NULL,
                         text TEXT NOT NULL,
                         ts INTEGER NOT NULL
                     );
                     CREATE INDEX IF NOT EXISTS activity_ts ON activity(ts);
                     PRAGMA user_version = 5;
                     COMMIT;",
                )
                .map_err(err)?;
        }
        if version < 6 {
            self.conn
                .execute_batch(
                    "BEGIN;
                     ALTER TABLE raids ADD COLUMN risk TEXT;
                     CREATE INDEX IF NOT EXISTS mod_actions_action_ts ON mod_actions(action, ts);
                     PRAGMA user_version = 6;
                     COMMIT;",
                )
                .map_err(err)?;
        }
        self.assign_missing_colors()?;
        Ok(())
    }

    /// On first run, add a starter rule so the app does something useful immediately.
    fn seed_defaults(&self) -> DbResult<()> {
        let seeded: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'seeded'", [], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if seeded.is_none() {
            self.seed_starter()?;
        }
        self.seed_packs()?;
        self.upgrade_packs()?;
        self.add_toxicity_rule()?;
        Ok(())
    }

    /// Add the default rule packs once (in test mode).
    fn seed_packs(&self) -> DbResult<()> {
        let done: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'seeded_packs_v1'", [], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if done.is_some() {
            return Ok(());
        }
        self.restore_default_packs()?;
        self.conn
            .execute("INSERT INTO settings (key, value) VALUES ('seeded_packs_v1', '1')", [])
            .map_err(err)?;
        Ok(())
    }

    /// Apply fixes to default pack rules from older versions (once).
    fn upgrade_packs(&self) -> DbResult<()> {
        let done: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'packs_v2'", [], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if done.is_some() {
            return Ok(());
        }
        for r in crate::packs::upgrade_pack_rules(&self.rules()?) {
            self.save_rule(&r)?;
        }
        self.conn.execute("INSERT INTO settings (key, value) VALUES ('packs_v2', '1')", []).map_err(err)?;
        Ok(())
    }

    /// Add the on-device toxicity rule for people who installed before it existed (once).
    fn add_toxicity_rule(&self) -> DbResult<()> {
        let done: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'packs_v3'", [], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if done.is_some() {
            return Ok(());
        }
        let r = crate::packs::toxicity_rule();
        if !self.rules()?.iter().any(|x| x.pack == r.pack && x.name == r.name) {
            self.save_rule(&r)?;
        }
        self.conn.execute("INSERT INTO settings (key, value) VALUES ('packs_v3', '1')", []).map_err(err)?;
        Ok(())
    }

    /// Add any default pack rule that isn't already present (matched by pack and name). Returns how many were added.
    pub fn restore_default_packs(&self) -> DbResult<usize> {
        let existing: Vec<(Option<String>, String)> = self.rules()?.into_iter().map(|r| (r.pack, r.name)).collect();
        let mut added = 0;
        for r in crate::packs::default_packs() {
            if !existing.iter().any(|(p, n)| p == &r.pack && n == &r.name) {
                self.save_rule(&r)?;
                added += 1;
            }
        }
        Ok(added)
    }

    fn seed_starter(&self) -> DbResult<()> {
        let mut starter = crate::packs::base("Follower/viewer bot spam", RuleKind::Keyword, Severity::High);
        starter.patterns = ["free followers", "cheap viewers", "best viewers", "buy followers", "primes and viewers"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        self.save_rule(&starter)?;
        self.conn
            .execute("INSERT INTO settings (key, value) VALUES ('seeded', '1')", [])
            .map_err(err)?;
        Ok(())
    }

    // ---- channels ----

    pub fn channels(&self) -> DbResult<Vec<Channel>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, broadcaster_id, is_mod, alert_level, sound, color, shoutout_message
                 FROM channels ORDER BY added_at",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                let level = ChannelAlertLevel::parse(&r.get::<_, String>(3)?);
                Ok(Channel {
                    name: r.get(0)?,
                    muted: level == ChannelAlertLevel::Off,
                    broadcaster_id: r.get(1)?,
                    is_mod: r.get::<_, i64>(2)? != 0,
                    alert_level: level,
                    sound: r.get::<_, i64>(4)? != 0,
                    color: r.get(5)?,
                    shoutout_message: r.get(6)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn add_channel(&self, name: &str, broadcaster_id: &str, now: i64) -> DbResult<()> {
        self.conn
            .execute(
                "INSERT INTO channels (name, muted, added_at, broadcaster_id, is_mod) VALUES (?1, 0, ?2, ?3, 1)
                 ON CONFLICT(name) DO UPDATE SET broadcaster_id = excluded.broadcaster_id, is_mod = 1",
                params![name, now, broadcaster_id],
            )
            .map_err(err)?;
        self.assign_missing_colors()?;
        Ok(())
    }

    /// Give every channel without a color the first palette color not already used.
    fn assign_missing_colors(&self) -> DbResult<()> {
        let rows: Vec<(String, Option<String>)> = {
            let mut stmt = self.conn.prepare("SELECT name, color FROM channels ORDER BY added_at").map_err(err)?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        let mut used: Vec<String> = rows.iter().filter_map(|(_, c)| c.clone()).collect();
        for (name, color) in rows {
            if color.is_some() {
                continue;
            }
            let pick = CHANNEL_PALETTE
                .iter()
                .find(|c| !used.iter().any(|u| u.eq_ignore_ascii_case(c)))
                .copied()
                // More channels than colors: reuse the least-used one.
                .unwrap_or_else(|| {
                    *CHANNEL_PALETTE
                        .iter()
                        .min_by_key(|c| used.iter().filter(|u| u.eq_ignore_ascii_case(c)).count())
                        .unwrap()
                });
            self.conn
                .execute("UPDATE channels SET color = ?2 WHERE name = ?1", params![name, pick])
                .map_err(err)?;
            used.push(pick.to_string());
        }
        Ok(())
    }

    pub fn set_channel_alerts(&self, name: &str, level: ChannelAlertLevel, sound: bool) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE channels SET alert_level = ?2, sound = ?3, muted = ?4 WHERE name = ?1",
                params![name, level.as_str(), sound as i64, (level == ChannelAlertLevel::Off) as i64],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Set a channel's color, or pick a fresh one from the palette when `color` is None.
    pub fn set_channel_color(&self, name: &str, color: Option<&str>) -> DbResult<()> {
        if let Some(c) = color {
            let valid = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
            if !valid {
                return Err("Colors look like #4dabf7.".into());
            }
        }
        self.conn.execute("UPDATE channels SET color = ?2 WHERE name = ?1", params![name, color]).map_err(err)?;
        self.assign_missing_colors()
    }

    pub fn set_shoutout_message(&self, name: &str, message: &str) -> DbResult<()> {
        let message: String = message.trim().chars().take(400).collect();
        self.conn
            .execute("UPDATE channels SET shoutout_message = ?2 WHERE name = ?1", params![name, message])
            .map_err(err)?;
        Ok(())
    }

    /// The channel's alert level and whether its alerts play a sound.
    pub fn channel_alerts(&self, name: &str) -> (ChannelAlertLevel, bool) {
        self.conn
            .query_row("SELECT alert_level, sound FROM channels WHERE name = ?1", params![name], |r| {
                Ok((ChannelAlertLevel::parse(&r.get::<_, String>(0)?), r.get::<_, i64>(1)? != 0))
            })
            .unwrap_or((ChannelAlertLevel::Default, true))
    }

    /// Record whether the user moderates `name`, as reported by Twitch.
    pub fn set_channel_mod_status(&self, name: &str, broadcaster_id: Option<&str>, is_mod: bool) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE channels SET is_mod = ?2, broadcaster_id = COALESCE(?3, broadcaster_id) WHERE name = ?1",
                params![name, is_mod as i64, broadcaster_id],
            )
            .map_err(err)?;
        Ok(())
    }

    pub fn broadcaster_id(&self, name: &str) -> Option<String> {
        self.conn
            .query_row("SELECT broadcaster_id FROM channels WHERE name = ?1", params![name], |r| r.get(0))
            .ok()
            .flatten()
    }

    pub fn remove_channel(&self, name: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM channels WHERE name = ?1", params![name]).map_err(err)?;
        Ok(())
    }

    /// The quick bell toggle: alerts off, or back to the default level.
    pub fn set_channel_muted(&self, name: &str, muted: bool) -> DbResult<()> {
        let (_, sound) = self.channel_alerts(name);
        let level = if muted { ChannelAlertLevel::Off } else { ChannelAlertLevel::Default };
        self.set_channel_alerts(name, level, sound)
    }

    #[cfg(test)]
    pub fn is_muted(&self, name: &str) -> bool {
        self.channel_alerts(name).0 == ChannelAlertLevel::Off
    }

    // ---- rules ----

    pub fn rules(&self) -> DbResult<Vec<Rule>> {
        let mut stmt = self.conn.prepare("SELECT id, data FROM rules ORDER BY id").map_err(err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, data) = row.map_err(err)?;
            if let Ok(mut rule) = serde_json::from_str::<Rule>(&data) {
                rule.id = id;
                out.push(rule);
            }
        }
        Ok(out)
    }

    /// Insert (id == 0) or update a rule; returns its id.
    pub fn save_rule(&self, rule: &Rule) -> DbResult<i64> {
        let data = serde_json::to_string(rule).map_err(err)?;
        if rule.id == 0 {
            self.conn.execute("INSERT INTO rules (data) VALUES (?1)", params![data]).map_err(err)?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn
                .execute("UPDATE rules SET data = ?2 WHERE id = ?1", params![rule.id, data])
                .map_err(err)?;
            Ok(rule.id)
        }
    }

    pub fn delete_rule(&self, id: i64) -> DbResult<()> {
        self.conn.execute("DELETE FROM rules WHERE id = ?1", params![id]).map_err(err)?;
        Ok(())
    }

    // ---- flags ----

    pub fn insert_flag(&self, flag: &Flag) -> DbResult<i64> {
        self.conn
            .execute(
                "INSERT INTO flags (channel, message_id, user_login, display_name, text, severity, matches, context, ts, status, user_id, context_after, resolution, kind, external_id, meta)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    flag.channel,
                    flag.message_id,
                    flag.user_login,
                    flag.display_name,
                    flag.text,
                    serde_json::to_string(&flag.severity).map_err(err)?,
                    serde_json::to_string(&flag.matches).map_err(err)?,
                    serde_json::to_string(&flag.context).map_err(err)?,
                    flag.timestamp,
                    flag.status,
                    flag.user_id,
                    serde_json::to_string(&flag.context_after).map_err(err)?,
                    flag.resolution,
                    flag.kind,
                    flag.external_id,
                    serde_json::to_string(&flag.meta).map_err(err)?,
                ],
            )
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn flags(&self, limit: i64) -> DbResult<Vec<Flag>> {
        self.query_flags("archived = 0", params![limit])
    }

    pub fn flags_for_user(&self, channel: &str, login: &str, limit: i64) -> DbResult<Vec<Flag>> {
        self.query_flags("archived = 0 AND channel = ?2 AND user_login = ?3", params![limit, channel, login])
    }

    fn query_flags(&self, filter: &str, args: impl rusqlite::Params) -> DbResult<Vec<Flag>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT id, channel, message_id, user_login, display_name, text, severity, matches, context, ts, status,
                        user_id, context_after, resolution, kind, external_id, meta
                 FROM flags WHERE {filter} ORDER BY ts DESC, id DESC LIMIT ?1"
            ))
            .map_err(err)?;
        let rows = stmt
            .query_map(args, |r| {
                let severity: String = r.get(6)?;
                let matches: String = r.get(7)?;
                let context: String = r.get(8)?;
                let after: String = r.get(12)?;
                Ok(Flag {
                    id: r.get(0)?,
                    channel: r.get(1)?,
                    message_id: r.get(2)?,
                    user_login: r.get(3)?,
                    display_name: r.get(4)?,
                    text: r.get(5)?,
                    severity: serde_json::from_str(&severity).unwrap_or(Severity::Medium),
                    matches: serde_json::from_str(&matches).unwrap_or_default(),
                    context: serde_json::from_str(&context).unwrap_or_default(),
                    timestamp: r.get(9)?,
                    status: r.get(10)?,
                    user_id: r.get(11)?,
                    context_after: serde_json::from_str(&after).unwrap_or_default(),
                    resolution: r.get(13)?,
                    kind: r.get(14)?,
                    external_id: r.get(15)?,
                    meta: serde_json::from_str(&r.get::<_, String>(16)?).unwrap_or_default(),
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn append_context_after(&self, flag_id: i64, line: &ContextLine) -> DbResult<()> {
        let current: String = self
            .conn
            .query_row("SELECT context_after FROM flags WHERE id = ?1", params![flag_id], |r| r.get(0))
            .optional()
            .map_err(err)?
            .unwrap_or_else(|| "[]".into());
        let mut lines: Vec<ContextLine> = serde_json::from_str(&current).unwrap_or_default();
        lines.push(line.clone());
        self.conn
            .execute(
                "UPDATE flags SET context_after = ?2 WHERE id = ?1",
                params![flag_id, serde_json::to_string(&lines).map_err(err)?],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Mark every open flag for this message as reviewed with a note about the action taken.
    pub fn resolve_flags(&self, channel: &str, user_login: &str, message_id: Option<&str>, resolution: &str) -> DbResult<Vec<i64>> {
        let ids: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id FROM flags WHERE channel = ?1 AND user_login = ?2 AND status = 'open'
                     AND (?3 IS NULL OR message_id = ?3)",
                )
                .map_err(err)?;
            let rows = stmt.query_map(params![channel, user_login, message_id], |r| r.get(0)).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        for id in &ids {
            self.conn
                .execute("UPDATE flags SET status = 'reviewed', resolution = ?2 WHERE id = ?1", params![id, resolution])
                .map_err(err)?;
        }
        Ok(ids)
    }

    pub fn flag_by_id(&self, id: i64) -> DbResult<Option<Flag>> {
        Ok(self.query_flags("id = ?2", params![1, id])?.into_iter().next())
    }

    /// The open rule flag for a chat message, if there is one.
    pub fn open_flag_for_message(&self, channel: &str, message_id: &str) -> Option<Flag> {
        self.query_flags("kind = 'rule' AND status = 'open' AND channel = ?2 AND message_id = ?3", params![1, channel, message_id])
            .ok()?
            .into_iter()
            .next()
    }

    pub fn update_flag_matches(&self, id: i64, severity: Severity, matches: &[RuleMatch]) -> DbResult<()> {
        self.conn
            .execute(
                "UPDATE flags SET severity = ?2, matches = ?3 WHERE id = ?1",
                params![id, serde_json::to_string(&severity).map_err(err)?, serde_json::to_string(matches).map_err(err)?],
            )
            .map_err(err)?;
        Ok(())
    }

    pub fn open_flag_exists(&self, kind: &str, channel: &str, login: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM flags WHERE kind = ?1 AND channel = ?2 AND user_login = ?3 AND status = 'open' LIMIT 1",
                params![kind, channel, login],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// ID of the flag for a Twitch item (AutoMod message, unban request), if it was already added.
    pub fn flag_by_external(&self, kind: &str, external_id: &str) -> Option<i64> {
        self.conn
            .query_row(
                "SELECT id FROM flags WHERE kind = ?1 AND external_id = ?2",
                params![kind, external_id],
                |r| r.get(0),
            )
            .ok()
    }

    /// Close the open flag for a Twitch item. Returns its ID if one was closed.
    pub fn resolve_external(&self, kind: &str, external_id: &str, resolution: &str) -> DbResult<Option<i64>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM flags WHERE kind = ?1 AND external_id = ?2 AND status = 'open'",
                params![kind, external_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if let Some(id) = id {
            self.conn
                .execute("UPDATE flags SET status = 'reviewed', resolution = ?2 WHERE id = ?1", params![id, resolution])
                .map_err(err)?;
        }
        Ok(id)
    }

    pub fn set_flag_status(&self, id: i64, status: &str) -> DbResult<()> {
        if !matches!(status, "open" | "reviewed" | "dismissed") {
            return Err(format!("Unknown flag status `{status}`"));
        }
        self.conn
            .execute("UPDATE flags SET status = ?2 WHERE id = ?1", params![id, status])
            .map_err(err)?;
        Ok(())
    }

    /// Hide every flag that is no longer open. They're kept (archived) for rule tuning stats.
    pub fn clear_closed_flags(&self) -> DbResult<usize> {
        self.conn
            .execute("UPDATE flags SET archived = 1 WHERE status != 'open' AND archived = 0", [])
            .map_err(err)
    }

    // ---- rule tuning ----

    pub fn insert_rule_hit(&self, hit: &RuleHit) -> DbResult<()> {
        self.conn
            .execute(
                "INSERT INTO rule_hits (rule_id, channel, user_login, display_name, text, terms, reason, ts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    hit.rule_id,
                    hit.channel,
                    hit.user_login,
                    hit.display_name,
                    hit.text,
                    serde_json::to_string(&hit.terms).map_err(err)?,
                    hit.reason,
                    hit.timestamp
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Drop test-mode hits older than `before_ms`, and archived flags older than that too.
    pub fn prune_tuning_data(&self, before_ms: i64) -> DbResult<()> {
        self.conn.execute("DELETE FROM rule_hits WHERE ts < ?1", params![before_ms]).map_err(err)?;
        self.conn
            .execute("DELETE FROM flags WHERE archived = 1 AND ts < ?1", params![before_ms])
            .map_err(err)?;
        Ok(())
    }

    /// Per-rule and per-pattern outcomes since `since_ms`, plus test-mode hits and samples.
    pub fn rule_stats(&self, since_ms: i64, samples_per_rule: i64) -> DbResult<Vec<RuleStats>> {
        let mut by_rule: std::collections::BTreeMap<i64, RuleStats> = std::collections::BTreeMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT matches, status, resolution, ts FROM flags WHERE kind = 'rule' AND ts >= ?1")
                .map_err(err)?;
            let rows = stmt
                .query_map(params![since_ms], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
                })
                .map_err(err)?;
            for row in rows {
                let (matches, status, resolution, ts) = row.map_err(err)?;
                let matches: Vec<RuleMatch> = serde_json::from_str(&matches).unwrap_or_default();
                let o = outcome(&status, resolution.as_deref());
                for m in matches {
                    let st = by_rule.entry(m.rule_id).or_insert_with(|| RuleStats { rule_id: m.rule_id, ..Default::default() });
                    st.flags += 1;
                    match o {
                        Outcome::Actioned => st.actioned += 1,
                        Outcome::Reviewed => st.reviewed += 1,
                        Outcome::Dismissed => st.dismissed += 1,
                        Outcome::Open => st.open += 1,
                    }
                    st.last_fired = Some(st.last_fired.map_or(ts, |t| t.max(ts)));
                    for p in m.patterns {
                        let ps = match st.patterns.iter_mut().find(|x| x.pattern == p) {
                            Some(x) => x,
                            None => {
                                st.patterns.push(PatternStats { pattern: p.clone(), ..Default::default() });
                                st.patterns.last_mut().unwrap()
                            }
                        };
                        ps.flags += 1;
                        if o == Outcome::Actioned {
                            ps.actioned += 1;
                        }
                        if o == Outcome::Dismissed {
                            ps.dismissed += 1;
                        }
                    }
                }
            }
        }
        let hit_counts: Vec<(i64, i64)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT rule_id, COUNT(*) FROM rule_hits WHERE ts >= ?1 GROUP BY rule_id")
                .map_err(err)?;
            let rows = stmt.query_map(params![since_ms], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
            rows.collect::<Result<_, _>>().map_err(err)?
        };
        for (rule_id, n) in hit_counts {
            let st = by_rule.entry(rule_id).or_insert_with(|| RuleStats { rule_id, ..Default::default() });
            st.test_hits = n;
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT channel, user_login, display_name, text, terms, reason, ts FROM rule_hits
                     WHERE rule_id = ?1 AND ts >= ?2 ORDER BY ts DESC LIMIT ?3",
                )
                .map_err(err)?;
            let rows = stmt
                .query_map(params![rule_id, since_ms, samples_per_rule], |r| {
                    Ok(RuleHit {
                        rule_id,
                        channel: r.get(0)?,
                        user_login: r.get(1)?,
                        display_name: r.get(2)?,
                        text: r.get(3)?,
                        terms: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                        reason: r.get(5)?,
                        timestamp: r.get(6)?,
                    })
                })
                .map_err(err)?;
            st.test_samples = rows.collect::<Result<_, _>>().map_err(err)?;
        }
        for st in by_rule.values_mut() {
            st.patterns.sort_by(|a, b| b.flags.cmp(&a.flags));
        }
        Ok(by_rule.into_values().collect())
    }

    // ---- raids ----

    pub fn insert_raid(&self, raid: &RaidNotice) -> DbResult<Raid> {
        self.conn
            .execute(
                "INSERT INTO raids (channel, from_login, from_display_name, viewers, ts, from_user_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![raid.channel, raid.from_login, raid.from_display_name, raid.viewers, raid.timestamp, raid.from_user_id],
            )
            .map_err(err)?;
        Ok(Raid {
            id: self.conn.last_insert_rowid(),
            channel: raid.channel.clone(),
            from_login: raid.from_login.clone(),
            from_display_name: raid.from_display_name.clone(),
            viewers: raid.viewers,
            timestamp: raid.timestamp,
            acknowledged: false,
            from_user_id: raid.from_user_id.clone(),
            shoutout_at: None,
            risk: None,
        })
    }

    pub fn set_raid_risk(&self, id: i64, risk: &crate::raidwatch::RaidRisk) -> DbResult<()> {
        let data = serde_json::to_string(risk).map_err(err)?;
        self.conn.execute("UPDATE raids SET risk = ?2 WHERE id = ?1", params![id, data]).map_err(err)?;
        Ok(())
    }

    /// Raids from this channel into any of yours before `before_id`.
    pub fn prior_raids_from(&self, from_login: &str, before_id: i64) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM raids WHERE from_login = ?1 AND id < ?2", params![from_login, before_id], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn raids(&self, limit: i64) -> DbResult<Vec<Raid>> {
        self.query_raids("1 = 1", params![limit])
    }

    pub fn raid_by_id(&self, id: i64) -> DbResult<Option<Raid>> {
        Ok(self.query_raids("id = ?2", params![1, id])?.into_iter().next())
    }

    pub fn set_raid_shoutout(&self, id: i64, ts: i64) -> DbResult<()> {
        self.conn.execute("UPDATE raids SET shoutout_at = ?2 WHERE id = ?1", params![id, ts]).map_err(err)?;
        Ok(())
    }

    fn query_raids(&self, filter: &str, args: impl rusqlite::Params) -> DbResult<Vec<Raid>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT id, channel, from_login, from_display_name, viewers, ts, acknowledged, from_user_id, shoutout_at, risk
                 FROM raids WHERE {filter} ORDER BY ts DESC, id DESC LIMIT ?1"
            ))
            .map_err(err)?;
        let rows = stmt
            .query_map(args, |r| {
                Ok(Raid {
                    id: r.get(0)?,
                    channel: r.get(1)?,
                    from_login: r.get(2)?,
                    from_display_name: r.get(3)?,
                    viewers: r.get(4)?,
                    timestamp: r.get(5)?,
                    acknowledged: r.get::<_, i64>(6)? != 0,
                    from_user_id: r.get(7)?,
                    shoutout_at: r.get(8)?,
                    risk: r.get::<_, Option<String>>(9)?.and_then(|t| serde_json::from_str(&t).ok()),
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    // ---- mod log & activity ----

    pub fn insert_mod_log(&self, e: &ModLogEntry) -> DbResult<ModLogEntry> {
        self.conn
            .execute(
                "INSERT INTO mod_log (channel, moderator, action, target_login, summary, ts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![e.channel, e.moderator, e.action, e.target_login, e.summary, e.timestamp],
            )
            .map_err(err)?;
        Ok(ModLogEntry { id: self.conn.last_insert_rowid(), ..e.clone() })
    }

    pub fn mod_log(&self, limit: i64) -> DbResult<Vec<ModLogEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, channel, moderator, action, target_login, summary, ts FROM mod_log ORDER BY ts DESC, id DESC LIMIT ?1",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(ModLogEntry {
                    id: r.get(0)?,
                    channel: r.get(1)?,
                    moderator: r.get(2)?,
                    action: r.get(3)?,
                    target_login: r.get(4)?,
                    summary: r.get(5)?,
                    timestamp: r.get(6)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn insert_activity(&self, a: &Activity) -> DbResult<Activity> {
        self.conn
            .execute(
                "INSERT INTO activity (channel, kind, user_login, display_name, text, ts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![a.channel, a.kind, a.user_login, a.display_name, a.text, a.timestamp],
            )
            .map_err(err)?;
        Ok(Activity { id: self.conn.last_insert_rowid(), ..a.clone() })
    }

    pub fn activity(&self, limit: i64) -> DbResult<Vec<Activity>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, channel, kind, user_login, display_name, text, ts FROM activity ORDER BY ts DESC, id DESC LIMIT ?1")
            .map_err(err)?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(Activity {
                    id: r.get(0)?,
                    channel: r.get(1)?,
                    kind: r.get(2)?,
                    user_login: r.get(3)?,
                    display_name: r.get(4)?,
                    text: r.get(5)?,
                    timestamp: r.get(6)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    /// Drop mod log and activity older than `before_ms`.
    pub fn prune_feeds(&self, before_ms: i64) -> DbResult<()> {
        self.conn.execute("DELETE FROM mod_log WHERE ts < ?1", params![before_ms]).map_err(err)?;
        self.conn.execute("DELETE FROM activity WHERE ts < ?1", params![before_ms]).map_err(err)?;
        Ok(())
    }

    pub fn ack_raids(&self) -> DbResult<()> {
        self.conn.execute("UPDATE raids SET acknowledged = 1", []).map_err(err)?;
        Ok(())
    }

    // ---- mod actions ----

    pub fn insert_mod_action(&self, a: &ModAction) -> DbResult<ModAction> {
        self.conn
            .execute(
                "INSERT INTO mod_actions (channel, target_login, action, duration_secs, reason, ok, error, ts, by_app, target_user_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    a.channel,
                    a.target_login,
                    a.action,
                    a.duration_secs,
                    a.reason,
                    a.ok as i64,
                    a.error,
                    a.timestamp,
                    a.by_app as i64,
                    a.target_user_id
                ],
            )
            .map_err(err)?;
        Ok(ModAction { id: self.conn.last_insert_rowid(), ..a.clone() })
    }

    pub fn mod_actions_for_user(&self, channel: &str, login: &str, limit: i64) -> DbResult<Vec<ModAction>> {
        self.query_actions("channel = ?2 AND target_login = ?3", params![limit, channel, login])
    }

    /// Successful warns, timeouts, bans and unbans since `since_ms`, from this app or other mods.
    pub fn recent_sanctions(&self, since_ms: i64, limit: i64) -> DbResult<Vec<ModAction>> {
        self.query_actions(
            "ok = 1 AND action IN ('warn', 'timeout', 'ban', 'unban') AND ts >= ?2",
            params![limit, since_ms],
        )
    }

    /// Users banned since `since_ms` (by anyone) who haven't been unbanned since: (channel, login, banned at).
    pub fn current_bans(&self, since_ms: i64) -> DbResult<Vec<(String, String, i64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT a.channel, a.target_login, MAX(a.ts) FROM mod_actions a
                 WHERE a.ok = 1 AND a.action = 'ban' AND a.ts >= ?1
                   AND NOT EXISTS (SELECT 1 FROM mod_actions u WHERE u.ok = 1 AND u.action = 'unban'
                                   AND u.channel = a.channel AND u.target_login = a.target_login AND u.ts > a.ts)
                 GROUP BY a.channel, a.target_login",
            )
            .map_err(err)?;
        let rows = stmt.query_map(params![since_ms], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    /// Whether this app already logged the same action recently (so the echo from chat isn't counted twice).
    pub fn app_action_recent(&self, channel: &str, login: &str, action: &str, since_ms: i64) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM mod_actions WHERE channel = ?1 AND target_login = ?2 AND action = ?3 AND by_app = 1 AND ts >= ?4 LIMIT 1",
                params![channel, login, action, since_ms],
                |_| Ok(()),
            )
            .is_ok()
    }

    fn query_actions(&self, filter: &str, args: impl rusqlite::Params) -> DbResult<Vec<ModAction>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT id, channel, target_login, action, duration_secs, reason, ok, error, ts, by_app, target_user_id
                 FROM mod_actions WHERE {filter} ORDER BY ts DESC, id DESC LIMIT ?1"
            ))
            .map_err(err)?;
        let rows = stmt
            .query_map(args, |r| {
                Ok(ModAction {
                    id: r.get(0)?,
                    channel: r.get(1)?,
                    target_login: r.get(2)?,
                    action: r.get(3)?,
                    duration_secs: r.get(4)?,
                    reason: r.get(5)?,
                    ok: r.get::<_, i64>(6)? != 0,
                    error: r.get(7)?,
                    timestamp: r.get(8)?,
                    by_app: r.get::<_, i64>(9)? != 0,
                    target_user_id: r.get(10)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    // ---- account cache (login name and ID only; tokens live in the OS keychain) ----

    pub fn account<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = 'account'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|v| serde_json::from_str(&v).ok())
    }

    pub fn save_account<T: Serialize>(&self, account: Option<&T>) -> DbResult<()> {
        match account {
            Some(a) => self
                .conn
                .execute(
                    "INSERT INTO settings (key, value) VALUES ('account', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![serde_json::to_string(a).map_err(err)?],
                )
                .map(|_| ())
                .map_err(err),
            None => self.conn.execute("DELETE FROM settings WHERE key = 'account'", []).map(|_| ()).map_err(err),
        }
    }

    // ---- settings ----

    pub fn settings(&self) -> Settings {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) -> DbResult<()> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES ('app', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![serde_json::to_string(settings).map_err(err)?],
            )
            .map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_starter_rule_and_packs_once() {
        let db = Db::open_in_memory().unwrap();
        let n = 1 + crate::packs::default_packs().len();
        assert_eq!(db.rules().unwrap().len(), n);
        db.seed_defaults().unwrap();
        assert_eq!(db.rules().unwrap().len(), n);
        // Deleted pack rules can be restored; existing ones aren't duplicated.
        let romantic = db.rules().unwrap().into_iter().find(|r| r.name == "Romantic / possessive").unwrap();
        db.delete_rule(romantic.id).unwrap();
        assert_eq!(db.restore_default_packs().unwrap(), 1);
        assert_eq!(db.rules().unwrap().len(), n);
    }

    #[test]
    fn rule_stats_count_outcomes_patterns_and_test_hits() {
        let db = Db::open_in_memory().unwrap();
        let m = |patterns: &[&str]| RuleMatch {
            rule_id: 7,
            rule_name: "r".into(),
            severity: Severity::Low,
            terms: vec![],
            reason: None,
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
            test_mode: false,
            wellbeing: false,
        };
        let flag = |ts: i64, status: &str, resolution: Option<&str>, pats: &[&str]| Flag {
            id: 0,
            channel: "c".into(),
            message_id: format!("m{ts}"),
            user_id: None,
            user_login: "u".into(),
            display_name: "u".into(),
            text: "t".into(),
            severity: Severity::Low,
            matches: vec![m(pats)],
            context: vec![],
            context_after: vec![],
            timestamp: ts,
            status: status.into(),
            resolution: resolution.map(str::to_string),
            kind: "rule".into(),
            external_id: None,
            meta: serde_json::json!({}),
        };
        db.insert_flag(&flag(10, "reviewed", Some("Timed out 10m"), &["a"])).unwrap();
        db.insert_flag(&flag(11, "dismissed", None, &["b"])).unwrap();
        db.insert_flag(&flag(12, "dismissed", None, &["b"])).unwrap();
        db.insert_flag(&flag(13, "reviewed", None, &["a"])).unwrap();
        db.insert_flag(&flag(14, "open", None, &["a", "b"])).unwrap();
        db.insert_flag(&flag(1, "dismissed", None, &["a"])).unwrap(); // outside window
        // Archiving (clearing) keeps flags for stats.
        assert_eq!(db.clear_closed_flags().unwrap(), 5);
        assert_eq!(db.flags(100).unwrap().len(), 1);

        db.insert_rule_hit(&RuleHit {
            rule_id: 9,
            channel: "c".into(),
            user_login: "u".into(),
            display_name: "u".into(),
            text: "would flag".into(),
            terms: vec!["flag".into()],
            reason: None,
            timestamp: 20,
        })
        .unwrap();

        let stats = db.rule_stats(5, 5).unwrap();
        let r7 = stats.iter().find(|s| s.rule_id == 7).unwrap();
        assert_eq!((r7.flags, r7.actioned, r7.reviewed, r7.dismissed, r7.open), (5, 1, 1, 2, 1));
        assert_eq!(r7.last_fired, Some(14));
        let b = r7.patterns.iter().find(|p| p.pattern == "b").unwrap();
        assert_eq!((b.flags, b.dismissed), (3, 2));
        let r9 = stats.iter().find(|s| s.rule_id == 9).unwrap();
        assert_eq!((r9.test_hits, r9.test_samples[0].text.as_str()), (1, "would flag"));

        db.prune_tuning_data(25).unwrap();
        assert!(db.rule_stats(0, 5).unwrap().iter().all(|s| s.test_hits == 0));
    }

    #[test]
    fn channel_and_rule_round_trip() {
        let db = Db::open_in_memory().unwrap();
        db.add_channel("streamer", "42", 1).unwrap();
        db.add_channel("streamer", "42", 2).unwrap();
        db.set_channel_muted("streamer", true).unwrap();
        let chans = db.channels().unwrap();
        assert_eq!(chans.len(), 1);
        assert!(chans[0].muted && chans[0].is_mod && db.is_muted("streamer"));
        assert_eq!(chans[0].alert_level, ChannelAlertLevel::Off);
        db.set_channel_alerts("streamer", ChannelAlertLevel::High, false).unwrap();
        assert_eq!(db.channel_alerts("streamer"), (ChannelAlertLevel::High, false));
        assert!(!db.is_muted("streamer"));
        assert_eq!(db.broadcaster_id("streamer").as_deref(), Some("42"));
        db.set_channel_mod_status("streamer", None, false).unwrap();
        let c = &db.channels().unwrap()[0];
        assert!(!c.is_mod);
        assert_eq!(c.broadcaster_id.as_deref(), Some("42"));

        let before = db.rules().unwrap().len();
        let mut rule = db.rules().unwrap().remove(0);
        rule.name = "renamed".into();
        db.save_rule(&rule).unwrap();
        assert_eq!(db.rules().unwrap()[0].name, "renamed");
        db.delete_rule(rule.id).unwrap();
        assert_eq!(db.rules().unwrap().len(), before - 1);
    }

    #[test]
    fn channels_get_distinct_colors() {
        let db = Db::open_in_memory().unwrap();
        for (i, name) in ["kitdy", "panda", "third"].iter().enumerate() {
            db.add_channel(name, &i.to_string(), i as i64).unwrap();
        }
        let colors: Vec<String> = db.channels().unwrap().into_iter().map(|c| c.color.unwrap()).collect();
        assert_eq!(colors, CHANNEL_PALETTE[..3].to_vec());
        db.set_channel_color("panda", Some("#123456")).unwrap();
        assert!(db.set_channel_color("panda", Some("red")).is_err());
        db.add_channel("fourth", "9", 9).unwrap();
        // The freed-up second palette color goes to the new channel.
        assert_eq!(db.channels().unwrap()[3].color.as_deref(), Some(CHANNEL_PALETTE[1]));
        db.set_shoutout_message("kitdy", "  !so @{raider}  ").unwrap();
        assert_eq!(db.channels().unwrap()[0].shoutout_message, "!so @{raider}");
    }

    #[test]
    fn mod_log_and_activity_round_trip() {
        let db = Db::open_in_memory().unwrap();
        db.insert_mod_log(&ModLogEntry {
            id: 0,
            channel: "c".into(),
            moderator: "m".into(),
            action: "ban".into(),
            target_login: Some("u".into()),
            summary: "banned u".into(),
            timestamp: 5,
        })
        .unwrap();
        db.insert_activity(&Activity {
            id: 0,
            channel: "c".into(),
            kind: "sub".into(),
            user_login: "u".into(),
            display_name: "U".into(),
            text: "subscribed".into(),
            timestamp: 6,
        })
        .unwrap();
        assert_eq!(db.mod_log(10).unwrap()[0].summary, "banned u");
        assert_eq!(db.activity(10).unwrap()[0].kind, "sub");
        db.prune_feeds(10).unwrap();
        assert!(db.mod_log(10).unwrap().is_empty() && db.activity(10).unwrap().is_empty());
    }

    #[test]
    fn flags_and_raids_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .insert_flag(&Flag {
                id: 0,
                channel: "c".into(),
                message_id: "m".into(),
                user_login: "u".into(),
                display_name: "U".into(),
                text: "bad".into(),
                severity: Severity::High,
                matches: vec![],
                context: vec![],
                timestamp: 5,
                status: "open".into(),
                user_id: Some("7".into()),
                context_after: vec![],
                resolution: None,
                kind: "automod".into(),
                external_id: Some("am1".into()),
                meta: serde_json::json!({"category": "swearing"}),
            })
            .unwrap();
        assert_eq!(db.flag_by_external("automod", "am1"), Some(id));
        assert_eq!(db.flag_by_id(id).unwrap().unwrap().kind, "automod");
        assert!(db.open_flag_exists("automod", "c", "u"));
        assert_eq!(db.resolve_external("automod", "am1", "Approved").unwrap(), Some(id));
        assert_eq!(db.resolve_external("automod", "am1", "Approved").unwrap(), None);
        db.set_flag_status(id, "open").unwrap();
        assert_eq!(db.flags(1).unwrap()[0].meta["category"], "swearing");
        let line = ContextLine { display_name: "x".into(), text: "after".into(), timestamp: 6 };
        db.append_context_after(id, &line).unwrap();
        assert_eq!(db.flags_for_user("c", "u", 10).unwrap()[0].context_after[0].text, "after");
        assert_eq!(db.resolve_flags("c", "u", Some("m"), "Banned").unwrap(), vec![id]);
        let f = &db.flags(10).unwrap()[0];
        assert_eq!((f.status.as_str(), f.resolution.as_deref()), ("reviewed", Some("Banned")));
        db.set_flag_status(id, "open").unwrap();
        db.set_flag_status(id, "dismissed").unwrap();
        assert!(db.set_flag_status(id, "bogus").is_err());
        assert_eq!(db.flags(10).unwrap()[0].status, "dismissed");
        assert_eq!(db.clear_closed_flags().unwrap(), 1);
        assert!(db.flags(10).unwrap().is_empty());

        db.insert_raid(&RaidNotice {
            channel: "c".into(),
            from_login: "r".into(),
            from_display_name: "R".into(),
            from_user_id: Some("77".into()),
            viewers: 10,
            timestamp: 1,
        })
        .unwrap();
        let raid = db.raids(1).unwrap().remove(0);
        db.set_raid_shoutout(raid.id, 3).unwrap();
        let raid = db.raid_by_id(raid.id).unwrap().unwrap();
        assert_eq!((raid.from_user_id.as_deref(), raid.shoutout_at), (Some("77"), Some(3)));
        db.ack_raids().unwrap();
        assert!(db.raids(10).unwrap()[0].acknowledged);

        let a = db
            .insert_mod_action(&ModAction {
                id: 0,
                channel: "c".into(),
                target_login: "u".into(),
                action: "timeout".into(),
                duration_secs: Some(600),
                reason: "spam".into(),
                ok: true,
                error: None,
                timestamp: 9,
                by_app: true,
                target_user_id: Some("7".into()),
            })
            .unwrap();
        assert!(a.id > 0);
        assert_eq!(db.mod_actions_for_user("c", "u", 10).unwrap().len(), 1);
        assert!(db.app_action_recent("c", "u", "timeout", 5));
        assert!(!db.app_action_recent("c", "u", "ban", 5));
        assert_eq!(db.recent_sanctions(0, 10).unwrap().len(), 1);
        assert!(db.recent_sanctions(10, 10).unwrap().is_empty());

        db.save_account(Some(&serde_json::json!({"login": "me"}))).unwrap();
        assert_eq!(db.account::<serde_json::Value>().unwrap()["login"], "me");
        db.save_account::<serde_json::Value>(None).unwrap();
        assert!(db.account::<serde_json::Value>().is_none());

        let mut s = db.settings();
        assert_eq!(s.raid_mode_minutes, 5);
        assert_eq!(s.ladder.len(), 4);
        s.raid_min_viewers = 25;
        db.save_settings(&s).unwrap();
        assert_eq!(db.settings().raid_min_viewers, 25);
    }
}
