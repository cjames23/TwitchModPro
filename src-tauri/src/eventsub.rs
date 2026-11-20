//! EventSub over WebSocket: AutoMod holds, unban requests and suspicious-user messages
//! for every channel the user moderates and monitors.

use crate::twitch_api::{ApiError, Helix};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const EVENTSUB_WS: &str = "wss://eventsub.wss.twitch.tv/ws";

/// (type, version) pairs subscribed per channel.
pub const SUBSCRIPTIONS: &[(&str, &str)] = &[
    ("automod.message.hold", "2"),
    ("automod.message.update", "2"),
    ("channel.unban_request.create", "1"),
    ("channel.unban_request.resolve", "1"),
    ("channel.suspicious_user.message", "1"),
];

#[derive(Debug, Clone)]
pub enum SubEvent {
    /// A notification: subscription type and its `event` payload.
    Notification { kind: String, event: serde_json::Value },
    /// A subscription couldn't be created, e.g. missing permission.
    SubscribeFailed { broadcaster_id: String, kinds: Vec<String>, error: String },
    Connected(bool),
}

pub type SubSink = Arc<dyn Fn(SubEvent) + Send + Sync>;

enum Command {
    Add(String),
    Remove(String),
}

/// Handle to the background EventSub connection. Dropping it doesn't stop it; call `stop`.
pub struct EventSubClient {
    tx: mpsc::UnboundedSender<Command>,
    task: tauri::async_runtime::JoinHandle<()>,
    current: Mutex<BTreeSet<String>>,
}

impl EventSubClient {
    pub fn start(helix: Helix, broadcaster_ids: BTreeSet<String>, sink: SubSink) -> Self {
        Self::start_at(EVENTSUB_WS, helix, broadcaster_ids, sink)
    }

    fn start_at(url: &str, helix: Helix, broadcaster_ids: BTreeSet<String>, sink: SubSink) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tauri::async_runtime::spawn(run(url.to_string(), helix, broadcaster_ids.clone(), rx, sink));
        EventSubClient { tx, task, current: Mutex::new(broadcaster_ids) }
    }

    /// Subscribe/unsubscribe so exactly these channels are covered.
    pub fn set_channels(&self, ids: BTreeSet<String>) {
        let mut cur = self.current.lock().unwrap();
        for added in ids.difference(&cur) {
            let _ = self.tx.send(Command::Add(added.clone()));
        }
        for removed in cur.difference(&ids) {
            let _ = self.tx.send(Command::Remove(removed.clone()));
        }
        *cur = ids;
    }

    pub fn stop(self) {
        self.task.abort();
    }
}

#[derive(Deserialize)]
struct Frame {
    metadata: Metadata,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Deserialize)]
struct Metadata {
    message_type: String,
    #[serde(default)]
    subscription_type: Option<String>,
}

type SubIds = Arc<Mutex<BTreeMap<String, Vec<String>>>>;

async fn subscribe_channel(helix: &Helix, session: &str, broadcaster_id: &str, subs: &SubIds, sink: &SubSink) -> Result<(), ApiError> {
    let mut ids = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut last_error = String::new();
    for (kind, version) in SUBSCRIPTIONS {
        match helix.eventsub_subscribe(session, kind, version, broadcaster_id).await {
            Ok(id) => ids.push(id),
            Err(ApiError::Unauthorized) => return Err(ApiError::Unauthorized),
            Err(e) => {
                let label = match *kind {
                    k if k.starts_with("automod") => "the AutoMod inbox",
                    k if k.contains("unban_request") => "unban requests",
                    _ => "suspicious-user alerts",
                };
                if !failed.iter().any(|f| f == label) {
                    failed.push(label.to_string());
                }
                last_error = e.to_string();
            }
        }
    }
    if !failed.is_empty() {
        sink(SubEvent::SubscribeFailed { broadcaster_id: broadcaster_id.to_string(), kinds: failed, error: last_error });
    }
    subs.lock().unwrap().insert(broadcaster_id.to_string(), ids);
    Ok(())
}

