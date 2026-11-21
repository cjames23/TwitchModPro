//! Safety checks wired into chat: raid risk scoring, ban-evasion matching, pile-on detection
//! and the on-device toxicity model.

use crate::core::{now_ms, Core};
use crate::db::{ContextLine, Flag, Raid, Settings};
use crate::evasion::{best_match, Banned};
use crate::irc::ChatMessage;
use crate::pileon::{self, PileOn, PileOnConfig};
use crate::raidwatch::{Chatter, RaidRisk, RaidWatch};
use crate::rules::{EvalCtx, RuleMatch, Severity};
use crate::toxicity::{Classifier, Scores};
use crate::waves::{Wave, WaveKind, WaveMember};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Weak};
use tauri::{Emitter, Manager};

/// How far back bans count for ban-evasion matching.
const EVASION_LOOKBACK_MS: i64 = 90 * 86_400_000;
const BANNED_REFRESH_MS: i64 = 60_000;
/// Messages waiting to be scored. When chat is busier than the model, the rest are skipped.
const TOXICITY_QUEUE: usize = 500;
/// A toxicity score this high makes a reply count as hostile for pile-on detection.
const HOSTILE_SCORE: f32 = 0.7;

#[derive(Debug, Clone, Copy)]
pub struct Features {
    pub evasion: bool,
    pub pile_on: Option<PileOnConfig>,
    pub toxicity: bool,
}

impl From<&Settings> for Features {
    fn from(s: &Settings) -> Self {
        Features {
            evasion: s.evasion_check,
            pile_on: s.pile_on_enabled.then(|| PileOnConfig {
                min_users: s.pile_on_min_users.clamp(2, 50) as usize,
                window_ms: s.pile_on_window_secs.clamp(15, 900) * 1000,
            }),
            toxicity: s.toxicity_enabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "state", content = "error")]
pub enum ToxicityStatus {
    Off,
    Loading,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToxicityInfo {
    pub status: ToxicityStatus,
    /// Messages scored since the model was switched on.
    pub scored: u64,
    /// Messages skipped because chat was busier than the model could keep up with.
    pub skipped: u64,
}

pub struct ToxicityState {
    tx: Option<SyncSender<ToxicityJob>>,
    model: Option<Arc<Classifier>>,
    status: ToxicityStatus,
    scored: u64,
    skipped: u64,
    /// Bumped each time the model is switched on, so an old worker can tell it's been replaced.
    generation: u64,
}

impl Default for ToxicityState {
    fn default() -> Self {
        ToxicityState { tx: None, model: None, status: ToxicityStatus::Off, scored: 0, skipped: 0, generation: 0 }
    }
}

pub struct ToxicityJob {
    pub msg: ChatMessage,
    pub context: Vec<ContextLine>,
    /// Toxicity rules that apply to this message.
    pub rules: Vec<i64>,
    /// Whether the score matters for pile-on detection (the message targets someone).
    pub pile_on: bool,
}

fn ago(ms: i64) -> String {
    let mins = ms / 60_000;
    match mins {
        m if m < 60 => "just now".into(),
        m if m < 48 * 60 => format!("{}h ago", m / 60),
        m => format!("{}d ago", m / 1440),
    }
}

impl Core {
    /// Re-read the optional checks from settings (after they're saved).
    pub fn refresh_features(self: &Arc<Self>, settings: &Settings) {
        let features = Features::from(settings);
        *self.features.write().unwrap() = features;
        self.set_toxicity(features.toxicity);
    }

    // ---------------- raid risk ----------------

    pub(crate) fn start_raid_watch(self: &Arc<Self>, raid: &Raid) {
        let prior = self.db.lock().unwrap().prior_raids_from(&raid.from_login, raid.id);
        let watch = RaidWatch::new(raid.id, &raid.channel, &raid.from_login, now_ms(), prior);
        self.raid_watch.lock().unwrap().insert(raid.channel.clone(), watch);

        // Look up the raiding channel's age and followers.
        let core = self.clone();
        let (channel, login, user_id, raid_id) = (raid.channel.clone(), raid.from_login.clone(), raid.from_user_id.clone(), raid.id);
        tauri::async_runtime::spawn(async move {
            let Ok(helix) = core.helix() else { return };
            let user = helix.users(&[login.clone()]).await.ok().and_then(|u| u.into_iter().next());
            let id = user.as_ref().map(|u| u.id.clone()).or(user_id);
            let followers = match &id {
                Some(id) => helix.follower_total(id).await.ok(),
                None => None,
            };
            if let Some(w) = core.raid_watch.lock().unwrap().get_mut(&channel).filter(|w| w.raid_id == raid_id) {
                w.info.created_at = user.map(|u| u.created_at);
                w.info.followers = followers;
            }
        });
    }

