//! Twitch login (OAuth device code flow) and the Helix moderation API.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Scopes the app asks for:
/// - list the channels you moderate (to restrict which channels can be added)
/// - delete messages; timeout / ban / unban; warn
/// - AutoMod held messages, unban requests and suspicious-user messages for the inbox
/// - shoutouts, and sending the optional shoutout chat message as you
/// - the mod log (`channel.moderate` needs all the moderator:read scopes below) and follows
pub const SCOPES: &[&str] = &[
    "user:read:moderated_channels",
    "moderator:manage:chat_messages",
    "moderator:manage:banned_users",
    "moderator:manage:warnings",
    "moderator:manage:automod",
    "moderator:manage:unban_requests",
    "moderator:read:suspicious_users",
    "moderator:manage:shoutouts",
    "user:write:chat",
    "moderator:read:followers",
    "moderator:read:blocked_terms",
    "moderator:read:chat_settings",
    "moderator:read:moderators",
    "moderator:read:vips",
];

/// Parse Twitch's RFC 3339 timestamps (`2016-12-14T20:32:28Z`, optional fraction) to Unix ms.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let time = time.trim_end_matches('Z');
    let time = time.split(['+']).next()?;
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.split(':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    let ms: i64 = frac.chars().chain("000".chars()).take(3).collect::<String>().parse().ok()?;
    // Days since 1970-01-01 (Howard Hinnant's civil-from-days, inverted).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: String,
    pub login: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UnbanRequest {
    pub id: String,
    pub channel: String,
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub text: String,
    pub created_at: i64,
}

const KEYCHAIN_SERVICE: &str = "com.twitchmodpro.app";
const KEYCHAIN_USER: &str = "twitch-tokens";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub id: String,
    pub helix: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints { id: "https://id.twitch.tv".into(), helix: "https://api.twitch.tv/helix".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub user_id: String,
    pub login: String,
    pub scopes: Vec<String>,
}

impl Account {
    pub fn missing_scopes(&self) -> Vec<&'static str> {
        SCOPES.iter().copied().filter(|s| !self.scopes.iter().any(|x| x == s)).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModChannel {
    pub id: String,
    pub login: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStart {
    #[serde(skip)]
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Debug, PartialEq)]
pub enum PollResult {
    Pending,
    SlowDown,
    Done(Tokens),
    Expired,
    Failed(String),
}

#[derive(Debug, PartialEq)]
pub enum ApiError {
    /// The token is invalid and couldn't be refreshed: the user must log in again.
    Unauthorized,
    /// Anything else, with a message suitable for showing to the user.
    Other(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "Your Twitch login expired. Please log in again."),
            ApiError::Other(m) => write!(f, "{m}"),
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError::Other(format!("Couldn't reach Twitch: {e}"))
    }
}

#[derive(Deserialize)]
struct TwitchError {
    #[serde(default)]
    message: String,
}

async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<TwitchError>(&body).map(|e| e.message).unwrap_or_default();
    if msg.is_empty() { format!("Twitch returned {status}") } else { msg }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

pub fn http_client() -> reqwest::Client {
    // rustls needs a process-wide crypto backend; this is a no-op if one is installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("http client")
}

// ---------------- device code login ----------------

pub async fn start_device_login(
    http: &reqwest::Client,
    ep: &Endpoints,
    client_id: &str,
) -> Result<DeviceStart, ApiError> {
    #[derive(Deserialize)]
    struct Resp {
        device_code: String,
        user_code: String,
        verification_uri: String,
        #[serde(default = "five")]
        interval: u64,
        #[serde(default = "thirty_min")]
        expires_in: u64,
    }
    fn five() -> u64 {
        5
    }
    fn thirty_min() -> u64 {
        1800
    }
    let resp = http
        .post(format!("{}/oauth2/device", ep.id))
        .form(&[("client_id", client_id), ("scopes", &SCOPES.join(" "))])
        .send()
        .await?;
    if !resp.status().is_success() {
        let msg = error_message(resp).await;
        return Err(ApiError::Other(if msg.contains("client") {
            format!("Twitch rejected the Client ID ({msg}). Check that it's correct and the app is a Public client.")
        } else {
            msg
        }));
    }
    let r: Resp = resp.json().await?;
    Ok(DeviceStart {
        device_code: r.device_code,
        user_code: r.user_code,
        verification_uri: r.verification_uri,
        interval: r.interval.max(1),
        expires_in: r.expires_in,
    })
}

pub async fn poll_device_login(
    http: &reqwest::Client,
    ep: &Endpoints,
    client_id: &str,
    device_code: &str,
) -> Result<PollResult, ApiError> {
    let resp = http
        .post(format!("{}/oauth2/token", ep.id))
        .form(&[
            ("client_id", client_id),
            ("scopes", &SCOPES.join(" ")),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?;
    if resp.status().is_success() {
        let t: TokenResponse = resp.json().await?;
        return Ok(PollResult::Done(Tokens { access_token: t.access_token, refresh_token: t.refresh_token }));
    }
    let msg = error_message(resp).await;
    let lower = msg.to_lowercase();
    Ok(if lower.contains("authorization_pending") {
        PollResult::Pending
    } else if lower.contains("slow_down") {
        PollResult::SlowDown
    } else if lower.contains("invalid device code") || lower.contains("expired") {
        PollResult::Expired
    } else {
        PollResult::Failed(msg)
    })
}

pub async fn refresh_tokens(
    http: &reqwest::Client,
    ep: &Endpoints,
    client_id: &str,
    refresh_token: &str,
) -> Result<Tokens, ApiError> {
    let resp = http
        .post(format!("{}/oauth2/token", ep.id))
        .form(&[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh_token)])
        .send()
        .await?;
    match resp.status().as_u16() {
        200..=299 => {
            let t: TokenResponse = resp.json().await?;
            Ok(Tokens { access_token: t.access_token, refresh_token: t.refresh_token.or(Some(refresh_token.into())) })
        }
        400 | 401 => Err(ApiError::Unauthorized),
        _ => Err(ApiError::Other(error_message(resp).await)),
    }
}

/// Validate a token. Twitch requires apps to do this at startup and hourly.
pub async fn validate_token(http: &reqwest::Client, ep: &Endpoints, access_token: &str) -> Result<Account, ApiError> {
    #[derive(Deserialize)]
    struct Resp {
        login: String,
        user_id: String,
        #[serde(default)]
        scopes: Vec<String>,
    }
    let resp = http
        .get(format!("{}/oauth2/validate", ep.id))
        .header("Authorization", format!("OAuth {access_token}"))
        .send()
        .await?;
    match resp.status().as_u16() {
        200..=299 => {
            let r: Resp = resp.json().await?;
            Ok(Account { user_id: r.user_id, login: r.login, scopes: r.scopes })
        }
        401 => Err(ApiError::Unauthorized),
        _ => Err(ApiError::Other(error_message(resp).await)),
    }
}

pub async fn revoke_token(http: &reqwest::Client, ep: &Endpoints, client_id: &str, token: &str) {
    let _ = http
        .post(format!("{}/oauth2/revoke", ep.id))
        .form(&[("client_id", client_id), ("token", token)])
        .send()
        .await;
}

// ---------------- keychain ----------------

pub fn load_tokens() -> Option<Tokens> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).ok()?;
    serde_json::from_str(&entry.get_password().ok()?).ok()
}

