mod alerts;
mod commands;
mod core;
mod db;
mod eventsub;
mod evasion;
mod intel;
mod irc;
mod normalize;
mod packs;
mod pileon;
mod raidwatch;
mod rules;
mod safety;
mod sharing;
mod toxicity;
mod twitch;
mod twitch_api;
mod waves;

use crate::core::{Core, SESSION_CHECK};
use commands::AppState;
use db::Db;
use rules::RuleEngine;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};
use twitch::ChatClient;

const TUNING_RETENTION_MS: i64 = 30 * 86_400_000;

/// Live chat is sent to the UI in batches at this interval.
const CHAT_FLUSH: Duration = Duration::from_millis(150);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // rustls needs a process-wide crypto backend for chat and API connections.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = Db::open(&dir.join("twitch-mod-pro.sqlite3"))?;
            let engine = RuleEngine::new(&db.rules()?);
            let core = Arc::new(Core::new(app.handle().clone(), db, engine));

            // Chat starts with no channels; they're joined once mod status is verified.
            let sink = core.clone();
            let _ = core.client.set(ChatClient::start(vec![], Arc::new(move |e| sink.handle(e))));
            app.manage(AppState { core: core.clone() });
            let settings = core.db.lock().unwrap().settings();
            core.set_toxicity(settings.toxicity_enabled);

            // Restore the saved login, then keep it validated and mod status fresh.
            let session = core.clone();
            tauri::async_runtime::spawn(async move {
                session.restore_session().await;
                let mut tick = tokio::time::interval(SESSION_CHECK);
                loop {
                    tick.tick().await;
                    // Keep 30 days of tuning data (test-mode hits and cleared flags).
                    {
                        let db = session.db.lock().unwrap();
                        let _ = db.prune_tuning_data(crate::core::now_ms() - TUNING_RETENTION_MS);
                        let _ = db.prune_feeds(crate::core::now_ms() - TUNING_RETENTION_MS);
                    }
                    session.check_session().await;
                }
            });

            // Look up account ages in batches (for new-account rules and flag cards),
            // then re-score any raid being watched.
            let ages = core.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tick.tick().await;
                    ages.resolve_ages().await;
                    ages.tick_raids();
                }
            });

            // Flush live chat to the UI in batches so busy channels don't flood it.
            let flush = core;
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(CHAT_FLUSH);
                loop {
                    tick.tick().await;
                    let batch = std::mem::take(&mut *flush.pending_chat.lock().unwrap());
                    if !batch.is_empty() {
                        let _ = flush.app.emit("chat-batch", batch);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_auth,
            commands::set_client_id,
            commands::start_login,
            commands::cancel_login,
            commands::logout,
            commands::list_moderated_channels,
            commands::add_channel,
            commands::remove_channel,
            commands::set_channel_muted,
            commands::set_raid_mode,
            commands::save_rule,
            commands::delete_rule,
            commands::test_rule,
            commands::set_flag_status,
            commands::clear_closed_flags,
            commands::ack_raids,
            commands::save_settings,
            commands::get_user_history,
            commands::delete_message,
            commands::timeout_user,
            commands::ban_user,
            commands::unban_user,
            commands::get_account_ages,
            commands::warn_user,
            commands::ban_everywhere,
            commands::wave_action,
            commands::wave_undo,
            commands::dismiss_wave,
            commands::automod_decide,
            commands::unban_request_decide,
            commands::rule_stats,
            commands::restore_default_packs,
            commands::set_channel_alerts,
            commands::set_channel_color,
            commands::set_shoutout_message,
            commands::shoutout,
            commands::get_mod_log,
            commands::get_activity,
            commands::set_rule_channel_alert,
            commands::set_pack_channel_alert,
            commands::get_toxicity_status,
            commands::score_toxicity,
            commands::export_rules,
            commands::save_rule_file,
            commands::read_rule_file,
            commands::preview_rule_import,
            commands::import_rules,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