    /// Note a chatter who started talking during a raid's watch window.
    pub(crate) fn watch_raid_chat(&self, msg: &ChatMessage, known: bool, flagged: bool) {
        if msg.is_mod || msg.is_broadcaster || msg.is_vip {
            return;
        }
        let mut watches = self.raid_watch.lock().unwrap();
        let Some(w) = watches.get_mut(&msg.channel) else { return };
        if !w.active(msg.timestamp) {
            return;
        }
        match w.chatters.get_mut(&msg.user_login) {
            Some(c) => c.flagged |= flagged,
            None if !known => {
                w.chatters.insert(
                    msg.user_login.clone(),
                    Chatter {
                        login: msg.user_login.clone(),
                        display_name: msg.display_name.clone(),
                        user_id: msg.user_id.clone(),
                        message_id: msg.id.clone(),
                        text: msg.text.clone(),
                        timestamp: msg.timestamp,
                        first_message: msg.is_first_msg,
                        flagged,
                    },
                );
                drop(watches);
                self.age_queue.lock().unwrap().insert(msg.user_login.clone());
            }
            None => {}
        }
    }

    /// Re-score watched raids (called every second). Sends updates to the UI and alerts once a raid looks risky.
    pub fn tick_raids(&self) {
        let now = now_ms();
        let ages = self.age_cache.lock().unwrap().clone();
        let mut updates: Vec<(i64, RaidRisk, bool)> = Vec::new();
        let mut waves: Vec<Wave> = Vec::new();
        {
            let mut watches = self.raid_watch.lock().unwrap();
            for w in watches.values_mut() {
                let mut risk = w.assess(&ages, now);
                let mut alert = false;
                if risk.level == Severity::High {
                    let members: Vec<WaveMember> = w
                        .suspicious(&ages, now)
                        .into_iter()
                        .map(|c| WaveMember {
                            login: c.login.clone(),
                            display_name: c.display_name.clone(),
                            user_id: c.user_id.clone(),
                            channel: w.channel.clone(),
                            message_id: c.message_id.clone(),
                            text: c.text.clone(),
                            timestamp: c.timestamp,
                            first_message: c.first_message,
                            outcome: None,
                        })
                        .collect();
                    let detail = Some(format!("Arrived with {}'s raid. {}", w.raider_login, risk.reasons.first().map(|r| r.text.as_str()).unwrap_or("")));
                    if let Some(wave) =
                        self.waves.lock().unwrap().upsert(WaveKind::RaidRaiders, &w.raider_login, members, Severity::High, detail, now)
                    {
                        w.wave_id = Some(wave.id);
                        waves.push(wave);
                    }
                    risk.wave_id = w.wave_id;
                    if !w.alerted_high {
                        w.alerted_high = true;
                        alert = true;
                    }
                }
                if w.last.as_ref() != Some(&risk) {
                    w.last = Some(risk.clone());
                    updates.push((w.raid_id, risk, alert));
                }
            }
            watches.retain(|_, w| w.active(now));
        }
        for wave in waves {
            self.publish_wave(wave);
        }
        for (raid_id, risk, alert) in updates {
            let (raid, level, sound) = {
                let db = self.db.lock().unwrap();
                let _ = db.set_raid_risk(raid_id, &risk);
                let Ok(Some(raid)) = db.raid_by_id(raid_id) else { continue };
                let (level, sound) = db.channel_alerts(&raid.channel);
                (raid, level, sound && db.settings().play_sounds)
            };
            let alert = alert && level != crate::alerts::ChannelAlertLevel::Off;
            if alert {
                let why = risk.reasons.first().map(|r| r.text.clone()).unwrap_or_default();
                self.notify(&format!("Risky raid on #{}", raid.channel), &format!("{}'s raid looks like a hate or bot raid. {why}.", raid.from_display_name));
            }
            let _ = self.app.emit("raid", serde_json::json!({ "raid": raid, "alert": alert, "sound": alert && sound, "update": true }));
        }
    }

    // ---------------- ban evasion ----------------

    fn banned_list(&self) -> Vec<Banned> {
        let now = now_ms();
        let mut cache = self.banned.lock().unwrap();
        if now - cache.0 > BANNED_REFRESH_MS {
            let rows = self.db.lock().unwrap().current_bans(now - EVASION_LOOKBACK_MS).unwrap_or_default();
            *cache = (now, rows.iter().map(|(c, l, t)| Banned::new(l, c, *t)).collect());
        }
        cache.1.clone()
    }

