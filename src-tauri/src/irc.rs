//! Minimal parser for Twitch's IRC dialect (IRCv3 tags + PRIVMSG/USERNOTICE).

use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct IrcMessage {
    pub tags: HashMap<String, String>,
    pub prefix: Option<String>,
    pub command: String,
    pub params: Vec<String>,
}

impl IrcMessage {
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }

    /// Nickname portion of the prefix (`nick!user@host`).
    pub fn nick(&self) -> Option<&str> {
        self.prefix.as_deref().map(|p| p.split('!').next().unwrap_or(p))
    }

    /// The channel parameter without its leading `#`, lowercased.
    pub fn channel(&self) -> Option<String> {
        self.params
            .first()
            .filter(|p| p.starts_with('#'))
            .map(|p| p.trim_start_matches('#').to_lowercase())
    }
}

fn unescape_tag(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(':') => out.push(';'),
            Some('s') => out.push(' '),
            Some('\\') => out.push('\\'),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Parse a single IRC line (without the trailing CRLF).
pub fn parse_line(line: &str) -> Option<IrcMessage> {
    let mut rest = line.trim_end_matches(['\r', '\n']);
    if rest.is_empty() {
        return None;
    }

    let mut tags = HashMap::new();
    if let Some(stripped) = rest.strip_prefix('@') {
        let (raw_tags, after) = stripped.split_once(' ')?;
        for pair in raw_tags.split(';') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            tags.insert(k.to_string(), unescape_tag(v));
        }
        rest = after.trim_start();
    }

    let mut prefix = None;
    if let Some(stripped) = rest.strip_prefix(':') {
        let (p, after) = stripped.split_once(' ')?;
        prefix = Some(p.to_string());
        rest = after.trim_start();
    }

    let (command, mut rest) = match rest.split_once(' ') {
        Some((c, r)) => (c.to_string(), r),
        None => (rest.to_string(), ""),
    };

    let mut params = Vec::new();
    while !rest.is_empty() {
        if let Some(trailing) = rest.strip_prefix(':') {
            params.push(trailing.to_string());
            break;
        }
        match rest.split_once(' ') {
            Some((p, r)) => {
                if !p.is_empty() {
                    params.push(p.to_string());
                }
                rest = r;
            }
            None => {
                params.push(rest.to_string());
                break;
            }
        }
    }

    Some(IrcMessage { tags, prefix, command, params })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub channel: String,
    /// Twitch ID of the channel (`room-id` tag).
    pub room_id: Option<String>,
    /// Twitch ID of the sender (`user-id` tag).
    pub user_id: Option<String>,
    pub user_login: String,
    pub display_name: String,
    pub color: Option<String>,
    pub text: String,
    pub is_action: bool,
    pub is_mod: bool,
    pub is_broadcaster: bool,
    pub is_vip: bool,
    pub is_subscriber: bool,
    pub is_first_msg: bool,
    pub emote_count: usize,
    /// Login of the user this message replies to, if it's a reply.
    #[serde(default)]
    pub reply_to: Option<String>,
    pub timestamp: i64,
}