pub fn save_tokens(tokens: &Tokens) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).map_err(|e| e.to_string())?;
    entry
        .set_password(&serde_json::to_string(tokens).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Couldn't save login to the system keychain: {e}"))
}

pub fn clear_tokens() {
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER) {
        let _ = entry.delete_credential();
    }
}

// ---------------- Helix ----------------

/// An authenticated Helix client. Refreshes the access token once on a 401.
#[derive(Clone)]
pub struct Helix {
    http: reqwest::Client,
    ep: Endpoints,
    client_id: String,
    pub account: Account,
    tokens: Arc<Mutex<Tokens>>,
    /// Called with new tokens after a refresh so they can be persisted.
    on_refresh: Arc<dyn Fn(&Tokens) + Send + Sync>,
}

impl Helix {
    pub fn new(
        http: reqwest::Client,
        ep: Endpoints,
        client_id: String,
        account: Account,
        tokens: Tokens,
        on_refresh: Arc<dyn Fn(&Tokens) + Send + Sync>,
    ) -> Self {
        Helix { http, ep, client_id, account, tokens: Arc::new(Mutex::new(tokens)), on_refresh }
    }

    pub async fn tokens(&self) -> Tokens {
        self.tokens.lock().await.clone()
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    async fn refresh(&self, stale_access: &str) -> Result<(), ApiError> {
        let mut t = self.tokens.lock().await;
        if t.access_token != stale_access {
            return Ok(()); // someone else already refreshed
        }
        let refresh = t.refresh_token.clone().ok_or(ApiError::Unauthorized)?;
        let fresh = refresh_tokens(&self.http, &self.ep, &self.client_id, &refresh).await?;
        (self.on_refresh)(&fresh);
        *t = fresh;
        Ok(())
    }

    /// Send a request built by `build`, retrying once after refreshing an expired token.
    async fn send(
        &self,
        build: impl Fn(&reqwest::Client, &str) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ApiError> {
        for attempt in 0..2 {
            let access = self.tokens.lock().await.access_token.clone();
            let resp = build(&self.http, &self.ep.helix)
                .header("Authorization", format!("Bearer {access}"))
                .header("Client-Id", &self.client_id)
                .send()
                .await?;
            if resp.status().as_u16() == 401 && attempt == 0 {
                self.refresh(&access).await?;
                continue;
            }
            if resp.status().as_u16() == 401 {
                return Err(ApiError::Unauthorized);
            }
            return Ok(resp);
        }
        Err(ApiError::Unauthorized)
    }

    async fn expect_ok(&self, resp: reqwest::Response) -> Result<reqwest::Response, ApiError> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(ApiError::Other(error_message(resp).await))
        }
    }

