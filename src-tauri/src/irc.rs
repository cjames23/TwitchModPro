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
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RaidNotice {
    pub channel: String,
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
            timestamp: sent_ts(msg),
            from_login,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_privmsg_with_tags() {
        let line = "@badge-info=;badges=moderator/1,subscriber/12;color=#FF4500;display-name=SomeMod;emotes=;first-msg=0;id=abc-123;mod=1;subscriber=1;tmi-sent-ts=1700000000000 :somemod!somemod@somemod.tmi.twitch.tv PRIVMSG #CoolStreamer :hello there chat";
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
        let line = "@badges=;display-name=BigRaider;login=bigraider;msg-id=raid;msg-param-displayName=BigRaider;msg-param-login=bigraider;msg-param-viewerCount=1337;system-msg=1337\\sraiders\\sfrom\\sBigRaider\\shave\\sjoined!;tmi-sent-ts=1700000001000 :tmi.twitch.tv USERNOTICE #targetchannel";
        let msg = parse_line(line).unwrap();
        assert_eq!(msg.tag("system-msg"), Some("1337 raiders from BigRaider have joined!"));
        let raid = RaidNotice::from_irc(&msg).unwrap();
        assert_eq!(raid.channel, "targetchannel");
        assert_eq!(raid.from_login, "bigraider");
        assert_eq!(raid.from_display_name, "BigRaider");
        assert_eq!(raid.viewers, 1337);
    }

    #[test]
    fn non_raid_usernotice_is_ignored() {
        let line = "@msg-id=sub;login=x :tmi.twitch.tv USERNOTICE #chan :yay";
        assert!(RaidNotice::from_irc(&parse_line(line).unwrap()).is_none());
    }

    #[test]
    fn parses_ping_and_unescapes() {
        let msg = parse_line("PING :tmi.twitch.tv").unwrap();
        assert_eq!(msg.command, "PING");
        assert_eq!(msg.params, vec!["tmi.twitch.tv"]);
        assert_eq!(unescape_tag(r"a\:b\sc\\d"), r"a;b c\d");
    }
}