/// Count emote occurrences in a `emotes` tag like `25:0-4,12-16/1902:6-10`.
fn count_emotes(tag: Option<&str>) -> usize {
    tag.map(|t| {
        t.split('/')
            .filter_map(|e| e.split_once(':'))
            .map(|(_, ranges)| ranges.split(',').filter(|r| !r.is_empty()).count())
            .sum()
    })
    .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RaidNotice {
    pub channel: String,
    pub from_user_id: Option<String>,
    pub from_login: String,
    pub from_display_name: String,
    pub viewers: i64,
    pub timestamp: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn sent_ts(msg: &IrcMessage) -> i64 {
    msg.tag("tmi-sent-ts").and_then(|t| t.parse().ok()).unwrap_or_else(now_ms)
}

impl ChatMessage {
    pub fn from_irc(msg: &IrcMessage) -> Option<Self> {
        if msg.command != "PRIVMSG" {
            return None;
        }
        let channel = msg.channel()?;
        let raw = msg.params.get(1)?.clone();
        let (text, is_action) = match raw
            .strip_prefix("\u{1}ACTION ")
            .map(|t| t.trim_end_matches('\u{1}'))
        {
            Some(t) => (t.to_string(), true),
            None => (raw, false),
        };
        let user_login = msg.nick().unwrap_or_default().to_lowercase();
        let badges = msg.tag("badges").unwrap_or_default();
        let has_badge = |b: &str| badges.split(',').any(|x| x.split('/').next() == Some(b));

        Some(ChatMessage {
            id: msg.tag("id").map(str::to_string).unwrap_or_else(|| format!("local-{}", now_ms())),
            display_name: msg.tag("display-name").unwrap_or(&user_login).to_string(),
            color: msg.tag("color").map(str::to_string),
            is_mod: msg.tag("mod") == Some("1") || has_badge("moderator"),
            is_broadcaster: has_badge("broadcaster"),
            is_vip: msg.tag("vip").is_some() || has_badge("vip"),
            is_subscriber: msg.tag("subscriber") == Some("1") || has_badge("subscriber"),
            is_first_msg: msg.tag("first-msg") == Some("1"),
            emote_count: count_emotes(msg.tag("emotes")),
            reply_to: msg.tag("reply-parent-user-login").map(|l| l.to_lowercase()),
            room_id: msg.tag("room-id").map(str::to_string),
            user_id: msg.tag("user-id").map(str::to_string),
            timestamp: sent_ts(msg),
            channel,
            user_login,
            text,
            is_action,
        })
    }
}

impl RaidNotice {
    pub fn from_irc(msg: &IrcMessage) -> Option<Self> {
        if msg.command != "USERNOTICE" || msg.tag("msg-id") != Some("raid") {
            return None;
        }
        let from_login = msg
            .tag("msg-param-login")
            .or_else(|| msg.tag("login"))?
            .to_lowercase();
        Some(RaidNotice {
            channel: msg.channel()?,
            from_display_name: msg
                .tag("msg-param-displayName")
                .or_else(|| msg.tag("display-name"))
                .unwrap_or(&from_login)
                .to_string(),
            viewers: msg.tag("msg-param-viewerCount").and_then(|v| v.parse().ok()).unwrap_or(0),
            from_user_id: msg.tag("user-id").map(str::to_string),
            timestamp: sent_ts(msg),
            from_login,
        })
    }
}

/// Something for the activity feed: subs, resubs, gifts, bits, announcements.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityNotice {
    pub channel: String,
    pub kind: String,
    pub user_login: String,
    pub display_name: String,
    pub text: String,
    pub timestamp: i64,
}

impl ActivityNotice {
    pub fn from_irc(msg: &IrcMessage) -> Option<Self> {
        let channel = msg.channel()?;
        let login = msg.tag("login").or_else(|| msg.nick()).unwrap_or_default().to_lowercase();
        let display = msg.tag("display-name").unwrap_or(&login).to_string();
        let user_text = msg.params.get(1).cloned().unwrap_or_default();
        let joined = |a: &str, b: &str| match (a.is_empty(), b.is_empty()) {
            (false, false) => format!("{a} “{b}”"),
            (false, true) => a.to_string(),
            _ => b.to_string(),
        };

        if msg.command == "PRIVMSG" {
            let bits: u32 = msg.tag("bits")?.parse().ok()?;
            let text = format!("cheered {bits} bit{}", if bits == 1 { "" } else { "s" });
            return Some(ActivityNotice {
                channel,
                kind: "bits".into(),
                user_login: login,
                display_name: display,
                text: joined(&text, &user_text),
                timestamp: sent_ts(msg),
            });
        }
        if msg.command != "USERNOTICE" {
            return None;
        }
        let system = msg.tag("system-msg").unwrap_or_default().trim().to_string();
        let kind = match msg.tag("msg-id")? {
            "sub" => "sub",
            "resub" => "resub",
            // Individual gifts from a gift bomb arrive separately; the bomb itself is enough.
            "subgift" if msg.tag("msg-param-community-gift-id").is_some() => return None,
            "subgift" => "subgift",
            "submysterygift" => "giftbomb",
            "giftpaidupgrade" | "anongiftpaidupgrade" | "primepaidupgrade" => "upgrade",
            "announcement" => "announcement",
            // Raids have their own alerts; shared-chat copies would duplicate other channels.
            "raid" | "unraid" | "sharedchatnotice" => return None,
            _ => "other",
        };
        let text = if kind == "announcement" { format!("Announcement: {user_text}") } else { joined(&system, &user_text) };
        if text.trim().is_empty() {
            return None;
        }
        Some(ActivityNotice { channel, kind: kind.into(), user_login: login, display_name: display, text, timestamp: sent_ts(msg) })
    }
}

/// A timeout, ban or chat clear (`CLEARCHAT`), or a single deleted message (`CLEARMSG`),
/// done by any moderator in a channel we're watching.
#[derive(Debug, Clone, PartialEq)]
pub enum ModEvent {
    /// `duration_secs` is None for a permanent ban.
    UserCleared { channel: String, login: String, user_id: Option<String>, duration_secs: Option<u32>, timestamp: i64 },
    MessageDeleted { channel: String, login: String, message_id: String },
}

impl ModEvent {
    pub fn from_irc(msg: &IrcMessage) -> Option<Self> {
        match msg.command.as_str() {
            "CLEARCHAT" => {
                // Without a target user, the whole chat was cleared; nothing to record.
                let login = msg.params.get(1)?.to_lowercase();
                Some(ModEvent::UserCleared {
                    channel: msg.channel()?,
                    login,
                    user_id: msg.tag("target-user-id").map(str::to_string),
                    duration_secs: msg.tag("ban-duration").and_then(|d| d.parse().ok()),
                    timestamp: sent_ts(msg),
                })
            }
            "CLEARMSG" => Some(ModEvent::MessageDeleted {
                channel: msg.channel()?,
                login: msg.tag("login")?.to_lowercase(),
                message_id: msg.tag("target-msg-id")?.to_string(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_privmsg_with_tags() {
        let line = "@badge-info=;badges=moderator/1,subscriber/12;color=#FF4500;display-name=SomeMod;emotes=25:0-4,12-16/1902:6-10;first-msg=0;id=abc-123;mod=1;reply-parent-user-login=SomeOne;room-id=111;subscriber=1;tmi-sent-ts=1700000000000;user-id=222 :somemod!somemod@somemod.tmi.twitch.tv PRIVMSG #CoolStreamer :hello there chat";
        let msg = parse_line(line).unwrap();
        assert_eq!(msg.command, "PRIVMSG");
        let chat = ChatMessage::from_irc(&msg).unwrap();
        assert_eq!(chat.channel, "coolstreamer");
        assert_eq!(chat.user_login, "somemod");
        assert_eq!(chat.display_name, "SomeMod");
        assert_eq!(chat.text, "hello there chat");
        assert!(chat.is_mod && chat.is_subscriber && !chat.is_vip && !chat.is_broadcaster);
        assert_eq!(chat.timestamp, 1700000000000);
        assert_eq!(chat.id, "abc-123");
        assert_eq!(chat.emote_count, 3);
        assert_eq!(chat.reply_to.as_deref(), Some("someone"));
        assert_eq!(chat.room_id.as_deref(), Some("111"));
        assert_eq!(chat.user_id.as_deref(), Some("222"));
    }

    #[test]
    fn parses_action_messages() {
        let line = ":bob!bob@bob.tmi.twitch.tv PRIVMSG #chan :\u{1}ACTION waves\u{1}";
        let chat = ChatMessage::from_irc(&parse_line(line).unwrap()).unwrap();
        assert!(chat.is_action);
        assert_eq!(chat.text, "waves");
    }

    #[test]
    fn parses_raid_usernotice() {
        let line = "@badges=;display-name=BigRaider;login=bigraider;user-id=555;msg-id=raid;msg-param-displayName=BigRaider;msg-param-login=bigraider;msg-param-viewerCount=1337;system-msg=1337\\sraiders\\sfrom\\sBigRaider\\shave\\sjoined!;tmi-sent-ts=1700000001000 :tmi.twitch.tv USERNOTICE #targetchannel";
        let msg = parse_line(line).unwrap();
        assert_eq!(msg.tag("system-msg"), Some("1337 raiders from BigRaider have joined!"));
        let raid = RaidNotice::from_irc(&msg).unwrap();
        assert_eq!(raid.channel, "targetchannel");
        assert_eq!(raid.from_login, "bigraider");
        assert_eq!(raid.from_display_name, "BigRaider");
        assert_eq!(raid.viewers, 1337);
        assert_eq!(raid.from_user_id.as_deref(), Some("555"));
    }

    #[test]
    fn non_raid_usernotice_is_ignored() {
        let line = "@msg-id=sub;login=x :tmi.twitch.tv USERNOTICE #chan :yay";
        assert!(RaidNotice::from_irc(&parse_line(line).unwrap()).is_none());
    }

    #[test]
    fn parses_activity_notices() {
        let resub = r"@display-name=Fan;login=fan;msg-id=resub;system-msg=Fan\ssubscribed\sat\sTier\s1.\sThey've\ssubscribed\sfor\s5\smonths!;tmi-sent-ts=1 :tmi.twitch.tv USERNOTICE #chan :love the streams";
        let a = ActivityNotice::from_irc(&parse_line(resub).unwrap()).unwrap();
        assert_eq!(a.kind, "resub");
        assert_eq!(a.text, "Fan subscribed at Tier 1. They've subscribed for 5 months! “love the streams”");

        let bomb_part = r"@login=gifter;msg-id=subgift;msg-param-community-gift-id=123;system-msg=x :tmi.twitch.tv USERNOTICE #chan";
        assert!(ActivityNotice::from_irc(&parse_line(bomb_part).unwrap()).is_none());
        let bomb = r"@login=gifter;msg-id=submysterygift;system-msg=gifter\sis\sgifting\s5\sTier\s1\sSubs! :tmi.twitch.tv USERNOTICE #chan";
        assert_eq!(ActivityNotice::from_irc(&parse_line(bomb).unwrap()).unwrap().kind, "giftbomb");

        let bits = "@bits=100;display-name=Cheery;tmi-sent-ts=1 :cheery!cheery@cheery.tmi.twitch.tv PRIVMSG #chan :Cheer100 gg";
        let b = ActivityNotice::from_irc(&parse_line(bits).unwrap()).unwrap();
        assert_eq!((b.kind.as_str(), b.text.as_str()), ("bits", "cheered 100 bits “Cheer100 gg”"));
        let plain = ":x!x@x.tmi.twitch.tv PRIVMSG #chan :hello";
        assert!(ActivityNotice::from_irc(&parse_line(plain).unwrap()).is_none());
        let raid = "@msg-id=raid;login=r;msg-param-viewerCount=5 :tmi.twitch.tv USERNOTICE #chan";
        assert!(ActivityNotice::from_irc(&parse_line(raid).unwrap()).is_none());
    }

    #[test]
    fn parses_clearchat_and_clearmsg() {
        let t = "@ban-duration=600;room-id=1;target-user-id=42;tmi-sent-ts=1700000000000 :tmi.twitch.tv CLEARCHAT #chan :SpamGuy";
        assert_eq!(
            ModEvent::from_irc(&parse_line(t).unwrap()),
            Some(ModEvent::UserCleared {
                channel: "chan".into(),
                login: "spamguy".into(),
                user_id: Some("42".into()),
                duration_secs: Some(600),
                timestamp: 1700000000000
            })
        );
        let b = "@room-id=1;target-user-id=42 :tmi.twitch.tv CLEARCHAT #chan :spamguy";
        assert!(matches!(ModEvent::from_irc(&parse_line(b).unwrap()), Some(ModEvent::UserCleared { duration_secs: None, .. })));
        assert_eq!(ModEvent::from_irc(&parse_line(":tmi.twitch.tv CLEARCHAT #chan").unwrap()), None);
        let m = "@login=spamguy;room-id=;target-msg-id=abc :tmi.twitch.tv CLEARMSG #chan :bad words";
        assert_eq!(
            ModEvent::from_irc(&parse_line(m).unwrap()),
            Some(ModEvent::MessageDeleted { channel: "chan".into(), login: "spamguy".into(), message_id: "abc".into() })
        );
    }

    #[test]
    fn parses_ping_and_unescapes() {
        let msg = parse_line("PING :tmi.twitch.tv").unwrap();
        assert_eq!(msg.command, "PING");
        assert_eq!(msg.params, vec!["tmi.twitch.tv"]);
        assert_eq!(unescape_tag(r"a\:b\sc\\d"), r"a;b c\d");
    }
}
