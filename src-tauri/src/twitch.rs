//! Read-only connection to Twitch chat over IRC-on-WebSocket.
//!
//! Connects anonymously (no login needed to read public chats), joins any number of
//! channels, reconnects with backoff, and respects Twitch's JOIN rate limit.

use crate::irc::{parse_line, ActivityNotice, ChatMessage, ModEvent, RaidNotice};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const TWITCH_WS: &str = "wss://irc-ws.chat.twitch.tv:443";
/// Twitch allows 20 JOINs per 10 seconds for unverified accounts; stay under it.
const JOIN_LIMIT: usize = 18;
const JOIN_WINDOW: Duration = Duration::from_secs(10);
/// Twitch PINGs about every 5 minutes; if we hear nothing for this long, reconnect.
const READ_TIMEOUT: Duration = Duration::from_secs(360);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Status(ConnStatus),
    Joined(String),
    Parted(String),
    Chat(ChatMessage),
    Raid(RaidNotice),
    /// A timeout, ban or message deletion by any moderator.
    Mod(ModEvent),
    /// Subs, gifts, bits, announcements.
    Activity(ActivityNotice),
    /// Server notice, e.g. a channel that doesn't exist or is suspended.
    Notice { channel: Option<String>, text: String },
}

pub type EventSink = Arc<dyn Fn(ChatEvent) + Send + Sync>;

enum Command {
    Join(String),
    Part(String),
}

#[derive(Clone)]
pub struct ChatClient {
    tx: mpsc::UnboundedSender<Command>,
    wanted: Arc<Mutex<BTreeSet<String>>>,
}

impl ChatClient {
    /// Start the background connection task.
    pub fn start(initial: Vec<String>, sink: EventSink) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let wanted = Arc::new(Mutex::new(initial.into_iter().collect::<BTreeSet<_>>()));
        tauri::async_runtime::spawn(run(rx, wanted.clone(), sink));
        ChatClient { tx, wanted }
    }

    pub fn join(&self, channel: &str) {
        if self.wanted.lock().unwrap().insert(channel.to_string()) {
            let _ = self.tx.send(Command::Join(channel.to_string()));
        }
    }

    pub fn part(&self, channel: &str) {
        if self.wanted.lock().unwrap().remove(channel) {
            let _ = self.tx.send(Command::Part(channel.to_string()));
        }
    }
}

fn anon_nick() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("justinfan{}", 10_000 + n % 80_000)
}