async fn run(
    base_url: String,
    helix: Helix,
    mut wanted: BTreeSet<String>,
    mut rx: mpsc::UnboundedReceiver<Command>,
    sink: SubSink,
) {
    let subs: SubIds = Arc::new(Mutex::new(BTreeMap::new()));
    let mut url = base_url.clone();
    let mut reconnecting = false;
    let mut backoff = Duration::from_secs(1);

    loop {
        let ws = match connect_async(url.as_str()).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                eprintln!("eventsub connect failed: {e}");
                sink(SubEvent::Connected(false));
                url = base_url.clone();
                reconnecting = false;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        let (mut write, mut read) = ws.split();
        let mut keepalive = Duration::from_secs(30);
        let mut session_id: Option<String> = None;
        let mut next_url: Option<String> = None;

        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    let Some(cmd) = cmd else { return };
                    match cmd {
                        Command::Add(id) => {
                            if wanted.insert(id.clone()) {
                                if let Some(s) = &session_id {
                                    if let Err(ApiError::Unauthorized) = subscribe_channel(&helix, s, &id, &subs, &sink).await {
                                        return;
                                    }
                                }
                            }
                        }
                        Command::Remove(id) => {
                            wanted.remove(&id);
                            let ids = subs.lock().unwrap().remove(&id).unwrap_or_default();
                            for sub in ids {
                                let _ = helix.eventsub_unsubscribe(&sub).await;
                            }
                        }
                    }
                }
                frame = tokio::time::timeout(keepalive + Duration::from_secs(10), read.next()) => {
                    let text = match frame {
                        Ok(Some(Ok(Message::Text(t)))) => t.to_string(),
                        Ok(Some(Ok(Message::Ping(p)))) => {
                            let _ = write.send(Message::Pong(p)).await;
                            continue;
                        }
                        Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) | Ok(Some(Err(_))) => break,
                        Ok(Some(Ok(_))) => continue,
                    };
                    let Ok(frame) = serde_json::from_str::<Frame>(&text) else { continue };
                    match frame.metadata.message_type.as_str() {
                        "session_welcome" => {
                            let session = &frame.payload["session"];
                            session_id = session["id"].as_str().map(str::to_string);
                            if let Some(k) = session["keepalive_timeout_seconds"].as_u64() {
                                keepalive = Duration::from_secs(k.max(10));
                            }
                            backoff = Duration::from_secs(1);
                            sink(SubEvent::Connected(true));
                            // After a reconnect message, existing subscriptions carry over.
                            if !reconnecting {
                                subs.lock().unwrap().clear();
                                if let Some(s) = session_id.clone() {
                                    for id in wanted.clone() {
                                        if let Err(ApiError::Unauthorized) = subscribe_channel(&helix, &s, &id, &subs, &sink).await {
                                            return;
                                        }
                                    }
                                }
                            }
                            reconnecting = false;
                        }
                        "session_keepalive" => {}
                        "session_reconnect" => {
                            next_url = frame.payload["session"]["reconnect_url"].as_str().map(str::to_string);
                            break;
                        }
                        "notification" => {
                            if let Some(kind) = frame.metadata.subscription_type {
                                sink(SubEvent::Notification { kind, event: frame.payload["event"].clone() });
                            }
                        }
                        "revocation" => {
                            eprintln!("eventsub subscription revoked: {}", frame.payload["subscription"]);
                        }
                        _ => {}
                    }
                }
            }
        }

        sink(SubEvent::Connected(false));
        match next_url {
            Some(u) => {
                url = u;
                reconnecting = true;
            }
            None => {
                url = base_url.clone();
                reconnecting = false;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twitch_api::{http_client, Account, Endpoints, Tokens};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test(flavor = "multi_thread")]
    async fn subscribes_after_welcome_answers_pings_and_forwards_notifications() {
        let helix_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/eventsub/subscriptions"))
            .and(body_string_contains("\"session_id\":\"sess1\""))
            .and(body_string_contains("\"broadcaster_user_id\":\"42\""))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({"data": [{"id": "sub"}]})))
            .expect(SUBSCRIPTIONS.len() as u64)
            .mount(&helix_server)
            .await;
        let helix = Helix::new(
            http_client(),
            Endpoints { id: helix_server.uri(), helix: format!("{}/helix", helix_server.uri()) },
            "cid".into(),
            Account { user_id: "999".into(), login: "me".into(), scopes: vec![] },
            Tokens { access_token: "t".into(), refresh_token: None },
            Arc::new(|_| {}),
        );

        // A fake EventSub server.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_url = format!("ws://{}", listener.local_addr().unwrap());
        let (pong_tx, pong_rx) = std::sync::mpsc::channel();
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let welcome = serde_json::json!({
                "metadata": {"message_type": "session_welcome"},
                "payload": {"session": {"id": "sess1", "keepalive_timeout_seconds": 10}}
            });
            ws.send(Message::Text(welcome.to_string().into())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            ws.send(Message::Ping(b"hi".to_vec().into())).await.unwrap();
            let note = serde_json::json!({
                "metadata": {"message_type": "notification", "subscription_type": "automod.message.hold"},
                "payload": {"event": {"message_id": "m1", "broadcaster_user_login": "chan"}}
            });
            ws.send(Message::Text(note.to_string().into())).await.unwrap();
            while let Some(Ok(m)) = ws.next().await {
                if let Message::Pong(p) = m {
                    let _ = pong_tx.send(p.to_vec());
                }
            }
        });

        let (tx, rx) = std::sync::mpsc::channel();
        let client = EventSubClient::start_at(
            &ws_url,
            helix,
            ["42".to_string()].into_iter().collect(),
            Arc::new(move |e| {
                let _ = tx.send(e);
            }),
        );
        let mut connected = false;
        let mut note = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline && note.is_none() {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(SubEvent::Connected(true)) => connected = true,
                Ok(SubEvent::Notification { kind, event }) => note = Some((kind, event)),
                Ok(SubEvent::SubscribeFailed { error, .. }) => panic!("subscribe failed: {error}"),
                _ => {}
            }
        }
        assert!(connected);
        let (kind, event) = note.expect("no notification forwarded");
        assert_eq!(kind, "automod.message.hold");
        assert_eq!(event["message_id"], "m1");
        assert_eq!(pong_rx.recv_timeout(Duration::from_secs(5)).unwrap(), b"hi".to_vec());
        client.stop();
        // `expect(5)` on the mock checks every subscription type was created for the session.
        helix_server.verify().await;
    }
}
