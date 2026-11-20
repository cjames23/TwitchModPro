//! Tauri commands called from the UI.

use crate::core::{now_ms, ActionKind, AuthState, ConnectionInfo, Core, UserHistoryView};
use crate::db::{Channel, Flag, ModAction, Raid, Settings};
use crate::intel::{BulkResult, ChannelResult};
use crate::waves::Wave;
use crate::irc::ChatMessage;
use crate::rules::{self, Rule, RuleEngine, RuleMatch};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tauri::State;

pub struct AppState {
    pub core: Arc<Core>,
}

/// Accepts `name`, `#name`, or a twitch.tv URL and returns a lowercase login.
pub fn normalize_channel(input: &str) -> Result<String, String> {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    channels: Vec<Channel>,
    rules: Vec<Rule>,
    settings: Settings,
    flags: Vec<Flag>,
    raids: Vec<Raid>,
    connection: ConnectionInfo,
    auth: AuthState,
    raid_modes: HashMap<String, i64>,
    waves: Vec<Wave>,
    /// Recent warns/timeouts/bans across all channels (for cross-channel notes and the ladder).
    sanctions: Vec<ModAction>,
}

#[tauri::command]
pub fn get_snapshot(state: State<AppState>) -> Result<Snapshot, String> {
    let core = &state.core;
    let auth = core.auth_state();
    let raid_modes = core.raid_modes();
    let waves = core.waves();
    let sanctions = core.sanctions()?;
    let db = core.db.lock().unwrap();
    Ok(Snapshot {
        channels: db.channels()?,
        rules: db.rules()?,
        settings: db.settings(),
        flags: db.flags(500)?,
        raids: db.raids(200)?,
        connection: core.connection.lock().unwrap().clone(),
        auth,
        raid_modes,
        waves,
        sanctions,
    })
}

// ---------------- auth ----------------

#[tauri::command]
pub fn get_auth(state: State<AppState>) -> AuthState {
    state.core.auth_state()
}

#[tauri::command]
pub async fn set_client_id(state: State<'_, AppState>, client_id: String) -> Result<AuthState, String> {
    let id = client_id.trim().to_string();
    if !id.is_empty() && (id.len() < 20 || !id.chars().all(|c| c.is_ascii_alphanumeric())) {
        return Err("That doesn't look like a Twitch Client ID (it's about 30 letters and numbers).".into());
    }
    let changed = {
        let db = state.core.db.lock().unwrap();
        let mut s = db.settings();
        let changed = s.twitch_client_id != id;
        s.twitch_client_id = id;
        db.save_settings(&s)?;
        changed
    };
    // Tokens belong to a specific Client ID, so switching IDs means logging in again.
    if changed && state.core.helix().is_ok() {
        state.core.end_session(None).await;
    }
    Ok(state.core.auth_state())
}

#[tauri::command]
pub async fn start_login(state: State<'_, AppState>) -> Result<AuthState, String> {
    state.core.start_login().await
}

#[tauri::command]
pub fn cancel_login(state: State<AppState>) -> AuthState {
    state.core.cancel_login();
    state.core.auth_state()
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<AuthState, String> {
    state.core.end_session(None).await;
    Ok(state.core.auth_state())
}

// ---------------- channels ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModChannelOption {
    login: String,
    name: String,
    added: bool,
    own: bool,
}

#[tauri::command]
pub async fn list_moderated_channels(state: State<'_, AppState>, refresh: bool) -> Result<Vec<ModChannelOption>, String> {
    let list = state.core.moderated_channels(refresh).await?;
    if refresh {
        state.core.sync_channels().await;
    }
    let own = state.core.helix().map(|h| h.account.login).unwrap_or_default();
    let added: BTreeSet<String> = state.core.db.lock().unwrap().channels()?.into_iter().map(|c| c.name).collect();
    let mut out: Vec<ModChannelOption> = list
        .into_iter()
        .map(|m| ModChannelOption { added: added.contains(&m.login), own: m.login == own, login: m.login, name: m.name })
        .collect();
    out.sort_by(|a, b| b.own.cmp(&a.own).then_with(|| a.login.cmp(&b.login)));
    Ok(out)
}