async fn run(mut rx: mpsc::UnboundedReceiver<Command>, wanted: Arc<Mutex<BTreeSet<String>>>, sink: EventSink) {
    let mut backoff = Duration::from_secs(1);
    loop {
        sink(ChatEvent::Status(ConnStatus::Connecting));
        let started = Instant::now();
        match session(&mut rx, &wanted, &sink).await {
            Ok(()) => eprintln!("twitch chat connection closed; reconnecting"),
            Err(e) => eprintln!("twitch chat connection error: {e}"),
        }
        sink(ChatEvent::Status(ConnStatus::Disconnected));
        // A connection that stayed up a while resets the backoff.
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// Sends queued lines to the socket, spacing out JOINs to stay under the rate limit.
async fn writer<S>(mut sink: S, mut lines: mpsc::UnboundedReceiver<String>)
where
    S: futures_util::Sink<Message> + Unpin,
{
    let mut recent_joins: VecDeque<Instant> = VecDeque::new();
    while let Some(line) = lines.recv().await {
        if line.starts_with("JOIN ") {
            while recent_joins.len() >= JOIN_LIMIT {
                let oldest = recent_joins[0];
                let elapsed = oldest.elapsed();
                if elapsed >= JOIN_WINDOW {
                    recent_joins.pop_front();
                } else {
                    tokio::time::sleep(JOIN_WINDOW - elapsed).await;
                }
            }
            recent_joins.push_back(Instant::now());
        }
        if sink.send(Message::Text(line.into())).await.is_err() {
            break;
        }
    }
}

async fn session(
    rx: &mut mpsc::UnboundedReceiver<Command>,
    wanted: &Arc<Mutex<BTreeSet<String>>>,
    sink: &EventSink,
) -> Result<(), String> {
    let (ws, _) = connect_async(TWITCH_WS).await.map_err(|e| e.to_string())?;
    let (ws_tx, mut ws_rx) = ws.split();
    let (out, out_rx) = mpsc::unbounded_channel::<String>();
    let writer_task = tauri::async_runtime::spawn(writer(ws_tx, out_rx));

    let send = |line: String| out.send(line).map_err(|_| "writer closed".to_string());
    send("CAP REQ :twitch.tv/tags twitch.tv/commands".into())?;
    send("PASS SCHMOOPIIE".into())?;
    send(format!("NICK {}", anon_nick()))?;
    // Drop commands queued while we were offline; `wanted` already reflects them.
    while rx.try_recv().is_ok() {}
    let initial: Vec<String> = wanted.lock().unwrap().iter().cloned().collect();
    for ch in initial {
        send(format!("JOIN #{ch}"))?;
    }

    let result = loop {
        tokio::select! {
            cmd = rx.recv() => match cmd {
                Some(Command::Join(ch)) => send(format!("JOIN #{ch}"))?,
                Some(Command::Part(ch)) => {
                    send(format!("PART #{ch}"))?;
                    sink(ChatEvent::Parted(ch));
                }
                None => break Ok(()),
            },
            frame = tokio::time::timeout(READ_TIMEOUT, ws_rx.next()) => {
                let frame = match frame {
                    Err(_) => break Err("no data from Twitch for 6 minutes".into()),
                    Ok(None) => break Ok(()),
                    Ok(Some(Err(e))) => break Err(e.to_string()),
                    Ok(Some(Ok(f))) => f,
                };
                let text = match frame {
                    Message::Text(t) => t.to_string(),
                    Message::Close(_) => break Ok(()),
                    _ => continue,
                };
                let mut reconnect = false;
                for line in text.split("\r\n").filter(|l| !l.is_empty()) {
                    let Some(msg) = parse_line(line) else { continue };
                    match msg.command.as_str() {
                        "PING" => send(format!("PONG :{}", msg.params.first().map(String::as_str).unwrap_or("tmi.twitch.tv")))?,
                        "001" => sink(ChatEvent::Status(ConnStatus::Connected)),
                        "RECONNECT" => reconnect = true,
                        "PRIVMSG" => {
                            if let Some(a) = ActivityNotice::from_irc(&msg) {
                                sink(ChatEvent::Activity(a));
                            }
                            if let Some(chat) = ChatMessage::from_irc(&msg) {
                                sink(ChatEvent::Chat(chat));
                            }
                        }
                        "USERNOTICE" => {
                            if let Some(raid) = RaidNotice::from_irc(&msg) {
                                sink(ChatEvent::Raid(raid));
                            } else if let Some(a) = ActivityNotice::from_irc(&msg) {
                                sink(ChatEvent::Activity(a));
                            }
                        }
                        "CLEARCHAT" | "CLEARMSG" => {
                            if let Some(ev) = ModEvent::from_irc(&msg) {
                                sink(ChatEvent::Mod(ev));
                            }
                        }
                        "JOIN" => {
                            if let Some(ch) = msg.channel() {
                                if msg.nick().is_some_and(|n| n.starts_with("justinfan")) {
                                    sink(ChatEvent::Joined(ch));
                                }
                            }
                        }
                        "NOTICE" => sink(ChatEvent::Notice {
                            channel: msg.channel(),
                            text: msg.params.last().cloned().unwrap_or_default(),
                        }),
                        _ => {}
                    }
                }
                if reconnect {
                    break Ok(());
                }
            }
        }
    };
    writer_task.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live check against Twitch: `cargo test -- --ignored live_connects`
    #[test]
    #[ignore]
    fn live_connects_and_joins() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (tx, rx) = std::sync::mpsc::channel();
        let _client = ChatClient::start(
            vec!["twitch".into()],
            Arc::new(move |e| {
                let _ = tx.send(e);
            }),
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        let (mut connected, mut joined) = (false, false);
        while Instant::now() < deadline && !(connected && joined) {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(ChatEvent::Status(ConnStatus::Connected)) => connected = true,
                Ok(ChatEvent::Joined(ch)) if ch == "twitch" => joined = true,
                _ => {}
            }
        }
        assert!(connected, "never got the 001 welcome");
        assert!(joined, "never joined #twitch");
    }
}
