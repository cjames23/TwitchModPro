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
    /// Twitch ID of the channel, known once mod status has been verified.
    pub broadcaster_id: Option<String>,
    /// Whether the logged-in user moderates this channel (as of the last check).
    pub is_mod: bool,
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
        Ok(())
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
            target: Default::default(),
            normalize: true,
            active_during: Default::default(),
            params: Default::default(),
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
            .prepare("SELECT name, muted, broadcaster_id, is_mod FROM channels ORDER BY added_at")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Channel {
                    name: r.get(0)?,
                    muted: r.get::<_, i64>(1)? != 0,
                    broadcaster_id: r.get(2)?,
                    is_mod: r.get::<_, i64>(3)? != 0,
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
        Ok(())
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
        self.query_flags("1 = 1", params![limit])
    }

    pub fn flags_for_user(&self, channel: &str, login: &str, limit: i64) -> DbResult<Vec<Flag>> {
        self.query_flags("channel = ?2 AND user_login = ?3", params![limit, channel, login])
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
    fn seeds_starter_rule_once() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.rules().unwrap().len(), 1);
        db.seed_defaults().unwrap();
        assert_eq!(db.rules().unwrap().len(), 1);
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
        assert_eq!(db.broadcaster_id("streamer").as_deref(), Some("42"));
        db.set_channel_mod_status("streamer", None, false).unwrap();
        let c = &db.channels().unwrap()[0];
        assert!(!c.is_mod);
        assert_eq!(c.broadcaster_id.as_deref(), Some("42"));

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