#[tauri::command]
pub async fn add_channel(state: State<'_, AppState>, name: String) -> Result<Vec<Channel>, String> {
    let name = normalize_channel(&name)?;
    state.core.add_channel(&name).await
}

#[tauri::command]
pub fn remove_channel(state: State<AppState>, name: String) -> Result<Vec<Channel>, String> {
    state.core.remove_channel(&name)
}

#[tauri::command]
pub fn set_channel_muted(state: State<AppState>, name: String, muted: bool) -> Result<Vec<Channel>, String> {
    let db = state.core.db.lock().unwrap();
    db.set_channel_muted(&name, muted)?;
    db.channels()
}

#[tauri::command]
pub fn set_channel_alerts(state: State<AppState>, name: String, level: String, sound: bool) -> Result<Vec<Channel>, String> {
    let db = state.core.db.lock().unwrap();
    db.set_channel_alerts(&name, crate::alerts::ChannelAlertLevel::parse(&level), sound)?;
    db.channels()
}

#[tauri::command]
pub fn set_channel_color(state: State<AppState>, name: String, color: Option<String>) -> Result<Vec<Channel>, String> {
    let db = state.core.db.lock().unwrap();
    db.set_channel_color(&name, color.as_deref())?;
    db.channels()
}

#[tauri::command]
pub fn set_shoutout_message(state: State<AppState>, name: String, message: String) -> Result<Vec<Channel>, String> {
    let db = state.core.db.lock().unwrap();
    db.set_shoutout_message(&name, &message)?;
    db.channels()
}

#[tauri::command]
pub async fn shoutout(state: State<'_, AppState>, raid_id: i64) -> Result<crate::core::ShoutoutResult, String> {
    state.core.shoutout(raid_id).await
}

#[tauri::command]
pub fn get_mod_log(state: State<AppState>, limit: i64) -> Result<Vec<crate::db::ModLogEntry>, String> {
    state.core.db.lock().unwrap().mod_log(limit.clamp(1, 2_000))
}

#[tauri::command]
pub fn get_activity(state: State<AppState>, limit: i64) -> Result<Vec<crate::db::Activity>, String> {
    state.core.db.lock().unwrap().activity(limit.clamp(1, 2_000))
}

#[tauri::command]
pub fn set_raid_mode(state: State<AppState>, channel: String, minutes: i64) -> HashMap<String, i64> {
    state.core.set_raid_mode(&channel, minutes.clamp(0, 24 * 60))
}

// ---------------- rules ----------------

fn rebuild_engine(core: &Core) -> Result<Vec<Rule>, String> {
    let rules = core.db.lock().unwrap().rules()?;
    *core.engine.write().unwrap() = RuleEngine::new(&rules);
    Ok(rules)
}

#[tauri::command]
pub fn save_rule(state: State<AppState>, rule: Rule) -> Result<Vec<Rule>, String> {
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
    rebuild_engine(&state.core)
}

#[tauri::command]
pub fn delete_rule(state: State<AppState>, id: i64) -> Result<Vec<Rule>, String> {
    state.core.db.lock().unwrap().delete_rule(id)?;
    rebuild_engine(&state.core)
}

fn parse_rule_alert(alert: &str) -> Result<crate::alerts::RuleAlert, String> {
    match alert {
        "default" => Ok(crate::alerts::RuleAlert::Default),
        "always" => Ok(crate::alerts::RuleAlert::Always),
        "never" => Ok(crate::alerts::RuleAlert::Never),
        other => Err(format!("Unknown alert setting `{other}`")),
    }
}

/// Set how one rule alerts in one channel ("default" removes the override).
#[tauri::command]
pub fn set_rule_channel_alert(state: State<AppState>, rule_id: i64, channel: String, alert: String) -> Result<Vec<Rule>, String> {
    set_channel_alerts_for(&state.core, &channel, &alert, |r| r.id == rule_id)
}

/// Set how every rule in a pack alerts in one channel. `pack` None = rules not from a pack; "*" = every rule.
#[tauri::command]
pub fn set_pack_channel_alert(state: State<AppState>, pack: Option<String>, channel: String, alert: String) -> Result<Vec<Rule>, String> {
    set_channel_alerts_for(&state.core, &channel, &alert, |r| pack.as_deref() == Some("*") || r.pack == pack)
}

