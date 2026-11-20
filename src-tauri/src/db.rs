//! SQLite storage for channels, rules, flags, raids and settings.

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
    pub muted: bool,
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
    pub user_login: String,
    pub display_name: String,
    pub text: String,
    pub severity: Severity,
    pub matches: Vec<RuleMatch>,
    pub context: Vec<ContextLine>,
    pub timestamp: i64,
    /// "open", "reviewed" or "dismissed"
    pub status: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub notify_flags: bool,
    pub notify_min_severity: Severity,
    pub notify_raids: bool,
    pub raid_min_viewers: i64,
    pub play_sounds: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            notify_flags: true,
            notify_min_severity: Severity::Medium,
            notify_raids: true,
            raid_min_viewers: 0,
            play_sounds: true,
        }
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
        db.seed_defaults()?;
        Ok(db)
    }

    /// On first run, add a starter rule so the app does something useful immediately.
    fn seed_defaults(&self) -> DbResult<()> {
        let seeded: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = 'seeded'", [], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if seeded.is_some() {
            return Ok(());
        }
        self.save_rule(&Rule {
            id: 0,
            name: "Follower/viewer bot spam".into(),
            enabled: true,
            kind: RuleKind::Keyword,
            patterns: ["free followers", "cheap viewers", "best viewers", "buy followers", "primes and viewers"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            whole_word: true,
            case_sensitive: false,
            channels: vec![],
            severity: Severity::High,
            exempt_privileged: true,
        })?;
        self.conn
            .execute("INSERT INTO settings (key, value) VALUES ('seeded', '1')", [])
            .map_err(err)?;
        Ok(())
    }

    // ---- channels ----

    pub fn channels(&self) -> DbResult<Vec<Channel>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, muted FROM channels ORDER BY added_at")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| Ok(Channel { name: r.get(0)?, muted: r.get::<_, i64>(1)? != 0 }))
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn add_channel(&self, name: &str, now: i64) -> DbResult<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO channels (name, muted, added_at) VALUES (?1, 0, ?2)",
                params![name, now],
            )
            .map_err(err)?;
        Ok(())
    }

    pub fn remove_channel(&self, name: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM channels WHERE name = ?1", params![name]).map_err(err)?;
        Ok(())
    }

    pub fn set_channel_muted(&self, name: &str, muted: bool) -> DbResult<()> {
        self.conn
            .execute("UPDATE channels SET muted = ?2 WHERE name = ?1", params![name, muted as i64])
            .map_err(err)?;
        Ok(())
    }

    pub fn is_muted(&self, name: &str) -> bool {
        self.conn
            .query_row("SELECT muted FROM channels WHERE name = ?1", params![name], |r| r.get::<_, i64>(0))
            .map(|m| m != 0)
            .unwrap_or(false)
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
                "INSERT INTO flags (channel, message_id, user_login, display_name, text, severity, matches, context, ts, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                ],
            )
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn flags(&self, limit: i64) -> DbResult<Vec<Flag>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, channel, message_id, user_login, display_name, text, severity, matches, context, ts, status
                 FROM flags ORDER BY ts DESC, id DESC LIMIT ?1",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![limit], |r| {
                let severity: String = r.get(6)?;
                let matches: String = r.get(7)?;
                let context: String = r.get(8)?;
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
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
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

    /// Delete every flag that is no longer open.
    pub fn clear_closed_flags(&self) -> DbResult<usize> {
        self.conn.execute("DELETE FROM flags WHERE status != 'open'", []).map_err(err)
    }

    // ---- raids ----

    pub fn insert_raid(&self, raid: &RaidNotice) -> DbResult<Raid> {
        self.conn
            .execute(
                "INSERT INTO raids (channel, from_login, from_display_name, viewers, ts) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![raid.channel, raid.from_login, raid.from_display_name, raid.viewers, raid.timestamp],
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
        })
    }

    pub fn raids(&self, limit: i64) -> DbResult<Vec<Raid>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, channel, from_login, from_display_name, viewers, ts, acknowledged
                 FROM raids ORDER BY ts DESC, id DESC LIMIT ?1",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(Raid {
                    id: r.get(0)?,
                    channel: r.get(1)?,
                    from_login: r.get(2)?,
                    from_display_name: r.get(3)?,
                    viewers: r.get(4)?,
                    timestamp: r.get(5)?,
                    acknowledged: r.get::<_, i64>(6)? != 0,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    pub fn ack_raids(&self) -> DbResult<()> {
        self.conn.execute("UPDATE raids SET acknowledged = 1", []).map_err(err)?;
        Ok(())
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
    fn seeds_starter_rule_once() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.rules().unwrap().len(), 1);
        db.seed_defaults().unwrap();
        assert_eq!(db.rules().unwrap().len(), 1);
    }

    #[test]
    fn channel_and_rule_round_trip() {
        let db = Db::open_in_memory().unwrap();
        db.add_channel("streamer", 1).unwrap();
        db.add_channel("streamer", 2).unwrap();
        db.set_channel_muted("streamer", true).unwrap();
        let chans = db.channels().unwrap();
        assert_eq!(chans.len(), 1);
        assert!(chans[0].muted && db.is_muted("streamer"));

        let mut rule = db.rules().unwrap().remove(0);
        rule.name = "renamed".into();
        db.save_rule(&rule).unwrap();
        assert_eq!(db.rules().unwrap()[0].name, "renamed");
        db.delete_rule(rule.id).unwrap();
        assert!(db.rules().unwrap().is_empty());
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
            })
            .unwrap();
        db.set_flag_status(id, "dismissed").unwrap();
        assert!(db.set_flag_status(id, "bogus").is_err());
        assert_eq!(db.flags(10).unwrap()[0].status, "dismissed");
        assert_eq!(db.clear_closed_flags().unwrap(), 1);

        db.insert_raid(&RaidNotice {
            channel: "c".into(),
            from_login: "r".into(),
            from_display_name: "R".into(),
            viewers: 10,
            timestamp: 1,
        })
        .unwrap();
        db.ack_raids().unwrap();
        assert!(db.raids(10).unwrap()[0].acknowledged);

        let mut s = db.settings();
        s.raid_min_viewers = 25;
        db.save_settings(&s).unwrap();
        assert_eq!(db.settings().raid_min_viewers, 25);
    }
}
