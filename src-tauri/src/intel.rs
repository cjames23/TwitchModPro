//! Cross-channel intelligence and bulk moderation:
//! account ages, other mods' actions, spam waves, the Twitch inbox (AutoMod, unban
//! requests, suspicious users), ban-everywhere and bulk wave actions.

use crate::core::{now_ms, ActionKind, Core};
use crate::db::{Channel, ContextLine, Flag, ModAction};
use crate::eventsub::{EventSubClient, SubEvent};
use crate::irc::{ChatMessage, ModEvent};
use crate::rules::{EvalCtx, RuleKind, RuleMatch, Severity};
use crate::twitch_api::ApiError;
use crate::waves::{Wave, WaveConfig};
use futures_util::{stream, StreamExt};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Weak};
use tauri::Emitter;

/// How many bulk actions run at once.
const BULK_CONCURRENCY: usize = 4;
/// If this app did the same action this recently, the echo from chat is ours.
const ECHO_WINDOW_MS: i64 = 20_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelResult {
    pub channel: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkResult {
    pub succeeded: usize,
    pub failed: Vec<(String, String)>,
    pub wave: Option<Wave>,
}

fn duration_label(secs: u32) -> String {
    crate::core::format_duration(secs)
}

fn s(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

impl Core {
    // ---------------- account age ----------------

    /// Look up queued accounts (up to 100 per call), then run new-account rules on held messages.
    pub async fn resolve_ages(self: &Arc<Self>) {
        let batch: Vec<String> = {
            let cache = self.age_cache.lock().unwrap();
            let mut q = self.age_queue.lock().unwrap();
            q.retain(|l| !cache.contains_key(l));
            let batch: Vec<String> = q.iter().take(100).cloned().collect();
            for l in &batch {
                q.remove(l);
            }
            batch
        };
        if batch.is_empty() {
            return;
        }
        let Ok(helix) = self.helix() else {
            self.age_waiting.lock().unwrap().clear();
            return;
        };
        let found = match helix.users(&batch).await {
            Ok(users) => users,
            Err(e) => {
                eprintln!("account lookup failed: {e}");
                vec![]
            }
        };
        let ages: HashMap<String, i64> = found.into_iter().map(|u| (u.login, u.created_at)).collect();
        {
            let mut cache = self.age_cache.lock().unwrap();
            if cache.len() > 200_000 {
                cache.clear();
            }
            cache.extend(ages.clone());
        }
        if !ages.is_empty() {
            let _ = self.app.emit("account-ages", &ages);
        }

        // Messages that were waiting for these accounts.
        let held: Vec<(ChatMessage, Vec<ContextLine>)> = {
            let mut waiting = self.age_waiting.lock().unwrap();
            let keys: Vec<(String, String)> = waiting.keys().filter(|(_, l)| batch.contains(l)).cloned().collect();
            keys.into_iter().filter_map(|k| waiting.remove(&k)).collect()
        };
        for (msg, context) in held {
            let Some(created) = ages.get(&msg.user_login).copied() else { continue };
            let key = (msg.channel.clone(), msg.user_login.clone());
            if self.new_account_flagged.lock().unwrap().contains(&key) {
                continue;
            }
            let all: Vec<RuleMatch> = {
                let engine = self.engine.read().unwrap();
                engine
                    .evaluate_with(&msg, EvalCtx { raid_mode: self.in_raid_mode(&msg.channel), account_created_at: Some(created) })
                    .into_iter()
                    .filter(|m| engine.kind_of(m.rule_id) == Some(RuleKind::NewAccount))
                    .collect()
            };
            let (tests, matches): (Vec<RuleMatch>, Vec<RuleMatch>) = all.into_iter().partition(|m| m.test_mode);
            self.record_test_hits(&msg, tests);
            let Some(severity) = matches.iter().map(|m| m.severity).max() else { continue };
            self.new_account_flagged.lock().unwrap().insert(key);
            self.add_flag(Flag {
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
                meta: serde_json::json!({ "accountCreatedAt": created, "firstMessage": msg.is_first_msg }),
            });
        }
    }

    /// Queue lookups for these accounts and return the ones already known.
    pub fn account_ages(&self, logins: &[String]) -> HashMap<String, i64> {
        let cache = self.age_cache.lock().unwrap();
        let mut q = self.age_queue.lock().unwrap();
        let mut out = HashMap::new();
        for l in logins {
            match cache.get(l) {
                Some(t) => {
                    out.insert(l.clone(), *t);
                }
                None => {
                    q.insert(l.clone());
                }
            }
        }
        out
    }

    pub async fn account_created_at(&self, login: &str) -> Option<i64> {
        if let Some(t) = self.age_cache.lock().unwrap().get(login) {
            return Some(*t);
        }
        let helix = self.helix().ok()?;
        let user = helix.users(&[login.to_string()]).await.ok()?.into_iter().next()?;
        self.age_cache.lock().unwrap().insert(user.login, user.created_at);
        Some(user.created_at)
    }

    // ---------------- other mods' actions (seen in chat) ----------------

    pub(crate) fn on_mod_event(&self, ev: ModEvent) {
        match ev {
            ModEvent::UserCleared { channel, login, user_id, duration_secs, timestamp } => {
                let action = if duration_secs.is_some() { "timeout" } else { "ban" };
                let (record, resolved) = {
                    let db = self.db.lock().unwrap();
                    if db.app_action_recent(&channel, &login, action, now_ms() - ECHO_WINDOW_MS) {
                        return; // our own action, already logged
                    }
                    let record = db
                        .insert_mod_action(&ModAction {
                            id: 0,
                            channel: channel.clone(),
                            target_login: login.clone(),
                            action: action.into(),
                            duration_secs: duration_secs.map(i64::from),
                            reason: String::new(),
                            ok: true,
                            error: None,
                            timestamp,
                            by_app: false,
                            target_user_id: user_id,
                        })
                        .ok();
                    let resolution = match duration_secs {
                        Some(d) => format!("Timed out {} by a mod", duration_label(d)),
                        None => "Banned by a mod".into(),
                    };
                    let ids = db.resolve_flags(&channel, &login, None, &resolution).unwrap_or_default();
                    (record, (ids, resolution))
                };
                if let Some(r) = record {
                    let _ = self.app.emit("mod-action", &r);
                }
                if !resolved.0.is_empty() {
                    let _ = self.app.emit("flags-resolved", serde_json::json!({ "ids": resolved.0, "resolution": resolved.1 }));
                }
            }
            ModEvent::MessageDeleted { channel, login, message_id } => {
                let ids = self
                    .db
                    .lock()
                    .unwrap()
                    .resolve_flags(&channel, &login, Some(&message_id), "Deleted by a mod")
                    .unwrap_or_default();
                if !ids.is_empty() {
                    let _ = self.app.emit("flags-resolved", serde_json::json!({ "ids": ids, "resolution": "Deleted by a mod" }));
                }
                let _ = self.app.emit("message-deleted", serde_json::json!({ "channel": channel, "messageId": message_id }));
            }
        }
    }

    pub fn sanctions(&self) -> Result<Vec<ModAction>, String> {
        let db = self.db.lock().unwrap();
        let days = db.settings().ladder_lookback_days.clamp(1, 365);
        db.recent_sanctions(now_ms() - days * 86_400_000, 5_000)
    }

    // ---------------- spam waves ----------------

    pub(crate) fn observe_waves(&self, msg: &ChatMessage) {
        let cfg: WaveConfig = *self.wave_cfg.lock().unwrap();
        let changed = self.waves.lock().unwrap().observe(msg, cfg);
        for wave in changed {
            let new = self.wave_notified.lock().unwrap().insert(wave.id);
            let (alert, sound) = {
                let db = self.db.lock().unwrap();
                let levels: Vec<_> = wave.channels.iter().map(|c| db.channel_alerts(c)).collect();
                let alert = levels.iter().any(|(l, _)| *l != crate::alerts::ChannelAlertLevel::Off);
                (alert, alert && db.settings().play_sounds && levels.iter().any(|(l, s)| *s && *l != crate::alerts::ChannelAlertLevel::Off))
            };
            if new {
                if alert && self.should_notify(&format!("wave:{}", wave.key)) {
                    let chans: Vec<String> = wave.channels.iter().map(|c| format!("#{c}")).collect();
                    self.notify(
                        &format!("Spam wave in {}", chans.join(", ")),
                        &format!("{} accounts {}", wave.members.len(), describe_wave(&wave)),
                    );
                }
            }
            let _ = self.app.emit("wave", serde_json::json!({ "wave": wave, "new": new, "sound": new && sound }));
        }
    }

    pub fn waves(&self) -> Vec<Wave> {
        self.waves.lock().unwrap().waves().to_vec()
    }

    pub fn dismiss_wave(&self, id: i64) -> Option<Wave> {
        let w = self.waves.lock().unwrap().dismiss(id, now_ms());
        if let Some(w) = &w {
            let _ = self.app.emit("wave", serde_json::json!({ "wave": w, "new": false }));
        }
        w
    }

    /// Ban or time out every member of a wave (except `exclude`), each in the channel they posted in.
    pub async fn wave_action(
        self: &Arc<Self>,
        id: i64,
        kind: ActionKind,
        reason: &str,
        exclude: &[String],
    ) -> Result<BulkResult, String> {
        let members: Vec<(String, String, Option<String>)> = {
            let waves = self.waves.lock().unwrap();
            let w = waves.waves().iter().find(|w| w.id == id).ok_or("That spam wave is gone.")?;
            w.members
                .iter()
                .filter(|m| !exclude.contains(&m.login))
                .filter(|m| m.outcome.as_deref().is_none_or(|o| o.starts_with("failed") || o == "unbanned"))
                .map(|m| (m.channel.clone(), m.login.clone(), m.user_id.clone()))
                .collect()
        };
        let outcome_ok = match kind {
            ActionKind::Ban => "banned".to_string(),
            ActionKind::Timeout(s) => format!("timed out {}", duration_label(s)),
            _ => return Err("Only ban and timeout can be done in bulk.".into()),
        };
        let results: Vec<((String, String), Result<ModAction, String>)> = stream::iter(members)
            .map(|(channel, login, user_id)| {
                let core = self.clone();
                let reason = reason.to_string();
                async move {
                    let r = core.moderate(&channel, &login, user_id, None, kind, &reason).await;
                    ((channel, login), r)
                }
            })
            .buffer_unordered(BULK_CONCURRENCY)
            .collect()
            .await;
        self.finish_bulk(id, results, &outcome_ok, "actioned")
    }

    /// Unban everyone this app banned or timed out from a wave.
    pub async fn wave_undo(self: &Arc<Self>, id: i64) -> Result<BulkResult, String> {
        let members: Vec<(String, String, Option<String>)> = {
            let waves = self.waves.lock().unwrap();
            let w = waves.waves().iter().find(|w| w.id == id).ok_or("That spam wave is gone.")?;
            w.members
                .iter()
                .filter(|m| m.outcome.as_deref().is_some_and(|o| o == "banned" || o.starts_with("timed out")))
                .map(|m| (m.channel.clone(), m.login.clone(), m.user_id.clone()))
                .collect()
        };
        let results = stream::iter(members)
            .map(|(channel, login, user_id)| {
                let core = self.clone();
                async move {
                    let r = core.moderate(&channel, &login, user_id, None, ActionKind::Unban, "").await;
                    ((channel, login), r)
                }
            })
            .buffer_unordered(BULK_CONCURRENCY)
            .collect()
            .await;
        self.finish_bulk(id, results, "unbanned", "open")
    }

    fn finish_bulk(
        &self,
        id: i64,
        results: Vec<((String, String), Result<ModAction, String>)>,
        outcome_ok: &str,
        status: &str,
    ) -> Result<BulkResult, String> {
        let mut failed = Vec::new();
        let mut succeeded = 0;
        let wave = {
            let mut waves = self.waves.lock().unwrap();
            let w = waves.get_mut(id);
            let mut wave = None;
            if let Some(w) = w {
                for ((channel, login), r) in &results {
                    if let Some(m) = w.members.iter_mut().find(|m| &m.login == login && &m.channel == channel) {
                        m.outcome = Some(match r {
                            Ok(_) => outcome_ok.to_string(),
                            Err(e) => format!("failed: {e}"),
                        });
                    }
                }
                w.status = status.into();
                wave = Some(w.clone());
            }
            wave
        };
        for ((_, login), r) in results {
            match r {
                Ok(_) => succeeded += 1,
                Err(e) => failed.push((login, e)),
            }
        }
        if let Some(w) = &wave {
            let _ = self.app.emit("wave", serde_json::json!({ "wave": w, "new": false }));
        }
        Ok(BulkResult { succeeded, failed, wave })
    }

    // ---------------- ban everywhere ----------------

    /// Ban a user in every monitored channel where you're a mod.
    pub async fn ban_everywhere(
        self: &Arc<Self>,
        login: &str,
        user_id: Option<String>,
        reason: &str,
    ) -> Result<Vec<ChannelResult>, String> {
        let channels: Vec<String> =
            self.db.lock().unwrap().channels()?.into_iter().filter(|c| c.is_mod).map(|c| c.name).collect();
        let user_id = match user_id {
            Some(id) => Some(id),
            None => self.helix()?.user_id(login).await.ok(),
        };
        let mut out = Vec::new();
        for channel in channels {
            let r = self.moderate(&channel, login, user_id.clone(), None, ActionKind::Ban, reason).await;
            out.push(ChannelResult { channel, ok: r.is_ok(), error: r.err() });
        }
        Ok(out)
    }

    // ---------------- Twitch inbox (EventSub) ----------------

    /// Keep EventSub subscribed to exactly the monitored channels you moderate.
    pub(crate) fn sync_eventsub(self: &Arc<Self>, channels: &[Channel]) {
        let Ok(helix) = self.helix() else { return };
        let ids: BTreeSet<String> = channels.iter().filter(|c| c.is_mod).filter_map(|c| c.broadcaster_id.clone()).collect();
        let mut es = self.eventsub.lock().unwrap();
        match es.as_ref() {
            Some(client) => client.set_channels(ids),
            None => {
                let weak: Weak<Core> = Arc::downgrade(self);
                *es = Some(EventSubClient::start(
                    helix,
                    ids,
                    Arc::new(move |ev| {
                        if let Some(core) = weak.upgrade() {
                            core.on_sub_event(ev);
                        }
                    }),
                ));
            }
        }
    }

    fn channel_for_broadcaster(&self, broadcaster_id: &str) -> Option<String> {
        self.db
            .lock()
            .unwrap()
            .channels()
            .ok()?
            .into_iter()
            .find(|c| c.broadcaster_id.as_deref() == Some(broadcaster_id))
            .map(|c| c.name)
    }

    fn on_sub_event(&self, ev: SubEvent) {
        match ev {
            SubEvent::Connected(connected) => {
                let _ = self.app.emit("inbox-status", serde_json::json!({ "connected": connected }));
            }
            SubEvent::SubscribeFailed { broadcaster_id, kinds, error } => {
                let ch = self.channel_for_broadcaster(&broadcaster_id).unwrap_or(broadcaster_id);
                let _ = self.app.emit(
                    "notice",
                    serde_json::json!({ "channel": ch, "text": format!("Couldn't turn on {} ({error})", kinds.join(", ")) }),
                );
            }
            SubEvent::Notification { kind, event } => self.on_notification(&kind, &event),
        }
    }

    fn on_notification(&self, kind: &str, e: &Value) {
        let channel = s(e, "broadcaster_user_login");
        match kind {
            "automod.message.hold" => {
                let message_id = s(e, "message_id");
                if self.db.lock().unwrap().flag_by_external("automod", &message_id).is_some() {
                    return;
                }
                let (category, level) = if e["automod"].is_object() {
                    (s(&e["automod"], "category"), e["automod"]["level"].as_i64())
                } else {
                    (s(e, "category"), e["level"].as_i64())
                };
                let blocked = e["reason"].as_str() == Some("blocked_term");
                let severity = match (blocked, level) {
                    (true, _) => Severity::Medium,
                    (_, Some(l)) if l >= 4 => Severity::High,
                    (_, Some(l)) if l >= 2 => Severity::Medium,
                    _ => Severity::Low,
                };
                let reason = if blocked {
                    "Contains a blocked term".to_string()
                } else {
                    format!("AutoMod: {}{}", category.replace('_', " "), level.map(|l| format!(" (level {l})")).unwrap_or_default())
                };
                self.add_flag(Flag {
                    id: 0,
                    channel,
                    message_id: message_id.clone(),
                    user_id: Some(s(e, "user_id")),
                    user_login: s(e, "user_login"),
                    display_name: s(e, "user_name"),
                    text: s(&e["message"], "text"),
                    severity,
                    matches: vec![RuleMatch { rule_id: 0, rule_name: "AutoMod".into(), severity, terms: vec![], patterns: vec![], test_mode: false, wellbeing: false, reason: Some(reason) }],
                    context: vec![],
                    context_after: vec![],
                    timestamp: now_ms(),
                    status: "open".into(),
                    resolution: None,
                    kind: "automod".into(),
                    external_id: Some(message_id),
                    meta: serde_json::json!({ "category": category, "level": level, "blockedTerm": blocked }),
                });
            }
            "automod.message.update" => {
                let by = e["moderator_user_login"].as_str().map(|m| format!(" by {m}")).unwrap_or_default();
                let resolution = match e["status"].as_str().unwrap_or_default().to_lowercase().as_str() {
                    "approved" => format!("Approved{by}"),
                    "denied" => format!("Denied{by}"),
                    "expired" => "Expired".into(),
                    _ => return,
                };
                self.resolve_external("automod", &s(e, "message_id"), &resolution);
            }
            "channel.unban_request.create" => {
                let req = crate::twitch_api::UnbanRequest {
                    id: s(e, "id"),
                    channel,
                    user_id: s(e, "user_id"),
                    user_login: s(e, "user_login"),
                    user_name: s(e, "user_name"),
                    text: s(e, "text"),
                    created_at: crate::twitch_api::parse_rfc3339_ms(&s(e, "created_at")).unwrap_or_else(now_ms),
                };
                self.add_unban_request(req);
            }
            "channel.unban_request.resolve" => {
                let by = e["moderator_user_login"].as_str().map(|m| format!(" by {m}")).unwrap_or_default();
                let resolution = match e["status"].as_str().unwrap_or_default() {
                    "approved" => format!("Unban approved{by}"),
                    "denied" => format!("Unban denied{by}"),
                    "canceled" => "Withdrawn by the user".into(),
                    _ => return,
                };
                self.resolve_external("unbanRequest", &s(e, "id"), &resolution);
            }
            "channel.moderate" => self.on_moderate(&channel, e),
            "channel.follow" => self.add_activity(crate::db::Activity {
                id: 0,
                channel,
                kind: "follow".into(),
                user_login: s(e, "user_login"),
                display_name: s(e, "user_name"),
                text: "followed".into(),
                timestamp: crate::twitch_api::parse_rfc3339_ms(&s(e, "followed_at")).unwrap_or_else(now_ms),
            }),
            "channel.suspicious_user.message" => {
                let login = s(e, "user_login");
                if self.db.lock().unwrap().open_flag_exists("suspicious", &channel, &login) {
                    return; // one open card per suspicious user is enough
                }
                let evasion = s(e, "ban_evasion_evaluation");
                let status = s(e, "low_trust_status");
                let shared = e["shared_ban_channel_ids"].as_array().map(|a| a.len()).unwrap_or(0);
                let severity = match (evasion.as_str(), status.as_str()) {
                    ("likely", _) => Severity::High,
                    ("possible", _) | (_, "restricted") => Severity::Medium,
                    _ if shared > 0 => Severity::Medium,
                    _ => Severity::Low,
                };
                let mut reasons = vec![];
                match evasion.as_str() {
                    "likely" => reasons.push("Likely ban evader".to_string()),
                    "possible" => reasons.push("Possible ban evader".to_string()),
                    _ => {}
                }
                match status.as_str() {
                    "restricted" => reasons.push("Restricted (only mods see their messages)".to_string()),
                    "active_monitoring" => reasons.push("Being monitored".to_string()),
                    _ => {}
                }
                if shared > 0 {
                    reasons.push(format!("Banned in {shared} channel{} that share ban info", if shared == 1 { "" } else { "s" }));
                }
                let message_id = s(&e["message"], "message_id");
                self.add_flag(Flag {
                    id: 0,
                    channel,
                    message_id: message_id.clone(),
                    user_id: Some(s(e, "user_id")),
                    user_login: login,
                    display_name: s(e, "user_name"),
                    text: s(&e["message"], "text"),
                    severity,
                    matches: vec![RuleMatch {
                        rule_id: 0,
                        rule_name: "Suspicious user".into(),
                        severity,
                        terms: vec![], patterns: vec![], test_mode: false, wellbeing: false,
                        reason: Some(if reasons.is_empty() { "Flagged by Twitch".into() } else { reasons.join(" · ") }),
                    }],
                    context: vec![],
                    context_after: vec![],
                    timestamp: now_ms(),
                    status: "open".into(),
                    resolution: None,
                    kind: "suspicious".into(),
                    external_id: Some(message_id),
                    meta: serde_json::json!({ "lowTrustStatus": status, "banEvasion": evasion, "sharedBans": shared, "types": e["types"] }),
                });
            }
            _ => {}
        }
    }

    /// Every mod action in the channel, by anyone (from EventSub `channel.moderate`).
    fn on_moderate(&self, channel: &str, e: &Value) {
        let action = s(e, "action");
        let moderator = e["moderator_user_name"].as_str().or(e["moderator_user_login"].as_str()).unwrap_or("A mod").to_string();
        let (summary, target) = describe_moderation(&action, e);
        let entry = crate::db::ModLogEntry {
            id: 0,
            channel: channel.to_string(),
            moderator,
            action: action.clone(),
            target_login: target.clone(),
            summary,
            timestamp: now_ms(),
        };
        let saved = {
            let db = self.db.lock().unwrap();
            // Warnings by other mods don't show up in chat, so record them here for cross-channel notes.
            let mod_login = s(e, "moderator_user_login");
            let me = self.helix().map(|h| h.account.login).unwrap_or_default();
            if action == "warn" && mod_login != me {
                if let Some(t) = &target {
                    if let Ok(a) = db.insert_mod_action(&ModAction {
                        id: 0,
                        channel: channel.to_string(),
                        target_login: t.clone(),
                        action: "warn".into(),
                        duration_secs: None,
                        reason: s(&e["warn"], "reason"),
                        ok: true,
                        error: None,
                        timestamp: now_ms(),
                        by_app: false,
                        target_user_id: e["warn"]["user_id"].as_str().map(str::to_string),
                    }) {
                        let _ = self.app.emit("mod-action", &a);
                    }
                }
            }
            db.insert_mod_log(&entry).ok()
        };
        if let Some(saved) = saved {
            let _ = self.app.emit("mod-log", &saved);
        }
    }

    fn resolve_external(&self, kind: &str, external_id: &str, resolution: &str) {
        if let Ok(Some(id)) = self.db.lock().unwrap().resolve_external(kind, external_id, resolution) {
            let _ = self.app.emit("flags-resolved", serde_json::json!({ "ids": [id], "resolution": resolution }));
        }
    }

    fn add_unban_request(&self, req: crate::twitch_api::UnbanRequest) {
        if self.db.lock().unwrap().flag_by_external("unbanRequest", &req.id).is_some() {
            return;
        }
        self.add_flag(Flag {
            id: 0,
            channel: req.channel,
            message_id: String::new(),
            user_id: Some(req.user_id),
            user_login: req.user_login,
            display_name: req.user_name,
            text: req.text,
            severity: Severity::Medium,
            matches: vec![RuleMatch {
                rule_id: 0,
                rule_name: "Unban request".into(),
                severity: Severity::Medium,
                terms: vec![], patterns: vec![], test_mode: false, wellbeing: false,
                reason: None,
            }],
            context: vec![],
            context_after: vec![],
            timestamp: req.created_at,
            status: "open".into(),
            resolution: None,
            kind: "unbanRequest".into(),
            external_id: Some(req.id),
            meta: serde_json::json!({}),
        });
    }

    /// Fetch unban requests that arrived while the app was closed.
    pub(crate) async fn load_unban_requests(&self, channels: &[Channel]) {
        let Ok(helix) = self.helix() else { return };
        for c in channels.iter().filter(|c| c.is_mod) {
            let Some(bid) = &c.broadcaster_id else { continue };
            match helix.pending_unban_requests(bid, &c.name).await {
                Ok(reqs) => reqs.into_iter().for_each(|r| self.add_unban_request(r)),
                Err(e) => eprintln!("couldn't load unban requests for #{}: {e}", c.name),
            }
        }
    }

    fn inbox_flag(&self, flag_id: i64, kind: &str) -> Result<(Flag, String), String> {
        let db = self.db.lock().unwrap();
        let flag = db.flag_by_id(flag_id)?.ok_or("That item is gone.")?;
        if flag.kind != kind {
            return Err("That item can't be handled this way.".into());
        }
        let bid = db.broadcaster_id(&flag.channel).ok_or_else(|| format!("Missing the channel ID for #{}", flag.channel))?;
        Ok((flag, bid))
    }

    fn log_inbox_action(&self, flag: &Flag, action: &str, ok: bool, error: Option<String>, resolution: Option<&str>) {
        let db = self.db.lock().unwrap();
        if let Ok(a) = db.insert_mod_action(&ModAction {
            id: 0,
            channel: flag.channel.clone(),
            target_login: flag.user_login.clone(),
            action: action.into(),
            duration_secs: None,
            reason: String::new(),
            ok,
            error,
            timestamp: now_ms(),
            by_app: true,
            target_user_id: flag.user_id.clone(),
        }) {
            let _ = self.app.emit("mod-action", &a);
        }
        if let (true, Some(r), Some(ext)) = (ok, resolution, flag.external_id.as_deref()) {
            if let Ok(Some(id)) = db.resolve_external(&flag.kind, ext, r) {
                let _ = self.app.emit("flags-resolved", serde_json::json!({ "ids": [id], "resolution": r }));
            }
        }
    }

    pub async fn automod_decide(self: &Arc<Self>, flag_id: i64, allow: bool) -> Result<(), String> {
        let (flag, _) = self.inbox_flag(flag_id, "automod")?;
        let helix = self.helix()?;
        let ext = flag.external_id.clone().unwrap_or_default();
        let r = helix.automod_decide(&ext, allow).await;
        let (action, resolution) = if allow { ("automod_allow", "Approved") } else { ("automod_deny", "Denied") };
        match r {
            Ok(()) => {
                self.log_inbox_action(&flag, action, true, None, Some(resolution));
                Ok(())
            }
            Err(ApiError::Unauthorized) => {
                self.end_session(Some(ApiError::Unauthorized.to_string())).await;
                Err(ApiError::Unauthorized.to_string())
            }
            Err(e) => {
                let msg = e.to_string();
                // Held messages expire after a few minutes; close the card if so.
                if msg.to_lowercase().contains("expired") || msg.contains("404") || msg.to_lowercase().contains("not found") {
                    self.log_inbox_action(&flag, action, true, None, Some("Expired"));
                    return Err("That message already expired or was handled.".into());
                }
                self.log_inbox_action(&flag, action, false, Some(msg.clone()), None);
                Err(msg)
            }
        }
    }

    pub async fn unban_request_decide(self: &Arc<Self>, flag_id: i64, approve: bool, text: &str) -> Result<(), String> {
        let (flag, bid) = self.inbox_flag(flag_id, "unbanRequest")?;
        let helix = self.helix()?;
        let ext = flag.external_id.clone().unwrap_or_default();
        let (action, resolution) =
            if approve { ("unban_request_approve", "Unban approved") } else { ("unban_request_deny", "Unban denied") };
        match helix.resolve_unban_request(&bid, &ext, approve, text).await {
            Ok(()) => {
                self.log_inbox_action(&flag, action, true, None, Some(resolution));
                Ok(())
            }
            Err(ApiError::Unauthorized) => {
                self.end_session(Some(ApiError::Unauthorized.to_string())).await;
                Err(ApiError::Unauthorized.to_string())
            }
            Err(e) => {
                self.log_inbox_action(&flag, action, false, Some(e.to_string()), None);
                Err(e.to_string())
            }
        }
    }
}

pub fn describe_wave(w: &Wave) -> String {
    use crate::waves::WaveKind::*;
    match w.kind {
        SameMessage => "posting the same message".into(),
        SimilarNames => format!("with names like “{}…”", w.key),
        FirstTimeBurst => "chatting for the first time at once".into(),
    }
}

/// A readable summary of a `channel.moderate` event, and the user it targeted (if any).
pub fn describe_moderation(action: &str, e: &Value) -> (String, Option<String>) {
    // The details live under a key named after the action, with a few shared keys.
    let key = match action {
        a if a.ends_with("_blocked_term") || a.ends_with("_permitted_term") => "automod_terms",
        "approve_unban_request" | "deny_unban_request" => "unban_request",
        a => a,
    };
    let d = &e[key];
    let user = d["user_login"].as_str().map(str::to_string);
    let u = user.clone().unwrap_or_else(|| "someone".into());
    let reason = d["reason"].as_str().filter(|r| !r.is_empty()).map(|r| format!(" ({r})")).unwrap_or_default();
    let shared = if action.starts_with("shared_chat_") { "(shared chat) " } else { "" };
    let summary = match action.trim_start_matches("shared_chat_") {
        "ban" => format!("{shared}banned {u}{reason}"),
        "timeout" => {
            let mins = d["expires_at"]
                .as_str()
                .and_then(crate::twitch_api::parse_rfc3339_ms)
                .map(|t| ((t - now_ms()).max(0) + 59_999) / 60_000);
            match mins {
                Some(m) => format!("{shared}timed out {u} for {}{reason}", crate::core::format_duration((m * 60) as u32)),
                None => format!("{shared}timed out {u}{reason}"),
            }
        }
        "unban" => format!("{shared}unbanned {u}"),
        "untimeout" => format!("{shared}removed {u}'s timeout"),
        "delete" => format!("{shared}deleted {u}'s message “{}”", d["message_body"].as_str().unwrap_or_default()),
        "warn" => format!("warned {u}{reason}"),
        "vip" => format!("made {u} a VIP"),
        "unvip" => format!("removed {u} as VIP"),
        "mod" => format!("made {u} a moderator"),
        "unmod" => format!("removed {u} as moderator"),
        "raid" => format!("started a raid to {u}"),
        "unraid" => "cancelled the raid".into(),
        "clear" => "cleared chat".into(),
        "slow" => format!("turned on slow mode ({}s)", d["wait_time_seconds"].as_i64().unwrap_or(0)),
        "slowoff" => "turned off slow mode".into(),
        "followers" => format!("turned on followers-only ({} min)", d["follow_duration_minutes"].as_i64().unwrap_or(0)),
        "followersoff" => "turned off followers-only".into(),
        "subscribers" => "turned on subscribers-only".into(),
        "subscribersoff" => "turned off subscribers-only".into(),
        "emoteonly" => "turned on emote-only".into(),
        "emoteonlyoff" => "turned off emote-only".into(),
        "uniquechat" => "turned on unique chat".into(),
        "uniquechatoff" => "turned off unique chat".into(),
        "approve_unban_request" => format!("approved {u}'s unban request"),
        "deny_unban_request" => format!("denied {u}'s unban request"),
        a if key == "automod_terms" => {
            let terms: Vec<&str> = d["terms"].as_array().map(|t| t.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            format!("{} {}", a.replace('_', " "), terms.join(", "))
        }
        a => a.replace('_', " "),
    };
    (summary, user)
}

#[cfg(test)]
mod tests {
    use super::describe_moderation;
    use serde_json::json;

    #[test]
    fn describes_moderation_events() {
        let (s, t) = describe_moderation("ban", &json!({"ban": {"user_login": "spammer", "reason": "bots"}}));
        assert_eq!((s.as_str(), t.as_deref()), ("banned spammer (bots)", Some("spammer")));
        let (s, _) = describe_moderation("slow", &json!({"slow": {"wait_time_seconds": 30}}));
        assert_eq!(s, "turned on slow mode (30s)");
        let (s, _) = describe_moderation("add_blocked_term", &json!({"automod_terms": {"terms": ["a", "b"]}}));
        assert_eq!(s, "add blocked term a, b");
        let (s, _) = describe_moderation("shared_chat_ban", &json!({"shared_chat_ban": {"user_login": "x"}}));
        assert_eq!(s, "(shared chat) banned x");
        let (s, t) = describe_moderation("delete", &json!({"delete": {"user_login": "x", "message_body": "hi"}}));
        assert_eq!((s.as_str(), t.as_deref()), ("deleted x's message “hi”", Some("x")));
        let (s, _) = describe_moderation("something_new", &json!({}));
        assert_eq!(s, "something new");
    }
}