fn set_channel_alerts_for(core: &crate::core::Core, channel: &str, alert: &str, pick: impl Fn(&Rule) -> bool) -> Result<Vec<Rule>, String> {
    let channel = normalize_channel(channel)?;
    let alert = parse_rule_alert(alert)?;
    {
        let db = core.db.lock().unwrap();
        for mut r in db.rules()?.into_iter().filter(|r| pick(r)) {
            if alert == crate::alerts::RuleAlert::Default {
                r.channel_alerts.remove(&channel);
            } else {
                r.channel_alerts.insert(channel.clone(), alert);
            }
            db.save_rule(&r)?;
        }
    }
    rebuild_engine(core)
}

/// Try a rule against sample text without saving it.
#[tauri::command]
pub fn test_rule(rule: Rule, text: String, username: Option<String>, first_message: Option<bool>) -> Result<Vec<RuleMatch>, String> {
    rules::validate(&rule)?;
    let mut rule = rule;
    rule.enabled = true;
    rule.exempt_privileged = false;
    let login = username.filter(|u| !u.trim().is_empty()).unwrap_or_else(|| "tester".into()).to_lowercase();
    let msg = ChatMessage {
        id: "test".into(),
        channel: rule.channels.first().cloned().unwrap_or_default(),
        room_id: None,
        user_id: None,
        display_name: login.clone(),
        user_login: login,
        color: None,
        text,
        is_action: false,
        is_mod: false,
        is_broadcaster: false,
        is_vip: false,
        is_subscriber: false,
        is_first_msg: first_message.unwrap_or(false),
        emote_count: 0,
        reply_to: None,
        timestamp: now_ms(),
    };
    let engine = RuleEngine::new(&[rule.clone()]);
    // Repeat rules need several messages; send the sample enough times to show what happens.
    if rule.kind == rules::RuleKind::Repeat {
        for _ in 1..rule.params.repeat_count {
            engine.evaluate(&msg, true);
        }
    }
    Ok(engine.evaluate(&msg, true))
}

/// How each rule has performed over the last `days` days.
#[tauri::command]
pub fn rule_stats(state: State<AppState>, days: i64) -> Result<Vec<crate::db::RuleStats>, String> {
    let since = now_ms() - days.clamp(1, 90) * 86_400_000;
    state.core.db.lock().unwrap().rule_stats(since, 8)
}

/// Re-add any default pack rules that were deleted (in test mode).
#[tauri::command]
pub fn restore_default_packs(state: State<AppState>) -> Result<Vec<Rule>, String> {
    state.core.db.lock().unwrap().restore_default_packs()?;
    rebuild_engine(&state.core)
}

// ---------------- flags & raids ----------------

#[tauri::command]
pub fn set_flag_status(state: State<AppState>, id: i64, status: String) -> Result<(), String> {
    state.core.db.lock().unwrap().set_flag_status(id, &status)
}

#[tauri::command]
pub fn clear_closed_flags(state: State<AppState>) -> Result<Vec<Flag>, String> {
    let db = state.core.db.lock().unwrap();
    db.clear_closed_flags()?;
    db.flags(500)
}

#[tauri::command]
pub fn ack_raids(state: State<AppState>) -> Result<(), String> {
    state.core.db.lock().unwrap().ack_raids()
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: Settings) -> Result<Settings, String> {
    let db = state.core.db.lock().unwrap();
    // The Client ID is changed through `set_client_id`, never by the general settings form.
    let mut settings = settings;
    settings.twitch_client_id = db.settings().twitch_client_id;
    settings.raid_mode_minutes = settings.raid_mode_minutes.clamp(0, 120);
    settings.ladder_lookback_days = settings.ladder_lookback_days.clamp(1, 365);
    settings.wave_min_accounts = settings.wave_min_accounts.clamp(2, 100);
    settings.wave_window_secs = settings.wave_window_secs.clamp(10, 600);
    settings.ladder.retain(|s| matches!(s.action.as_str(), "warn" | "timeout" | "ban"));
    if settings.ladder.is_empty() {
        settings.ladder = Settings::default().ladder;
    }
    db.save_settings(&settings)?;
    *state.core.wave_cfg.lock().unwrap() = crate::core::wave_config(&settings);
    Ok(db.settings())
}

