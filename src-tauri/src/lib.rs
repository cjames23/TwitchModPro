mod commands;
mod core;
mod db;
mod eventsub;
mod intel;
mod irc;
mod normalize;
mod rules;
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

/// Live chat is sent to the UI in batches at this interval.
const CHAT_FLUSH: Duration = Duration::from_millis(150);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // rustls needs a process-wide crypto backend for chat and API connections.
    let _ = rustls::crypto::ring::default_provider().install_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
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

            // Restore the saved login, then keep it validated and mod status fresh.
            let session = core.clone();
            tauri::async_runtime::spawn(async move {
                session.restore_session().await;
                let mut tick = tokio::time::interval(SESSION_CHECK);
                tick.tick().await;
                loop {
                    tick.tick().await;
                    session.check_session().await;
                }
            });

            // Look up account ages in batches (for new-account rules and flag cards).
            let ages = core.clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tick.tick().await;
                    ages.resolve_ages().await;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
