//! App core: login session, mod-verified channels, chat processing, raid mode,
//! user history and moderation actions.

use crate::alerts::{flag_alerts, GlobalAlerts, RuleAlert};
use crate::db::{Activity, Channel, ContextLine, Db, Flag, ModAction, Raid, RuleHit};
use crate::eventsub::EventSubClient;
use crate::irc::{ChatMessage, RaidNotice};
use crate::rules::{EvalCtx, RuleEngine, RuleKind, RuleMatch, Severity};
use crate::waves::{WaveConfig, WaveDetector};
use crate::twitch::{ChatClient, ChatEvent, ConnStatus};
use crate::twitch_api::{self as api, Account, ApiError, DeviceStart, Endpoints, Helix, ModChannel, PollResult, Tokens};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

/// Earlier messages saved with a flag.
const CONTEXT_BEFORE: usize = 8;
/// Later messages appended to a flag as they arrive.
const CONTEXT_AFTER: usize = 5;
const NOTIFY_COOLDOWN: Duration = Duration::from_secs(4);
const HISTORY_PER_USER: usize = 25;
const HISTORY_MAX_USERS: usize = 50_000;
/// How often to re-validate the token (Twitch requires hourly) and re-check mod status.
pub const SESSION_CHECK: Duration = Duration::from_secs(30 * 60);

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveMessage {
    #[serde(flatten)]
    pub msg: ChatMessage,
    pub flag_severity: Option<Severity>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub status: ConnStatus,
    pub joined: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginProgress {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    pub client_id_set: bool,
    pub client_id_from_build: bool,
    pub account: Option<Account>,
    pub login: Option<LoginProgress>,
    pub error: Option<String>,
    /// True while the saved login is being checked at startup.
    pub checking: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShoutoutResult {
    /// Error from Twitch's native shoutout, if it failed.
    pub native: Option<String>,
    /// The chat message that was sent, if any.
    pub message_sent: Option<String>,
    pub message_error: Option<String>,
    pub raid: Raid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryLine {
    pub id: String,
    pub text: String,
    pub timestamp: i64,
    pub flag_severity: Option<Severity>,
}

#[derive(Default)]
struct UserHistory {
    users: HashMap<(String, String), (Option<String>, String, VecDeque<HistoryLine>)>,
}

impl UserHistory {
    fn record(&mut self, msg: &ChatMessage, flag_severity: Option<Severity>) {
        if self.users.len() >= HISTORY_MAX_USERS {
            let cutoff = msg.timestamp - 2 * 60 * 60 * 1000;
            self.users.retain(|_, (_, _, lines)| lines.back().is_some_and(|l| l.timestamp >= cutoff));
            if self.users.len() >= HISTORY_MAX_USERS {
                self.users.clear();
            }
        }
        let entry = self
            .users
            .entry((msg.channel.clone(), msg.user_login.clone()))
            .or_insert_with(|| (None, String::new(), VecDeque::new()));
        if msg.user_id.is_some() {
            entry.0 = msg.user_id.clone();
        }
        entry.1 = msg.display_name.clone();
        entry.2.push_back(HistoryLine {
            id: msg.id.clone(),
            text: msg.text.clone(),
            timestamp: msg.timestamp,
            flag_severity,
        });
        while entry.2.len() > HISTORY_PER_USER {
            entry.2.pop_front();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserHistoryView {
    pub channel: String,
    pub login: String,
    pub display_name: String,
    pub user_id: Option<String>,
    pub messages: Vec<HistoryLine>,
    pub flags: Vec<Flag>,
    pub actions: Vec<ModAction>,
    /// Warns, timeouts and bans of this user in your other channels.
    pub other_channels: Vec<ModAction>,
    pub account_created_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActionKind {
    Delete,
    Warn,
    Timeout(u32),
    Ban,
    Unban,
}

pub fn wave_config(s: &crate::db::Settings) -> WaveConfig {
    WaveConfig {
        min_accounts: s.wave_min_accounts.clamp(2, 100) as usize,
        window_ms: s.wave_window_secs.clamp(10, 600) * 1000,
    }
}

pub fn format_duration(secs: u32) -> String {
    match secs {
        s if s % 86_400 == 0 => format!("{}d", s / 86_400),
        s if s % 3_600 == 0 => format!("{}h", s / 3_600),
        s if s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

pub struct Core {
    pub app: AppHandle,
    pub db: Mutex<Db>,
    pub engine: RwLock<RuleEngine>,
    pub connection: Mutex<ConnectionInfo>,
    pub pending_chat: Mutex<Vec<LiveMessage>>,
    pub client: OnceLock<ChatClient>,
    pub http: reqwest::Client,
    pub endpoints: Endpoints,
    pub(crate) context: Mutex<HashMap<String, VecDeque<ContextLine>>>,
    pending_after: Mutex<HashMap<String, Vec<(i64, usize)>>>,
    last_notified: Mutex<HashMap<String, Instant>>,
    history: Mutex<UserHistory>,
    raid_until: Mutex<HashMap<String, i64>>,
    helix: RwLock<Option<Helix>>,
    moderated: Mutex<Option<Vec<ModChannel>>>,
    auth: Mutex<AuthState>,
    login_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// Account creation times (ms) by login.
    pub(crate) age_cache: Mutex<HashMap<String, i64>>,
    /// Logins waiting for an account-age lookup.
    pub(crate) age_queue: Mutex<BTreeSet<String>>,
    /// Messages held until their sender's account age is known (for new-account rules).
    pub(crate) age_waiting: Mutex<HashMap<(String, String), (ChatMessage, Vec<ContextLine>)>>,
    /// (channel, login) already flagged by a new-account rule this session.
    pub(crate) new_account_flagged: Mutex<HashSet<(String, String)>>,
    pub(crate) waves: Mutex<WaveDetector>,
    pub(crate) wave_cfg: Mutex<WaveConfig>,
    pub(crate) wave_notified: Mutex<HashSet<i64>>,
    pub(crate) eventsub: Mutex<Option<EventSubClient>>,
}

impl Core {
    pub fn new(app: AppHandle, db: Db, engine: RuleEngine) -> Self {
        let wave_cfg = wave_config(&db.settings());
        Core {
            app,
            db: Mutex::new(db),
            engine: RwLock::new(engine),
            connection: Mutex::new(ConnectionInfo { status: ConnStatus::Connecting, joined: vec![] }),
            pending_chat: Mutex::new(Vec::new()),
            client: OnceLock::new(),
            http: api::http_client(),
            endpoints: Endpoints::default(),
            context: Mutex::new(HashMap::new()),
            pending_after: Mutex::new(HashMap::new()),
            last_notified: Mutex::new(HashMap::new()),
            history: Mutex::new(UserHistory::default()),
            raid_until: Mutex::new(HashMap::new()),
            helix: RwLock::new(None),
            moderated: Mutex::new(None),
            auth: Mutex::new(AuthState { checking: true, ..Default::default() }),
            login_task: Mutex::new(None),
            age_cache: Mutex::new(HashMap::new()),
            age_queue: Mutex::new(BTreeSet::new()),
            age_waiting: Mutex::new(HashMap::new()),
            new_account_flagged: Mutex::new(HashSet::new()),
            waves: Mutex::new(WaveDetector::default()),
            wave_cfg: Mutex::new(wave_cfg),
            wave_notified: Mutex::new(HashSet::new()),
            eventsub: Mutex::new(None),
        }
    }

    pub(crate) fn client(&self) -> Option<&ChatClient> {
        self.client.get()
    }

    // ---------------- auth ----------------

    pub fn client_id(&self) -> Option<String> {
        let from_settings = self.db.lock().unwrap().settings().twitch_client_id;
        let id = if from_settings.trim().is_empty() {
            option_env!("TWITCH_CLIENT_ID").unwrap_or_default().to_string()
        } else {
            from_settings
        };
        let id = id.trim().to_string();
        (!id.is_empty()).then_some(id)
    }

    pub fn auth_state(&self) -> AuthState {
        let mut s = self.auth.lock().unwrap().clone();
        s.client_id_set = self.client_id().is_some();
        s.client_id_from_build = option_env!("TWITCH_CLIENT_ID").is_some_and(|v| !v.is_empty());
        s.account = self.helix.read().unwrap().as_ref().map(|h| h.account.clone());
        s
    }

    fn emit_auth(&self) {
        let _ = self.app.emit("auth", self.auth_state());
    }

    pub fn helix(&self) -> Result<Helix, String> {
        self.helix.read().unwrap().clone().ok_or_else(|| "Log in with Twitch first.".to_string())
    }

    fn set_auth_error(&self, error: Option<String>) {
        let mut a = self.auth.lock().unwrap();
        a.error = error;
        a.checking = false;
    }

    /// Restore the saved login at startup, then join the channels the user moderates.
    pub async fn restore_session(self: &Arc<Self>) {
        let (Some(client_id), Some(tokens)) = (self.client_id(), api::load_tokens()) else {
            self.set_auth_error(None);
            self.emit_auth();
            self.emit_channels();
            return;
        };
        let cached: Option<Account> = self.db.lock().unwrap().account();
        let account = match api::validate_token(&self.http, &self.endpoints, &tokens.access_token).await {
            Ok(a) => Ok((a, tokens)),
            Err(ApiError::Unauthorized) => match &tokens.refresh_token {
                Some(r) => match api::refresh_tokens(&self.http, &self.endpoints, &client_id, r).await {
                    Ok(fresh) => api::validate_token(&self.http, &self.endpoints, &fresh.access_token)
                        .await
                        .map(|a| (a, fresh)),
                    Err(e) => Err(e),
                },
                None => Err(ApiError::Unauthorized),
            },
            // Offline or Twitch is down: trust the cached account for now.
            Err(ApiError::Other(e)) => match cached {
                Some(a) => Ok((a, tokens)),
                None => Err(ApiError::Other(e)),
            },
        };
        match account {
            Ok((acct, tokens)) if acct.missing_scopes().is_empty() => {
                self.start_session(client_id, acct, tokens).await;
            }
            Ok(_) => {
                self.end_session(Some("The app needs new permissions. Please log in again.".into())).await;
            }
            Err(ApiError::Unauthorized) => {
                self.end_session(Some("Your Twitch login expired. Please log in again.".into())).await;
            }
            Err(ApiError::Other(e)) => {
                self.set_auth_error(Some(e));
                self.emit_auth();
                self.emit_channels();
            }
        }
    }

    async fn start_session(self: &Arc<Self>, client_id: String, account: Account, tokens: Tokens) {
        if let Err(e) = api::save_tokens(&tokens) {
            eprintln!("{e}");
        }
        let _ = self.db.lock().unwrap().save_account(Some(&account));
        let helix = Helix::new(
            self.http.clone(),
            self.endpoints.clone(),
            client_id,
            account,
            tokens,
            Arc::new(|t: &Tokens| {
                if let Err(e) = api::save_tokens(t) {
                    eprintln!("{e}");
                }
            }),
        );
        *self.helix.write().unwrap() = Some(helix);
        {
            let mut a = self.auth.lock().unwrap();
            a.login = None;
            a.error = None;
            a.checking = false;
        }
        self.emit_auth();
        self.sync_channels().await;
    }

    /// Log out locally (and revoke the token if possible). `reason` is shown on the login screen.
    pub async fn end_session(&self, reason: Option<String>) {
        if let Some(es) = self.eventsub.lock().unwrap().take() {
            es.stop();
        }
        let helix = self.helix.write().unwrap().take();
        if let Some(h) = helix {
            let t = h.tokens().await;
            api::revoke_token(&self.http, &self.endpoints, h.client_id(), &t.access_token).await;
        }
        api::clear_tokens();
        let _ = self.db.lock().unwrap().save_account::<Account>(None);
        *self.moderated.lock().unwrap() = None;
        if let Some(c) = self.client() {
            for ch in self.db.lock().unwrap().channels().unwrap_or_default() {
                c.part(&ch.name);
            }
        }
        self.set_auth_error(reason);
        self.emit_auth();
        self.emit_channels();
    }

    pub fn cancel_login(&self) {
        if let Some(t) = self.login_task.lock().unwrap().take() {
            t.abort();
        }
        self.auth.lock().unwrap().login = None;
        self.emit_auth();
    }

    pub async fn start_login(self: &Arc<Self>) -> Result<AuthState, String> {
        let client_id = self.client_id().ok_or("Add your Twitch Client ID first.")?;
        self.cancel_login();
        let start: DeviceStart = api::start_device_login(&self.http, &self.endpoints, &client_id)
            .await
            .map_err(|e| e.to_string())?;
        {
            let mut a = self.auth.lock().unwrap();
            a.error = None;
            a.login = Some(LoginProgress {
                user_code: start.user_code.clone(),
                verification_uri: start.verification_uri.clone(),
                expires_at: now_ms() + start.expires_in as i64 * 1000,
            });
        }
        let core = self.clone();
        let task = tauri::async_runtime::spawn(async move {
            let mut interval = start.interval;
            let deadline = Instant::now() + Duration::from_secs(start.expires_in);
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                if Instant::now() > deadline {
                    core.finish_login_with_error("The login code expired. Try again.".into());
                    return;
                }
                match api::poll_device_login(&core.http, &core.endpoints, &client_id, &start.device_code).await {
                    Ok(PollResult::Pending) => {}
                    Ok(PollResult::SlowDown) => interval += 5,
                    Ok(PollResult::Done(tokens)) => {
                        match api::validate_token(&core.http, &core.endpoints, &tokens.access_token).await {
                            Ok(acct) => core.start_session(client_id.clone(), acct, tokens).await,
                            Err(e) => core.finish_login_with_error(e.to_string()),
                        }
                        return;
                    }
                    Ok(PollResult::Expired) => {
                        core.finish_login_with_error("The login code expired. Try again.".into());
                        return;
                    }
                    Ok(PollResult::Failed(msg)) => {
                        core.finish_login_with_error(msg);
                        return;
                    }
                    Err(e) => eprintln!("login poll error: {e}"),
                }
            }
        });
        *self.login_task.lock().unwrap() = Some(task);
        let state = self.auth_state();
        self.emit_auth();
        Ok(state)
    }

    fn finish_login_with_error(&self, msg: String) {
        {
            let mut a = self.auth.lock().unwrap();
            a.login = None;
            a.error = Some(msg);
        }
        self.emit_auth();
    }

    /// Periodic check: re-validate the token and refresh the moderated-channel list.
    pub async fn check_session(self: &Arc<Self>) {
        let Ok(h) = self.helix() else { return };
        let t = h.tokens().await;
        if let Err(ApiError::Unauthorized) = api::validate_token(&self.http, &self.endpoints, &t.access_token).await {
            // Try a refresh through a harmless API call before giving up.
            if let Err(ApiError::Unauthorized) = h.moderated_channels().await {
                self.end_session(Some("Your Twitch login expired. Please log in again.".into())).await;
                return;
            }
        }
        self.sync_channels().await;
    }

    // ---------------- channels ----------------

    /// Fetch the channels the user moderates (cached unless `refresh`).
    pub async fn moderated_channels(self: &Arc<Self>, refresh: bool) -> Result<Vec<ModChannel>, String> {
        if !refresh {
            if let Some(c) = self.moderated.lock().unwrap().clone() {
                return Ok(c);
            }
        }
        let h = self.helix()?;
        match h.moderated_channels().await {
            Ok(list) => {
                *self.moderated.lock().unwrap() = Some(list.clone());
                Ok(list)
            }
            Err(ApiError::Unauthorized) => {
                self.end_session(Some("Your Twitch login expired. Please log in again.".into())).await;
                Err(ApiError::Unauthorized.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Re-check mod status for every saved channel, then join/part accordingly.
    pub async fn sync_channels(self: &Arc<Self>) {
        let list = self.moderated_channels(true).await;
        if self.helix.read().unwrap().is_none() {
            return;
        }
        let channels = {
            let db = self.db.lock().unwrap();
            if let Ok(list) = &list {
                for ch in db.channels().unwrap_or_default() {
                    let m = list.iter().find(|m| m.login == ch.name);
                    let _ = db.set_channel_mod_status(&ch.name, m.map(|m| m.id.as_str()), m.is_some());
                }
            }
            db.channels().unwrap_or_default()
        };
        if let Err(e) = &list {
            eprintln!("couldn't refresh moderated channels, using last known status: {e}");
        }
        if let Some(client) = self.client() {
            for ch in &channels {
                if ch.is_mod {
                    client.join(&ch.name);
                } else {
                    client.part(&ch.name);
                }
            }
        }
        self.emit_channels();
        self.sync_eventsub(&channels);
        self.load_unban_requests(&channels).await;
    }

    pub fn emit_channels(&self) {
        if let Ok(ch) = self.db.lock().unwrap().channels() {
            let _ = self.app.emit("channels", ch);
        }
    }

    /// Add a channel, but only if Twitch says the user moderates it.
    pub async fn add_channel(self: &Arc<Self>, login: &str) -> Result<Vec<Channel>, String> {
        let mut list = self.moderated_channels(false).await?;
        if !list.iter().any(|m| m.login == login) {
            // Maybe they were modded since the last check.
            list = self.moderated_channels(true).await?;
        }
        let Some(m) = list.iter().find(|m| m.login == login) else {
            return Err(format!("You're not a moderator in #{login}, so it can't be added."));
        };
        let channels = {
            let db = self.db.lock().unwrap();
            db.add_channel(&m.login, &m.id, now_ms())?;
            db.channels()?
        };
        if let Some(c) = self.client() {
            c.join(&m.login);
        }
        self.sync_eventsub(&channels);
        let added: Vec<Channel> = channels.iter().filter(|c| c.name == m.login).cloned().collect();
        self.load_unban_requests(&added).await;
        Ok(channels)
    }

    pub fn remove_channel(self: &Arc<Self>, name: &str) -> Result<Vec<Channel>, String> {
        let channels = {
            let db = self.db.lock().unwrap();
            db.remove_channel(name)?;
            db.channels()?
        };
        self.context.lock().unwrap().remove(name);
        self.raid_until.lock().unwrap().remove(name);
        if let Some(c) = self.client() {
            c.part(name);
        }
        self.sync_eventsub(&channels);
        Ok(channels)
    }

    // ---------------- raid mode ----------------

    pub fn raid_modes(&self) -> HashMap<String, i64> {
        let now = now_ms();
        let mut m = self.raid_until.lock().unwrap();
        m.retain(|_, until| *until > now);
        m.clone()
    }

    pub fn set_raid_mode(&self, channel: &str, minutes: i64) -> HashMap<String, i64> {
        {
            let mut m = self.raid_until.lock().unwrap();
            if minutes > 0 {
                m.insert(channel.to_string(), now_ms() + minutes * 60_000);
            } else {
                m.remove(channel);
            }
        }
        let modes = self.raid_modes();
        let _ = self.app.emit("raid-mode", &modes);
        modes
    }

    pub(crate) fn in_raid_mode(&self, channel: &str) -> bool {
        self.raid_until.lock().unwrap().get(channel).is_some_and(|u| *u > now_ms())
    }

    // ---------------- chat events ----------------

    pub fn handle(&self, event: ChatEvent) {
        match event {
            ChatEvent::Status(status) => {
                let info = {
                    let mut c = self.connection.lock().unwrap();
                    c.status = status;
                    if status != ConnStatus::Connected {
                        c.joined.clear();
                    }
                    c.clone()
                };
                let _ = self.app.emit("connection", info);
            }
            ChatEvent::Joined(ch) => {
                let info = {
                    let mut c = self.connection.lock().unwrap();
                    if !c.joined.contains(&ch) {
                        c.joined.push(ch);
                    }
                    c.clone()
                };
                let _ = self.app.emit("connection", info);
            }
            ChatEvent::Parted(ch) => {
                let info = {
                    let mut c = self.connection.lock().unwrap();
                    c.joined.retain(|j| j != &ch);
                    c.clone()
                };
                let _ = self.app.emit("connection", info);
            }
            ChatEvent::Chat(msg) => self.on_chat(msg),
            ChatEvent::Raid(raid) => self.on_raid(raid),
            ChatEvent::Mod(ev) => self.on_mod_event(ev),
            ChatEvent::Activity(a) => self.add_activity(Activity {
                id: 0,
                channel: a.channel,
                kind: a.kind,
                user_login: a.user_login,
                display_name: a.display_name,
                text: a.text,
                timestamp: a.timestamp,
            }),
            ChatEvent::Notice { channel, text } => {
                let _ = self.app.emit("notice", serde_json::json!({ "channel": channel, "text": text }));
            }
        }
    }

    fn on_chat(&self, msg: ChatMessage) {
        let raid_mode = self.in_raid_mode(&msg.channel);
        let created_at = self.age_cache.lock().unwrap().get(&msg.user_login).copied();
        let key = (msg.channel.clone(), msg.user_login.clone());
        let (matches, needs_age) = {
            let engine = self.engine.read().unwrap();
            let mut matches = engine.evaluate_with(&msg, EvalCtx { raid_mode, account_created_at: created_at });
            let needs_age = created_at.is_none() && engine.needs_account_age(&msg, raid_mode);
            // New-account rules flag a user once per channel, not every message.
            let kinds: Vec<Option<RuleKind>> = matches.iter().map(|m| engine.kind_of(m.rule_id)).collect();
            let mut flagged = self.new_account_flagged.lock().unwrap();
            let already = flagged.contains(&key);
            let mut i = 0;
            matches.retain(|_| {
                let keep = kinds[i] != Some(RuleKind::NewAccount) || !already;
                i += 1;
                keep
            });
            if kinds.contains(&Some(RuleKind::NewAccount)) {
                flagged.insert(key.clone());
            }
            (matches, needs_age)
        };
        // Rules in test mode only record what they would have flagged.
        let (tests, mut matches): (Vec<RuleMatch>, Vec<RuleMatch>) = matches.into_iter().partition(|m| m.test_mode);
        self.record_test_hits(&msg, tests);
        matches.sort_by(|a, b| b.severity.cmp(&a.severity));
        let severity = matches.iter().map(|m| m.severity).max();
        let line = ContextLine { display_name: msg.display_name.clone(), text: msg.text.clone(), timestamp: msg.timestamp };

        // Append this message to flags still collecting follow-up context.
        let followups: Vec<i64> = {
            let mut pending = self.pending_after.lock().unwrap();
            let list = pending.entry(msg.channel.clone()).or_default();
            let ids = list.iter().map(|(id, _)| *id).collect();
            list.iter_mut().for_each(|(_, left)| *left -= 1);
            list.retain(|(_, left)| *left > 0);
            ids
        };
        if !followups.is_empty() {
            let db = self.db.lock().unwrap();
            for id in followups {
                if db.append_context_after(id, &line).is_ok() {
                    let _ = self.app.emit("flag-context", serde_json::json!({ "id": id, "line": line }));
                }
            }
        }

        let context = {
            let mut ctx = self.context.lock().unwrap();
            let history = ctx.entry(msg.channel.clone()).or_default();
            let snapshot: Vec<ContextLine> =
                if severity.is_some() || needs_age { history.iter().cloned().collect() } else { Vec::new() };
            history.push_back(line);
            while history.len() > CONTEXT_BEFORE {
                history.pop_front();
            }
            snapshot
        };
        self.history.lock().unwrap().record(&msg, severity);

        if needs_age {
            self.age_queue.lock().unwrap().insert(msg.user_login.clone());
            let mut waiting = self.age_waiting.lock().unwrap();
            if waiting.len() < 5_000 {
                waiting.entry(key).or_insert_with(|| (msg.clone(), context.clone()));
            }
        }

        if let Some(severity) = severity {
            if created_at.is_none() {
                // Look the account up so the card can show its age.
                self.age_queue.lock().unwrap().insert(msg.user_login.clone());
            }
            let flag = Flag {
                id: 0,
                channel: msg.channel.clone(),
                message_id: msg.id.clone(),
                user_id: msg.user_id.clone(),
                user_login: msg.user_login.clone(),
                display_name: msg.display_name.clone(),
                text: msg.text.clone(),
                severity,
                matches,
                context,
                context_after: vec![],
                timestamp: msg.timestamp,
                status: "open".into(),
                resolution: None,
                kind: "rule".into(),
                external_id: None,
                meta: serde_json::json!({ "accountCreatedAt": created_at, "firstMessage": msg.is_first_msg }),
            };
            self.add_flag(flag);
        }

        self.observe_waves(&msg);
        self.pending_chat.lock().unwrap().push(LiveMessage { msg, flag_severity: severity });
    }

    pub(crate) fn record_test_hits(&self, msg: &ChatMessage, tests: Vec<RuleMatch>) {
        if tests.is_empty() {
            return;
        }
        let db = self.db.lock().unwrap();
        for m in tests {
            let _ = db.insert_rule_hit(&RuleHit {
                rule_id: m.rule_id,
                channel: msg.channel.clone(),
                user_login: msg.user_login.clone(),
                display_name: msg.display_name.clone(),
                text: msg.text.clone(),
                terms: m.terms,
                reason: m.reason,
                timestamp: msg.timestamp,
            });
        }
    }

    /// Save a new flag, start collecting follow-up context, alert if needed and send it to the UI.
    pub(crate) fn add_flag(&self, mut flag: Flag) -> Flag {
        let (settings, (level, channel_sound)) = {
            let db = self.db.lock().unwrap();
            match db.insert_flag(&flag) {
                Ok(id) => flag.id = id,
                Err(e) => eprintln!("failed to save flag: {e}"),
            }
            (db.settings(), db.channel_alerts(&flag.channel))
        };
        if flag.id > 0 && flag.kind == "rule" {
            self.pending_after.lock().unwrap().entry(flag.channel.clone()).or_default().push((flag.id, CONTEXT_AFTER));
        }
        let overrides: Vec<RuleAlert> = {
            let engine = self.engine.read().unwrap();
            flag.matches.iter().filter(|m| m.rule_id > 0).map(|m| engine.alert_for(m.rule_id, &flag.channel)).collect()
        };
        let global = GlobalAlerts { enabled: settings.notify_flags, min_severity: settings.notify_min_severity };
        let alert = flag_alerts(level, global, flag.severity, &overrides);
        let sound = alert && channel_sound && settings.play_sounds;
        if alert && self.should_notify(&flag.channel) {
            let what = match flag.kind.as_str() {
                "automod" => "AutoMod is holding a message".to_string(),
                "unbanRequest" => "New unban request".to_string(),
                "suspicious" => "Suspicious user".to_string(),
                _ => flag.matches.iter().map(|m| m.rule_name.as_str()).collect::<Vec<_>>().join(", "),
            };
            self.notify(&format!("#{}: {what}", flag.channel), &format!("{}: {}", flag.display_name, flag.text));
        }
        let _ = self.app.emit("flag", serde_json::json!({ "flag": flag, "alert": alert, "sound": sound }));
        flag
    }

    fn on_raid(&self, notice: RaidNotice) {
        let (raid, settings, (level, channel_sound)): (Raid, _, _) = {
            let db = self.db.lock().unwrap();
            let raid = match db.insert_raid(&notice) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("failed to save raid: {e}");
                    return;
                }
            };
            (raid, db.settings(), db.channel_alerts(&notice.channel))
        };
        let muted = level == crate::alerts::ChannelAlertLevel::Off;
        if settings.raid_mode_minutes > 0 {
            self.set_raid_mode(&raid.channel, settings.raid_mode_minutes);
        }
        let alert = !muted && settings.notify_raids && raid.viewers >= settings.raid_min_viewers;
        if alert {
            let mode = if settings.raid_mode_minutes > 0 {
                format!(" Raid mode on for {} min.", settings.raid_mode_minutes)
            } else {
                String::new()
            };
            self.notify(
                &format!("Raid on #{}", raid.channel),
                &format!("{} is raiding with {} viewers.{mode}", raid.from_display_name, raid.viewers),
            );
        }
        let sound = alert && channel_sound && settings.play_sounds;
        let _ = self.app.emit("raid", serde_json::json!({ "raid": raid, "alert": alert, "sound": sound }));
    }

    pub(crate) fn add_activity(&self, a: Activity) {
        if let Ok(saved) = self.db.lock().unwrap().insert_activity(&a) {
            let _ = self.app.emit("activity", &saved);
        }
    }

    /// Send Twitch's native shoutout for a raider, plus the channel's optional chat message.
    pub async fn shoutout(self: &Arc<Self>, raid_id: i64) -> Result<ShoutoutResult, String> {
        let helix = self.helix()?;
        let (raid, channel) = {
            let db = self.db.lock().unwrap();
            let raid = db.raid_by_id(raid_id)?.ok_or("That raid is gone.")?;
            let channel = db.channels()?.into_iter().find(|c| c.name == raid.channel).ok_or("That channel isn't monitored any more.")?;
            (raid, channel)
        };
        if !channel.is_mod {
            return Err(format!("You're not a moderator in #{}.", channel.name));
        }
        let from = channel.broadcaster_id.clone().ok_or("Missing the channel ID; try refreshing your channel list.")?;
        let to = match raid.from_user_id.clone() {
            Some(id) => id,
            None => helix.user_id(&raid.from_login).await.map_err(|e| e.to_string())?,
        };
        let native = helix.shoutout(&from, &to).await.map_err(|e| e.to_string());
        if native.is_ok() {
            let _ = self.db.lock().unwrap().set_raid_shoutout(raid.id, now_ms());
        }
        let message = if channel.shoutout_message.trim().is_empty() {
            None
        } else {
            let text = channel
                .shoutout_message
                .replace("{raider}", &raid.from_login)
                .replace("{raider_name}", &raid.from_display_name)
                .replace("{viewers}", &raid.viewers.to_string());
            Some(helix.send_chat_message(&from, &text).await.map(|_| text).map_err(|e| e.to_string()))
        };
        let raid = self.db.lock().unwrap().raid_by_id(raid.id)?.ok_or("That raid is gone.")?;
        Ok(ShoutoutResult { native: native.err(), message_sent: message.as_ref().and_then(|m| m.as_ref().ok().cloned()), message_error: message.and_then(|m| m.err()), raid })
    }

    pub(crate) fn should_notify(&self, channel: &str) -> bool {
        let mut last = self.last_notified.lock().unwrap();
        let now = Instant::now();
        match last.get(channel) {
            Some(t) if now.duration_since(*t) < NOTIFY_COOLDOWN => false,
            _ => {
                last.insert(channel.to_string(), now);
                true
            }
        }
    }

    pub(crate) fn notify(&self, title: &str, body: &str) {
        if let Err(e) = self.app.notification().builder().title(title).body(body).show() {
            eprintln!("notification failed: {e}");
        }
    }

    // ---------------- user history ----------------

    pub fn user_history(&self, channel: &str, login: &str) -> Result<UserHistoryView, String> {
        let (user_id, display_name, messages) = self
            .history
            .lock()
            .unwrap()
            .users
            .get(&(channel.to_string(), login.to_string()))
            .map(|(id, name, lines)| (id.clone(), name.clone(), lines.iter().rev().cloned().collect()))
            .unwrap_or((None, login.to_string(), vec![]));
        let db = self.db.lock().unwrap();
        let flags = db.flags_for_user(channel, login, 25)?;
        let user_id = user_id.or_else(|| flags.iter().find_map(|f| f.user_id.clone()));
        let other_channels = db
            .recent_sanctions(0, 5_000)?
            .into_iter()
            .filter(|a| a.target_login == login && a.channel != channel)
            .take(25)
            .collect();
        Ok(UserHistoryView {
            channel: channel.to_string(),
            login: login.to_string(),
            display_name,
            user_id,
            messages,
            flags,
            actions: db.mod_actions_for_user(channel, login, 25)?,
            other_channels,
            account_created_at: self.age_cache.lock().unwrap().get(login).copied(),
        })
    }

    // ---------------- moderation actions ----------------

    /// Run a moderation action through the Helix API and log it.
    pub async fn moderate(
        self: &Arc<Self>,
        channel: &str,
        target_login: &str,
        target_user_id: Option<String>,
        message_id: Option<String>,
        kind: ActionKind,
        reason: &str,
    ) -> Result<ModAction, String> {
        let helix = self.helix()?;
        let broadcaster_id = {
            let db = self.db.lock().unwrap();
            let is_mod = db.channels()?.iter().any(|c| c.name == channel && c.is_mod);
            if !is_mod {
                return Err(format!("You're not a moderator in #{channel}."));
            }
            db.broadcaster_id(channel)
        }
        .ok_or_else(|| format!("Missing the channel ID for #{channel}; try refreshing your channel list."))?;

        let result: Result<(), ApiError> = async {
            match kind {
                ActionKind::Delete => {
                    let id = message_id.as_deref().ok_or(ApiError::Other("No message to delete".into()))?;
                    helix.delete_message(&broadcaster_id, id).await
                }
                _ => {
                    let user_id = match target_user_id.clone() {
                        Some(id) => id,
                        None => helix.user_id(target_login).await?,
                    };
                    match kind {
                        ActionKind::Warn => helix.warn(&broadcaster_id, &user_id, reason).await,
                        ActionKind::Timeout(secs) => helix.ban(&broadcaster_id, &user_id, Some(secs), reason).await,
                        ActionKind::Ban => helix.ban(&broadcaster_id, &user_id, None, reason).await,
                        ActionKind::Unban => helix.unban(&broadcaster_id, &user_id).await,
                        ActionKind::Delete => unreachable!(),
                    }
                }
            }
        }
        .await;

        let (action, duration) = match kind {
            ActionKind::Delete => ("delete", None),
            ActionKind::Warn => ("warn", None),
            ActionKind::Timeout(s) => ("timeout", Some(s as i64)),
            ActionKind::Ban => ("ban", None),
            ActionKind::Unban => ("unban", None),
        };
        let record = ModAction {
            id: 0,
            channel: channel.to_string(),
            target_login: target_login.to_string(),
            action: action.into(),
            duration_secs: duration,
            reason: reason.trim().to_string(),
            ok: result.is_ok(),
            error: result.as_ref().err().map(|e| e.to_string()),
            timestamp: now_ms(),
            by_app: true,
            target_user_id,
        };
        let (saved, resolved) = {
            let db = self.db.lock().unwrap();
            let saved = db.insert_mod_action(&record)?;
            let resolution = match kind {
                ActionKind::Delete => Some("Message deleted".to_string()),
                ActionKind::Warn => Some("Warned".to_string()),
                ActionKind::Timeout(s) => Some(format!("Timed out {}", format_duration(s))),
                ActionKind::Ban => Some("Banned".to_string()),
                ActionKind::Unban => None,
            };
            let resolved = match (&result, resolution) {
                (Ok(()), Some(r)) => {
                    let only_msg = (kind == ActionKind::Delete).then_some(message_id.as_deref()).flatten();
                    let ids = db.resolve_flags(channel, target_login, only_msg, &r)?;
                    Some((ids, r))
                }
                _ => None,
            };
            (saved, resolved)
        };
        if let Some((ids, resolution)) = resolved {
            let _ = self.app.emit("flags-resolved", serde_json::json!({ "ids": ids, "resolution": resolution }));
        }
        let _ = self.app.emit("mod-action", &saved);

        match result {
            Ok(()) => Ok(saved),
            Err(ApiError::Unauthorized) => {
                self.end_session(Some("Your Twitch login expired. Please log in again.".into())).await;
                Err(ApiError::Unauthorized.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn durations() {
        assert_eq!(format_duration(600), "10m");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(86_400 * 7), "7d");
        assert_eq!(format_duration(90), "90s");
    }
}