    /// For a chatter seen for the first time this session: do they look like someone banned recently?
    pub(crate) fn check_evasion(&self, msg: &ChatMessage, f: Features) -> Option<(Banned, &'static str)> {
        if !f.evasion || msg.is_mod || msg.is_broadcaster || msg.is_vip {
            return None;
        }
        let list = self.banned_list();
        let (b, why) = best_match(&msg.user_login, &msg.channel, &list)?;
        if self.db.lock().unwrap().open_flag_exists("evasion", &msg.channel, &msg.user_login) {
            return None;
        }
        Some((b.clone(), why))
    }

    pub(crate) fn add_evasion_flag(&self, msg: &ChatMessage, b: &Banned, why: &str, context: Vec<ContextLine>, created_at: Option<i64>) {
        let same_channel = b.channel == msg.channel;
        let severity = if same_channel { Severity::High } else { Severity::Medium };
        let where_ = if same_channel { "here".to_string() } else { format!("in #{}", b.channel) };
        self.age_queue.lock().unwrap().insert(msg.user_login.clone());
        self.add_flag(Flag {
            id: 0,
            channel: msg.channel.clone(),
            message_id: msg.id.clone(),
            user_id: msg.user_id.clone(),
            user_login: msg.user_login.clone(),
            display_name: msg.display_name.clone(),
            text: msg.text.clone(),
            severity,
            matches: vec![RuleMatch {
                rule_id: 0,
                rule_name: "Possible ban evasion".into(),
                severity,
                terms: vec![],
                patterns: vec![],
                test_mode: false,
                wellbeing: false,
                reason: Some(format!("Name looks like {}, banned {where_} {}: {why}", b.login, ago(now_ms() - b.banned_at))),
            }],
            context,
            context_after: vec![],
            timestamp: msg.timestamp,
            status: "open".into(),
            resolution: None,
            kind: "evasion".into(),
            external_id: None,
            meta: serde_json::json!({
                "evasionOf": b.login,
                "evasionChannel": b.channel,
                "bannedAt": b.banned_at,
                "accountCreatedAt": created_at,
                "firstMessage": msg.is_first_msg,
            }),
        });
    }

    // ---------------- pile-ons ----------------

    pub(crate) fn observe_pileon(&self, msg: &ChatMessage, flagged: bool, f: Features) {
        let Some(cfg) = f.pile_on else { return };
        if pileon::targets(msg).is_empty() {
            return; // not aimed at anyone
        }
        let hostile = flagged || pileon::rude(&msg.text);
        let found = self.pileons.lock().unwrap().observe(msg, hostile, cfg);
        self.report_pileons(found, cfg);
    }

    fn report_pileons(&self, found: Vec<PileOn>, cfg: PileOnConfig) {
        for p in found {
            let who = if p.target == p.channel { "the streamer".to_string() } else { format!("@{}", p.target) };
            let mins = (cfg.window_ms / 60_000).max(1);
            let detail = format!(
                "{} chatters went after {who} within {mins} min; {} of them were hostile.",
                p.senders, p.hostile
            );
            let severity = if p.target == p.channel || p.hostile >= cfg.min_users + 2 { Severity::High } else { Severity::Medium };
            let wave = self.waves.lock().unwrap().upsert(WaveKind::PileOn, &p.target, p.members, severity, Some(detail), now_ms());
            if let Some(w) = wave {
                self.publish_wave(w);
            }
        }
    }

    // ---------------- toxicity model ----------------

    pub fn toxicity_info(&self) -> ToxicityInfo {
        let t = self.toxicity.lock().unwrap();
        ToxicityInfo { status: t.status.clone(), scored: t.scored, skipped: t.skipped }
    }

    fn emit_toxicity(&self) {
        let _ = self.app.emit("toxicity-status", self.toxicity_info());
    }

    fn model_dir(&self) -> PathBuf {
        let bundled = self.app.path().resource_dir().map(|d| d.join("toxicity")).unwrap_or_default();
        if bundled.join("model.safetensors").exists() {
            return bundled;
        }
        // `tauri dev` and tests run from the source tree.
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/toxicity")
    }

    /// The loaded model, if the check is on and ready.
    pub fn toxicity_model(&self) -> Option<Arc<Classifier>> {
        self.toxicity.lock().unwrap().model.clone()
    }