// ---------------- moderation ----------------

#[tauri::command]
pub async fn get_user_history(state: State<'_, AppState>, channel: String, login: String) -> Result<UserHistoryView, String> {
    let login = login.to_lowercase();
    let mut view = state.core.user_history(&channel, &login)?;
    if view.account_created_at.is_none() {
        view.account_created_at = state.core.account_created_at(&login).await;
    }
    Ok(view)
}

/// Known account creation times; unknown ones are looked up and arrive later as `account-ages` events.
#[tauri::command]
pub fn get_account_ages(state: State<AppState>, logins: Vec<String>) -> HashMap<String, i64> {
    let logins: Vec<String> = logins.into_iter().map(|l| l.to_lowercase()).take(500).collect();
    state.core.account_ages(&logins)
}

#[tauri::command]
pub async fn warn_user(
    state: State<'_, AppState>,
    channel: String,
    user_login: String,
    user_id: Option<String>,
    reason: String,
) -> Result<ModAction, String> {
    state.core.moderate(&channel, &user_login, user_id, None, ActionKind::Warn, &reason).await
}

#[tauri::command]
pub async fn ban_everywhere(
    state: State<'_, AppState>,
    user_login: String,
    user_id: Option<String>,
    reason: String,
) -> Result<Vec<ChannelResult>, String> {
    state.core.ban_everywhere(&user_login, user_id, &reason).await
}

#[tauri::command]
pub async fn wave_action(
    state: State<'_, AppState>,
    id: i64,
    action: String,
    seconds: Option<u32>,
    reason: String,
    exclude: Vec<String>,
) -> Result<BulkResult, String> {
    let kind = match (action.as_str(), seconds) {
        ("ban", _) => ActionKind::Ban,
        ("timeout", Some(s)) if (1..=1_209_600).contains(&s) => ActionKind::Timeout(s),
        _ => return Err("Choose ban or a timeout length.".into()),
    };
    state.core.wave_action(id, kind, &reason, &exclude).await
}

#[tauri::command]
pub async fn wave_undo(state: State<'_, AppState>, id: i64) -> Result<BulkResult, String> {
    state.core.wave_undo(id).await
}

#[tauri::command]
pub fn dismiss_wave(state: State<AppState>, id: i64) -> Result<Wave, String> {
    state.core.dismiss_wave(id).ok_or_else(|| "That spam wave is gone.".into())
}

#[tauri::command]
pub async fn automod_decide(state: State<'_, AppState>, flag_id: i64, allow: bool) -> Result<(), String> {
    state.core.automod_decide(flag_id, allow).await
}

#[tauri::command]
pub async fn unban_request_decide(
    state: State<'_, AppState>,
    flag_id: i64,
    approve: bool,
    text: String,
) -> Result<(), String> {
    state.core.unban_request_decide(flag_id, approve, &text).await
}

#[tauri::command]
pub async fn delete_message(
    state: State<'_, AppState>,
    channel: String,
    user_login: String,
    message_id: String,
) -> Result<ModAction, String> {
    state.core.moderate(&channel, &user_login, None, Some(message_id), ActionKind::Delete, "").await
}

#[tauri::command]
pub async fn timeout_user(
    state: State<'_, AppState>,
    channel: String,
    user_login: String,
    user_id: Option<String>,
    seconds: u32,
    reason: String,
) -> Result<ModAction, String> {
    if !(1..=1_209_600).contains(&seconds) {
        return Err("Timeouts must be between 1 second and 14 days.".into());
    }
    state.core.moderate(&channel, &user_login, user_id, None, ActionKind::Timeout(seconds), &reason).await
}

#[tauri::command]
pub async fn ban_user(
    state: State<'_, AppState>,
    channel: String,
    user_login: String,
    user_id: Option<String>,
    reason: String,
) -> Result<ModAction, String> {
    state.core.moderate(&channel, &user_login, user_id, None, ActionKind::Ban, &reason).await
}

#[tauri::command]
pub async fn unban_user(
    state: State<'_, AppState>,
    channel: String,
    user_login: String,
    user_id: Option<String>,
) -> Result<ModAction, String> {
    state.core.moderate(&channel, &user_login, user_id, None, ActionKind::Unban, "").await
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
