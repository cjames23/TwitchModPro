mod db;
mod irc;
mod rules;
mod twitch;

use db::{Channel, ContextLine, Db, Flag, Raid, Settings};
use irc::{ChatMessage, RaidNotice};
use rules::{Rule, RuleEngine, Severity};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use twitch::{ChatClient, ChatEvent, ConnStatus};

/// How many earlier messages from the same channel are saved with a flag.
const CONTEXT_LINES: usize = 8;
/// Minimum gap between desktop notifications for the same channel.
const NOTIFY_COOLDOWN: Duration = Duration::from_secs(4);
/// Live chat is sent to the UI in batches at this interval.
const CHAT_FLUSH: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveMessage {
    #[serde(flatten)]
    msg: ChatMessage,
    flag_severity: Option<Severity>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionInfo {
    status: ConnStatus,
    joined: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FlagEvent {
    flag: Flag,
    /// False when the channel is muted or the severity is below the alert threshold.
    alert: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RaidEvent {
    raid: Raid,
    alert: bool,
}

struct Core {
    app: AppHandle,
    db: Mutex<Db>,
    engine: RwLock<RuleEngine>,
    context: Mutex<HashMap<String, VecDeque<ContextLine>>>,
    pending_chat: Mutex<Vec<LiveMessage>>,
    connection: Mutex<ConnectionInfo>,
    last_notified: Mutex<HashMap<String, Instant>>,
}

struct AppState {
    core: Arc<Core>,
    client: OnceLock<ChatClient>,
}

impl AppState {
    fn client(&self) -> Result<&ChatClient, String> {
        self.client.get().ok_or_else(|| "Chat client is not ready yet".into())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Accepts `name`, `#name`, or a twitch.tv URL and returns a lowercase login.
fn normalize_channel(input: &str) -> Result<String, String> {
    let mut s = input.trim().to_lowercase();
    for prefix in ["https://", "http://", "www.", "twitch.tv/", "m.twitch.tv/", "#"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    let s = s.split(['/', '?']).next().unwrap_or_default().to_string();
    let valid = (1..=25).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid { Ok(s) } else { Err(format!("`{}` isn't a valid Twitch channel name", input.trim())) }
}

impl Core {
    fn handle(&self, event: ChatEvent) {
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
            ChatEvent::Notice { channel, text } => {
                let _ = self.app.emit("notice", serde_json::json!({ "channel": channel, "text": text }));
            }
        }
    }

    fn on_chat(&self, msg: ChatMessage) {
        let matches = self.engine.read().unwrap().evaluate(&msg);
        let severity = matches.iter().map(|m| m.severity).max();

        // Grab context before adding this message to the channel's history.
        let context = {
            let mut ctx = self.context.lock().unwrap();
            let history = ctx.entry(msg.channel.clone()).or_default();
            let snapshot: Vec<ContextLine> =
                if severity.is_some() { history.iter().cloned().collect() } else { Vec::new() };
            history.push_back(ContextLine {
                display_name: msg.display_name.clone(),
                text: msg.text.clone(),
                timestamp: msg.timestamp,
            });
            while history.len() > CONTEXT_LINES {
                history.pop_front();
            }
            snapshot
        };

        if let Some(severity) = severity {
            let mut flag = Flag {
                id: 0,
                channel: msg.channel.clone(),
                message_id: msg.id.clone(),
                user_login: msg.user_login.clone(),
                display_name: msg.display_name.clone(),
                text: msg.text.clone(),
                severity,
                matches,
                context,
                timestamp: msg.timestamp,
                status: "open".into(),
            };
            let (settings, muted) = {
                let db = self.db.lock().unwrap();
                match db.insert_flag(&flag) {
                    Ok(id) => flag.id = id,
                    Err(e) => eprintln!("failed to save flag: {e}"),
                }
                (db.settings(), db.is_muted(&flag.channel))
            };
            let alert = !muted && settings.notify_flags && severity >= settings.notify_min_severity;
            if alert && self.should_notify(&flag.channel) {
                let rules: Vec<&str> = flag.matches.iter().map(|m| m.rule_name.as_str()).collect();
                self.notify(
                    &format!("Flagged in #{} ({})", flag.channel, rules.join(", ")),
                    &format!("{}: {}", flag.display_name, flag.text),
                );
            }
            let _ = self.app.emit("flag", FlagEvent { flag, alert });
        }

        self.pending_chat.lock().unwrap().push(LiveMessage { msg, flag_severity: severity });
    }

    fn on_raid(&self, notice: RaidNotice) {
        let (raid, settings, muted) = {
            let db = self.db.lock().unwrap();
            let raid = match db.insert_raid(&notice) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("failed to save raid: {e}");
                    return;
                }
            };
            (raid, db.settings(), db.is_muted(&notice.channel))
        };
        let alert = !muted && settings.notify_raids && raid.viewers >= settings.raid_min_viewers;
        if alert {
            self.notify(
                &format!("Raid on #{}", raid.channel),
                &format!("{} is raiding with {} viewers", raid.from_display_name, raid.viewers),
            );
        }
        let _ = self.app.emit("raid", RaidEvent { raid, alert });
    }

    fn should_notify(&self, channel: &str) -> bool {
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

    fn notify(&self, title: &str, body: &str) {
        if let Err(e) = self.app.notification().builder().title(title).body(body).show() {
            eprintln!("notification failed: {e}");
        }
    }

    fn rebuild_engine(&self) -> Result<Vec<Rule>, String> {
        let rules = self.db.lock().unwrap().rules()?;
        *self.engine.write().unwrap() = RuleEngine::new(&rules);
        Ok(rules)
    }
}

// ---------------- commands ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    channels: Vec<Channel>,
    rules: Vec<Rule>,
    settings: Settings,
    flags: Vec<Flag>,
    raids: Vec<Raid>,
    connection: ConnectionInfo,
}

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Result<Snapshot, String> {
    let core = &state.core;
    let db = core.db.lock().unwrap();
    Ok(Snapshot {
        channels: db.channels()?,
        rules: db.rules()?,
        settings: db.settings(),
        flags: db.flags(500)?,
        raids: db.raids(200)?,
        connection: core.connection.lock().unwrap().clone(),
    })
}

#[tauri::command]
fn add_channel(state: State<AppState>, name: String) -> Result<Vec<Channel>, String> {
    let name = normalize_channel(&name)?;
    let channels = {
        let db = state.core.db.lock().unwrap();
        db.add_channel(&name, now_ms())?;
        db.channels()?
    };
    state.client()?.join(&name);
    Ok(channels)
}

#[tauri::command]
fn remove_channel(state: State<AppState>, name: String) -> Result<Vec<Channel>, String> {
    let channels = {
        let db = state.core.db.lock().unwrap();
        db.remove_channel(&name)?;
        db.channels()?
    };
    state.core.context.lock().unwrap().remove(&name);
    state.client()?.part(&name);
    Ok(channels)
}

#[tauri::command]
fn set_channel_muted(state: State<AppState>, name: String, muted: bool) -> Result<Vec<Channel>, String> {
    let db = state.core.db.lock().unwrap();
    db.set_channel_muted(&name, muted)?;
    db.channels()
}

#[tauri::command]
fn save_rule(state: State<AppState>, rule: Rule) -> Result<Vec<Rule>, String> {
    rules::validate(&rule)?;
    let mut rule = rule;
    rule.channels = rule
        .channels
        .iter()
        .filter(|c| !c.trim().is_empty())
        .map(|c| normalize_channel(c))
        .collect::<Result<BTreeSet<_>, _>>()?
        .into_iter()
        .collect();
    state.core.db.lock().unwrap().save_rule(&rule)?;
    state.core.rebuild_engine()
}

#[tauri::command]
fn delete_rule(state: State<AppState>, id: i64) -> Result<Vec<Rule>, String> {
    state.core.db.lock().unwrap().delete_rule(id)?;
    state.core.rebuild_engine()
}

/// Try a rule against sample text without saving it. Returns the matched terms.
#[tauri::command]
fn test_rule(rule: Rule, text: String, channel: Option<String>) -> Result<Vec<String>, String> {
    rules::validate(&rule)?;
    let mut rule = rule;
    rule.enabled = true;
    let channel = channel.unwrap_or_else(|| rule.channels.first().cloned().unwrap_or_default());
    let msg = ChatMessage {
        id: "test".into(),
        channel,
        user_login: "tester".into(),
        display_name: "tester".into(),
        color: None,
        text,
        is_action: false,
        is_mod: false,
        is_broadcaster: false,
        is_vip: false,
        is_subscriber: false,
        is_first_msg: false,
        timestamp: now_ms(),
    };
    Ok(RuleEngine::new(&[rule]).evaluate(&msg).into_iter().flat_map(|m| m.terms).collect())
}

#[tauri::command]
fn set_flag_status(state: State<AppState>, id: i64, status: String) -> Result<(), String> {
    state.core.db.lock().unwrap().set_flag_status(id, &status)
}

#[tauri::command]
fn clear_closed_flags(state: State<AppState>) -> Result<Vec<Flag>, String> {
    let db = state.core.db.lock().unwrap();
    db.clear_closed_flags()?;
    db.flags(500)
}

#[tauri::command]
fn ack_raids(state: State<AppState>) -> Result<(), String> {
    state.core.db.lock().unwrap().ack_raids()
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Settings) -> Result<Settings, String> {
    let db = state.core.db.lock().unwrap();
    db.save_settings(&settings)?;
    Ok(db.settings())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = Db::open(&dir.join("twitch-mod-pro.sqlite3"))?;
            let rules = db.rules()?;
            let channels: Vec<String> = db.channels()?.into_iter().map(|c| c.name).collect();

            let core = Arc::new(Core {
                app: app.handle().clone(),
                db: Mutex::new(db),
                engine: RwLock::new(RuleEngine::new(&rules)),
                context: Mutex::new(HashMap::new()),
                pending_chat: Mutex::new(Vec::new()),
                connection: Mutex::new(ConnectionInfo { status: ConnStatus::Connecting, joined: vec![] }),
                last_notified: Mutex::new(HashMap::new()),
            });

            let state = AppState { core: core.clone(), client: OnceLock::new() };
            let sink_core = core.clone();
            let client = ChatClient::start(channels, Arc::new(move |e| sink_core.handle(e)));
            let _ = state.client.set(client);
            app.manage(state);

            // Flush live chat to the UI in batches so busy channels don't flood it.
            let flush_core = core;
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(CHAT_FLUSH);
                loop {
                    tick.tick().await;
                    let batch = std::mem::take(&mut *flush_core.pending_chat.lock().unwrap());
                    if !batch.is_empty() {
                        let _ = flush_core.app.emit("chat-batch", batch);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            add_channel,
            remove_channel,
            set_channel_muted,
            save_rule,
            delete_rule,
            test_rule,
            set_flag_status,
            clear_closed_flags,
            ack_raids,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::normalize_channel;

    #[test]
    fn normalizes_channel_input() {
        assert_eq!(normalize_channel("  #CoolStreamer ").unwrap(), "coolstreamer");
        assert_eq!(normalize_channel("https://www.twitch.tv/Some_Guy/videos").unwrap(), "some_guy");
        assert!(normalize_channel("bad name").is_err());
        assert!(normalize_channel("").is_err());
    }
}