    /// Switch the model on (loading it on a background thread) or off (freeing it).
    pub fn set_toxicity(self: &Arc<Self>, on: bool) {
        let mut t = self.toxicity.lock().unwrap();
        if !on {
            if t.status != ToxicityStatus::Off {
                *t = ToxicityState { generation: t.generation + 1, ..Default::default() };
                drop(t);
                self.emit_toxicity();
            }
            return;
        }
        if t.tx.is_some() {
            return;
        }
        let (tx, rx) = sync_channel::<ToxicityJob>(TOXICITY_QUEUE);
        t.generation += 1;
        let generation = t.generation;
        t.tx = Some(tx);
        t.status = ToxicityStatus::Loading;
        t.scored = 0;
        t.skipped = 0;
        drop(t);
        self.emit_toxicity();

        let dir = self.model_dir();
        let weak: Weak<Core> = Arc::downgrade(self);
        let spawned = std::thread::Builder::new().name("toxicity".into()).spawn(move || {
            let loaded = Classifier::load(&dir).map(Arc::new);
            {
                let Some(core) = weak.upgrade() else { return };
                let mut t = core.toxicity.lock().unwrap();
                if t.generation != generation {
                    return; // switched off while loading
                }
                match &loaded {
                    Ok(m) => {
                        t.model = Some(m.clone());
                        t.status = ToxicityStatus::Ready;
                    }
                    Err(e) => {
                        t.tx = None;
                        t.status = ToxicityStatus::Failed(e.clone());
                    }
                }
                drop(t);
                core.emit_toxicity();
            }
            let Ok(model) = loaded else { return };
            // Runs until the sender is dropped (the check was switched off).
            while let Ok(job) = rx.recv() {
                let Ok(scores) = model.score(&job.msg.text) else { continue };
                let Some(core) = weak.upgrade() else { return };
                core.on_scored(job, scores);
            }
        });
        if let Err(e) = spawned {
            let mut t = self.toxicity.lock().unwrap();
            t.tx = None;
            t.status = ToxicityStatus::Failed(e.to_string());
            drop(t);
            self.emit_toxicity();
        }
    }

    /// A job for the model, if this message should be scored.
    pub(crate) fn toxicity_job(&self, msg: &ChatMessage, f: Features, ctx: EvalCtx) -> Option<ToxicityJob> {
        if !f.toxicity || msg.is_broadcaster || msg.text.trim().is_empty() {
            return None;
        }
        let rules = self.engine.read().unwrap().toxicity_rules(msg, ctx);
        let pile_on = f.pile_on.is_some() && !msg.is_mod && !msg.is_vip && !pileon::targets(msg).is_empty();
        (!rules.is_empty() || pile_on).then(|| ToxicityJob { msg: msg.clone(), context: vec![], rules, pile_on })
    }

    pub(crate) fn queue_toxicity(&self, job: ToxicityJob) {
        let mut t = self.toxicity.lock().unwrap();
        let Some(tx) = &t.tx else { return };
        match tx.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => t.skipped += 1,
            Err(TrySendError::Disconnected(_)) => t.tx = None,
        }
    }

    fn on_scored(&self, job: ToxicityJob, scores: Scores) {
        let report = {
            let mut t = self.toxicity.lock().unwrap();
            t.scored += 1;
            t.scored % 200 == 0
        };
        if report {
            self.emit_toxicity(); // keep the Settings counter roughly current
        }
        let msg = &job.msg;
        let all = self.engine.read().unwrap().toxicity_matches(&job.rules, scores.toxic, scores.severe);
        let (tests, matches): (Vec<RuleMatch>, Vec<RuleMatch>) = all.into_iter().partition(|m| m.test_mode);
        self.record_test_hits(msg, tests);
        if let Some(severity) = matches.iter().map(|m| m.severity).max() {
            let existing = self.db.lock().unwrap().open_flag_for_message(&msg.channel, &msg.id);
            match existing {
                // Already flagged by another rule: add the model's verdict to that card.
                Some(mut flag) => {
                    for m in matches {
                        if !flag.matches.iter().any(|x| x.rule_id == m.rule_id) {
                            flag.matches.push(m);
                        }
                    }
                    flag.severity = flag.severity.max(severity);
                    let _ = self.db.lock().unwrap().update_flag_matches(flag.id, flag.severity, &flag.matches);
                    let _ = self.app.emit("flag", serde_json::json!({ "flag": flag, "alert": false, "update": true }));
                }
                None => {
                    let created_at = self.age_cache.lock().unwrap().get(&msg.user_login).copied();
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
                        context: job.context.clone(),
                        context_after: vec![],
                        timestamp: msg.timestamp,
                        status: "open".into(),
                        resolution: None,
                        kind: "rule".into(),
                        external_id: None,
                        meta: serde_json::json!({ "accountCreatedAt": created_at, "firstMessage": msg.is_first_msg, "toxicity": scores }),
                    });
                }
            }
        }
        if job.pile_on && scores.toxic >= HOSTILE_SCORE {
            if let Some(cfg) = self.features.read().unwrap().pile_on {
                let found = self.pileons.lock().unwrap().mark_hostile(&msg.id, now_ms(), cfg);
                self.report_pileons(found, cfg);
            }
        }
    }
}