    /// Every channel the logged-in user can moderate, including their own.
    pub async fn moderated_channels(&self) -> Result<Vec<ModChannel>, ApiError> {
        #[derive(Deserialize)]
        struct Item {
            broadcaster_id: String,
            broadcaster_login: String,
            broadcaster_name: String,
        }
        #[derive(Deserialize, Default)]
        struct Pagination {
            cursor: Option<String>,
        }
        #[derive(Deserialize)]
        struct Page {
            data: Vec<Item>,
            #[serde(default)]
            pagination: Pagination,
        }

        let mut out = vec![ModChannel {
            id: self.account.user_id.clone(),
            login: self.account.login.clone(),
            name: self.account.login.clone(),
        }];
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let user_id = self.account.user_id.clone();
            let after = cursor.clone();
            let resp = self
                .send(|http, base| {
                    let mut q = vec![("user_id", user_id.clone()), ("first", "100".to_string())];
                    if let Some(a) = &after {
                        q.push(("after", a.clone()));
                    }
                    http.get(format!("{base}/moderation/channels")).query(&q)
                })
                .await?;
            let page: Page = self.expect_ok(resp).await?.json().await?;
            for i in page.data {
                if !out.iter().any(|c| c.id == i.broadcaster_id) {
                    out.push(ModChannel { id: i.broadcaster_id, login: i.broadcaster_login, name: i.broadcaster_name });
                }
            }
            match page.pagination.cursor.filter(|c| !c.is_empty()) {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(out)
    }

    /// Look up a user's ID from their login.
    pub async fn user_id(&self, login: &str) -> Result<String, ApiError> {
        #[derive(Deserialize)]
        struct Item {
            id: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            data: Vec<Item>,
        }
        let login = login.to_string();
        let resp = self.send(|http, base| http.get(format!("{base}/users")).query(&[("login", &login)])).await?;
        let r: Resp = self.expect_ok(resp).await?.json().await?;
        r.data.into_iter().next().map(|u| u.id).ok_or_else(|| ApiError::Other(format!("No Twitch user named {login}")))
    }

    /// A channel's follower count. Twitch returns the total for any channel, even ones you don't moderate.
    pub async fn follower_total(&self, broadcaster_id: &str) -> Result<i64, ApiError> {
        #[derive(Deserialize)]
        struct Resp {
            total: i64,
        }
        let id = broadcaster_id.to_string();
        let resp = self
            .send(|http, base| http.get(format!("{base}/channels/followers")).query(&[("broadcaster_id", id.as_str()), ("first", "1")]))
            .await?;
        let r: Resp = self.expect_ok(resp).await?.json().await?;
        Ok(r.total)
    }

    /// Look up accounts by login (up to 100 at a time), mainly for their creation dates.
    pub async fn users(&self, logins: &[String]) -> Result<Vec<UserInfo>, ApiError> {
        #[derive(Deserialize)]
        struct Item {
            id: String,
            login: String,
            created_at: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            data: Vec<Item>,
        }
        let mut out = Vec::new();
        for chunk in logins.chunks(100) {
            let q: Vec<(&str, &str)> = chunk.iter().map(|l| ("login", l.as_str())).collect();
            let resp = self.send(|http, base| http.get(format!("{base}/users")).query(&q)).await?;
            let r: Resp = self.expect_ok(resp).await?.json().await?;
            out.extend(r.data.into_iter().filter_map(|u| {
                Some(UserInfo { created_at: parse_rfc3339_ms(&u.created_at)?, id: u.id, login: u.login })
            }));
        }
        Ok(out)
    }

    /// Send a Twitch warning the user must acknowledge before chatting again.
    pub async fn warn(&self, broadcaster_id: &str, user_id: &str, reason: &str) -> Result<(), ApiError> {
        let q = [("broadcaster_id", broadcaster_id.to_string()), ("moderator_id", self.account.user_id.clone())];
        let reason: String = reason.trim().chars().take(500).collect();
        let reason = if reason.is_empty() { "Please follow the chat rules.".to_string() } else { reason };
        let body = serde_json::json!({ "data": { "user_id": user_id, "reason": reason } });
        let resp = self
            .send(|http, base| http.post(format!("{base}/moderation/warnings")).query(&q).json(&body))
            .await?;
        self.expect_ok(resp).await.map(|_| ())
    }

    /// Allow or deny a message AutoMod is holding.
    pub async fn automod_decide(&self, message_id: &str, allow: bool) -> Result<(), ApiError> {
        let body = serde_json::json!({
            "user_id": self.account.user_id,
            "msg_id": message_id,
            "action": if allow { "ALLOW" } else { "DENY" },
        });
        let resp = self.send(|http, base| http.post(format!("{base}/moderation/automod/message")).json(&body)).await?;
        self.expect_ok(resp).await.map(|_| ())
    }

    pub async fn pending_unban_requests(&self, broadcaster_id: &str, channel: &str) -> Result<Vec<UnbanRequest>, ApiError> {
        #[derive(Deserialize)]
        struct Item {
            id: String,
            user_id: String,
            user_login: String,
            user_name: String,
            #[serde(default)]
            text: String,
            created_at: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            data: Vec<Item>,
        }
        let q = [
            ("broadcaster_id", broadcaster_id.to_string()),
            ("moderator_id", self.account.user_id.clone()),
            ("status", "pending".to_string()),
            ("first", "100".to_string()),
        ];
        let resp = self.send(|http, base| http.get(format!("{base}/moderation/unban_requests")).query(&q)).await?;
        let r: Resp = self.expect_ok(resp).await?.json().await?;
        Ok(r.data
            .into_iter()
            .map(|i| UnbanRequest {
                created_at: parse_rfc3339_ms(&i.created_at).unwrap_or(0),
                id: i.id,
                channel: channel.to_string(),
                user_id: i.user_id,
                user_login: i.user_login,
                user_name: i.user_name,
                text: i.text,
            })
            .collect())
    }

    pub async fn resolve_unban_request(
        &self,
        broadcaster_id: &str,
        request_id: &str,
        approve: bool,
        resolution_text: &str,
    ) -> Result<(), ApiError> {
        let mut q = vec![
            ("broadcaster_id", broadcaster_id.to_string()),
            ("moderator_id", self.account.user_id.clone()),
            ("unban_request_id", request_id.to_string()),
            ("status", if approve { "approved" } else { "denied" }.to_string()),
        ];
        if !resolution_text.trim().is_empty() {
            q.push(("resolution_text", resolution_text.trim().chars().take(500).collect()));
        }
        let resp = self.send(|http, base| http.patch(format!("{base}/moderation/unban_requests")).query(&q)).await?;
        self.expect_ok(resp).await.map(|_| ())
    }

    /// Subscribe an EventSub WebSocket session to an event for one channel. Returns the subscription ID.
    pub async fn eventsub_subscribe(
        &self,
        session_id: &str,
        kind: &str,
        version: &str,
        broadcaster_id: &str,
    ) -> Result<String, ApiError> {
        #[derive(Deserialize)]
        struct Item {
            id: String,
        }
        #[derive(Deserialize)]
        struct Resp {
            data: Vec<Item>,
        }
        let body = serde_json::json!({
            "type": kind,
            "version": version,
            "condition": { "broadcaster_user_id": broadcaster_id, "moderator_user_id": self.account.user_id },
            "transport": { "method": "websocket", "session_id": session_id },
        });
        let resp = self.send(|http, base| http.post(format!("{base}/eventsub/subscriptions")).json(&body)).await?;
        let r: Resp = self.expect_ok(resp).await?.json().await?;
        r.data.into_iter().next().map(|i| i.id).ok_or_else(|| ApiError::Other("Twitch returned no subscription".into()))
    }

    pub async fn eventsub_unsubscribe(&self, subscription_id: &str) -> Result<(), ApiError> {
        let q = [("id", subscription_id.to_string())];
        let resp = self.send(|http, base| http.delete(format!("{base}/eventsub/subscriptions")).query(&q)).await?;
        self.expect_ok(resp).await.map(|_| ())
    }

    /// Twitch's native shoutout. Twitch only allows it while the channel is live, once every
    /// 2 minutes per channel, and once an hour for the same streamer.
    pub async fn shoutout(&self, from_broadcaster_id: &str, to_broadcaster_id: &str) -> Result<(), ApiError> {
        let q = [
            ("from_broadcaster_id", from_broadcaster_id.to_string()),
            ("to_broadcaster_id", to_broadcaster_id.to_string()),
            ("moderator_id", self.account.user_id.clone()),
        ];
        let resp = self.send(|http, base| http.post(format!("{base}/chat/shoutouts")).query(&q)).await?;
        if resp.status().as_u16() == 429 {
            return Err(ApiError::Other(
                "Twitch limits shoutouts to one every 2 minutes per channel, and once an hour per streamer.".into(),
            ));
        }
        self.expect_ok(resp).await.map(|_| ())
    }

    /// Send a chat message as the logged-in user.
    pub async fn send_chat_message(&self, broadcaster_id: &str, message: &str) -> Result<(), ApiError> {
        #[derive(Deserialize)]
        struct Item {
            is_sent: bool,
            #[serde(default)]
            drop_reason: Option<serde_json::Value>,
        }
        #[derive(Deserialize)]
        struct Resp {
            data: Vec<Item>,
        }
        let body = serde_json::json!({
            "broadcaster_id": broadcaster_id,
            "sender_id": self.account.user_id,
            "message": message.chars().take(500).collect::<String>(),
        });
        let resp = self.send(|http, base| http.post(format!("{base}/chat/messages")).json(&body)).await?;
        let r: Resp = self.expect_ok(resp).await?.json().await?;
        match r.data.into_iter().next() {
            Some(i) if i.is_sent => Ok(()),
            Some(i) => Err(ApiError::Other(format!(
                "Twitch didn't send the message: {}",
                i.drop_reason.and_then(|d| d["message"].as_str().map(str::to_string)).unwrap_or_else(|| "no reason given".into())
            ))),
            None => Err(ApiError::Other("Twitch didn't send the message.".into())),
        }
    }

    pub async fn delete_message(&self, broadcaster_id: &str, message_id: &str) -> Result<(), ApiError> {
        let q = [
            ("broadcaster_id", broadcaster_id.to_string()),
            ("moderator_id", self.account.user_id.clone()),
            ("message_id", message_id.to_string()),
        ];
        let resp = self.send(|http, base| http.delete(format!("{base}/moderation/chat")).query(&q)).await?;
        self.expect_ok(resp).await.map(|_| ())
    }

    /// Ban a user, or time them out when `duration_secs` is set (1 to 1,209,600).
    pub async fn ban(
        &self,
        broadcaster_id: &str,
        user_id: &str,
        duration_secs: Option<u32>,
        reason: &str,
    ) -> Result<(), ApiError> {
        let q = [("broadcaster_id", broadcaster_id.to_string()), ("moderator_id", self.account.user_id.clone())];
        let mut data = serde_json::json!({ "user_id": user_id });
        if let Some(d) = duration_secs {
            data["duration"] = d.clamp(1, 1_209_600).into();
        }
        if !reason.trim().is_empty() {
            data["reason"] = reason.trim().chars().take(500).collect::<String>().into();
        }
        let body = serde_json::json!({ "data": data });
        let resp = self
            .send(|http, base| http.post(format!("{base}/moderation/bans")).query(&q).json(&body))
            .await?;
        self.expect_ok(resp).await.map(|_| ())
    }

    pub async fn unban(&self, broadcaster_id: &str, user_id: &str) -> Result<(), ApiError> {
        let q = [
            ("broadcaster_id", broadcaster_id.to_string()),
            ("moderator_id", self.account.user_id.clone()),
            ("user_id", user_id.to_string()),
        ];
        let resp = self.send(|http, base| http.delete(format!("{base}/moderation/bans")).query(&q)).await?;
        self.expect_ok(resp).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{body_json, body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ep(server: &MockServer) -> Endpoints {
        Endpoints { id: server.uri(), helix: format!("{}/helix", server.uri()) }
    }

    fn account() -> Account {
        Account { user_id: "999".into(), login: "me".into(), scopes: SCOPES.iter().map(|s| s.to_string()).collect() }
    }

    fn helix(server: &MockServer, refreshes: Arc<AtomicUsize>) -> Helix {
        Helix::new(
            http_client(),
            ep(server),
            "cid".into(),
            account(),
            Tokens { access_token: "old".into(), refresh_token: Some("r1".into()) },
            Arc::new(move |_| {
                refreshes.fetch_add(1, Ordering::SeqCst);
            }),
        )
    }

    #[test]
    fn parses_twitch_timestamps() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_ms("2016-12-14T20:32:28Z"), Some(1_481_747_548_000));
        assert_eq!(parse_rfc3339_ms("2016-12-14T20:32:28.5Z"), Some(1_481_747_548_500));
        assert_eq!(parse_rfc3339_ms("2024-02-29T12:00:00.123456Z"), Some(1_709_208_000_123));
        assert_eq!(parse_rfc3339_ms("garbage"), None);
    }

    #[tokio::test]
    async fn users_warn_automod_and_unban_requests() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(query_param("login", "newbie"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "5", "login": "newbie", "display_name": "Newbie", "created_at": "2016-12-14T20:32:28Z"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/moderation/warnings"))
            .and(body_json(serde_json::json!({"data": {"user_id": "5", "reason": "Please follow the chat rules."}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/moderation/automod/message"))
            .and(body_json(serde_json::json!({"user_id": "999", "msg_id": "m1", "action": "DENY"})))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/moderation/unban_requests"))
            .and(query_param("status", "pending"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "r1", "broadcaster_id": "1", "user_id": "5", "user_login": "newbie", "user_name": "Newbie",
                          "text": "sorry", "status": "pending", "created_at": "2016-12-14T20:32:28Z"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/helix/moderation/unban_requests"))
            .and(query_param("unban_request_id", "r1"))
            .and(query_param("status", "denied"))
            .and(query_param("resolution_text", "no"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/eventsub/subscriptions"))
            .and(body_string_contains("automod.message.hold"))
            .and(body_string_contains("\"session_id\":\"sess\""))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({"data": [{"id": "sub1"}]})))
            .mount(&server)
            .await;

        let h = helix(&server, Arc::new(AtomicUsize::new(0)));
        let users = h.users(&["newbie".into()]).await.unwrap();
        assert_eq!(users[0].created_at, 1_481_747_548_000);
        h.warn("1", "5", "  ").await.unwrap();
        h.automod_decide("m1", false).await.unwrap();
        let reqs = h.pending_unban_requests("1", "chan").await.unwrap();
        assert_eq!((reqs[0].id.as_str(), reqs[0].channel.as_str(), reqs[0].text.as_str()), ("r1", "chan", "sorry"));
        h.resolve_unban_request("1", "r1", false, "no").await.unwrap();
        assert_eq!(h.eventsub_subscribe("sess", "automod.message.hold", "2", "1").await.unwrap(), "sub1");
    }

    #[tokio::test]
    async fn shoutout_and_chat_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/shoutouts"))
            .and(query_param("from_broadcaster_id", "1"))
            .and(query_param("to_broadcaster_id", "2"))
            .and(query_param("moderator_id", "999"))
            .respond_with(ResponseTemplate::new(204))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/shoutouts"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(body_json(serde_json::json!({"broadcaster_id": "1", "sender_id": "999", "message": "!so @raider"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": [{"message_id": "x", "is_sent": true}]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"message_id": "", "is_sent": false, "drop_reason": {"code": "msg_duplicate", "message": "Duplicate message"}}]
            })))
            .mount(&server)
            .await;
        let h = helix(&server, Arc::new(AtomicUsize::new(0)));
        h.shoutout("1", "2").await.unwrap();
        assert!(h.shoutout("1", "2").await.unwrap_err().to_string().contains("2 minutes"));
        h.send_chat_message("1", "!so @raider").await.unwrap();
        assert!(h.send_chat_message("1", "again").await.unwrap_err().to_string().contains("Duplicate message"));
    }

    #[tokio::test]
    async fn device_flow_start_and_poll() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/device"))
            .and(body_string_contains("client_id=cid"))
            .and(body_string_contains("moderator%3Amanage%3Abanned_users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "device_code": "dev", "user_code": "ABCD1234", "verification_uri": "https://www.twitch.tv/activate?device-code=ABCD1234",
                "interval": 5, "expires_in": 1800
            })))
            .mount(&server)
            .await;
        let http = http_client();
        let start = start_device_login(&http, &ep(&server), "cid").await.unwrap();
        assert_eq!(start.user_code, "ABCD1234");

        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"status":400,"message":"authorization_pending"})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok", "refresh_token": "ref", "expires_in": 14000, "scope": SCOPES, "token_type": "bearer"
            })))
            .mount(&server)
            .await;
        assert_eq!(poll_device_login(&http, &ep(&server), "cid", "dev").await.unwrap(), PollResult::Pending);
        assert_eq!(
            poll_device_login(&http, &ep(&server), "cid", "dev").await.unwrap(),
            PollResult::Done(Tokens { access_token: "tok".into(), refresh_token: Some("ref".into()) })
        );
    }

    #[tokio::test]
    async fn expired_device_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"status":400,"message":"invalid device code"})))
            .mount(&server)
            .await;
        assert_eq!(poll_device_login(&http_client(), &ep(&server), "cid", "dev").await.unwrap(), PollResult::Expired);
    }

    #[tokio::test]
    async fn validate_reports_account_and_missing_scopes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/oauth2/validate"))
            .and(header("Authorization", "OAuth good"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "client_id": "cid", "login": "me", "user_id": "999", "scopes": ["user:read:moderated_channels"], "expires_in": 100
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/oauth2/validate"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let acct = validate_token(&http_client(), &ep(&server), "good").await.unwrap();
        assert_eq!(acct.login, "me");
        assert_eq!(acct.missing_scopes(), SCOPES[1..].to_vec());
        assert_eq!(validate_token(&http_client(), &ep(&server), "bad").await, Err(ApiError::Unauthorized));
    }

    #[tokio::test]
    async fn moderated_channels_paginates_and_includes_own_channel() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/helix/moderation/channels"))
            .and(query_param("after", "c1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"broadcaster_id": "2", "broadcaster_login": "two", "broadcaster_name": "Two"}],
                "pagination": {}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/moderation/channels"))
            .and(query_param("user_id", "999"))
            .and(header("Client-Id", "cid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"broadcaster_id": "1", "broadcaster_login": "one", "broadcaster_name": "One"}],
                "pagination": {"cursor": "c1"}
            })))
            .mount(&server)
            .await;
        let chans = helix(&server, Arc::new(AtomicUsize::new(0))).moderated_channels().await.unwrap();
        let logins: Vec<_> = chans.iter().map(|c| c.login.as_str()).collect();
        assert_eq!(logins, vec!["me", "one", "two"]);
    }

    #[tokio::test]
    async fn refreshes_expired_token_once_then_retries() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/helix/moderation/chat"))
            .and(header("Authorization", "Bearer old"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": "new", "refresh_token": "r2"})))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/helix/moderation/chat"))
            .and(header("Authorization", "Bearer new"))
            .and(query_param("message_id", "m1"))
            .and(query_param("moderator_id", "999"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let refreshes = Arc::new(AtomicUsize::new(0));
        let h = helix(&server, refreshes.clone());
        h.delete_message("1", "m1").await.unwrap();
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(h.tokens().await.access_token, "new");
    }

    #[tokio::test]
    async fn ban_and_timeout_send_correct_body_and_surface_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/moderation/bans"))
            .and(body_json(serde_json::json!({"data": {"user_id": "5", "duration": 600, "reason": "spam"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/moderation/bans"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "Bad Request", "status": 400, "message": "The user specified in the user_id field is already banned."
            })))
            .mount(&server)
            .await;
        let h = helix(&server, Arc::new(AtomicUsize::new(0)));
        h.ban("1", "5", Some(600), " spam ").await.unwrap();
        let err = h.ban("1", "6", None, "").await.unwrap_err();
        assert!(err.to_string().contains("already banned"));
    }

    #[tokio::test]
    async fn failed_refresh_means_logged_out() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/helix/moderation/bans"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"status":400,"message":"Invalid refresh token"})))
            .mount(&server)
            .await;
        let h = helix(&server, Arc::new(AtomicUsize::new(0)));
        assert_eq!(h.unban("1", "5").await, Err(ApiError::Unauthorized));
    }
}
