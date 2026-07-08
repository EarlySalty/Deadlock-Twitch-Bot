//! Helix-Chat-Endpoints — Nachrichten senden, Ankündigungen, Bans, Löschen.
//!
//! Port von `bot/chat/moderation.py:1293–1903` (send/announcement/ban/unban/delete).
//! Alle Endpoints brauchen einen **User-Token** des Bot-Accounts (nicht App-Token),
//! deshalb expliziter `user_token`-Parameter analog zu `raid.rs`/`moderation.rs`.

use crate::client::{HelixClient, HelixError};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Typen
// ---------------------------------------------------------------------------

/// Ergebnis eines `send_chat_message`-Aufrufs.
/// HTTP-200 mit `is_sent=false` = Drop — diesen Fall separat auswerten!
///
/// Port von `moderation.py:1435–1500` (is_sent/drop_reason-Parsing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// Nachricht wurde zugestellt (`is_sent=true` oder 204).
    Sent,
    /// Twitch hat die Nachricht intern verworfen (`is_sent=false`).
    ///
    /// `code` z. B. `"sender_banned"`, `"sender_timedout"`, `"channel_settings"`.
    /// `message` ist der menschenlesbare Grund aus der Helix-Antwort.
    Dropped { code: String, message: String },
    /// HTTP-Fehler (4xx/5xx) der nicht durch den 2-Attempt-Retry aufgelöst wurde.
    HttpError { status: u16, body: String },
}

/// Ergebnis eines `POST /chat/announcements`-Aufrufs.
///
/// Additiv zum alten bool-Vertrag: bestehende Aufrufer können weiter nur
/// [`AnnouncementOutcome::accepted`] auswerten, Debug-Pfade bekommen Status und
/// Body-Snippet für Twitch-Ablehnungen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnouncementOutcome {
    pub accepted: bool,
    pub status: Option<u16>,
    pub detail: Option<String>,
}

impl AnnouncementOutcome {
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            status: None,
            detail: None,
        }
    }

    pub fn rejected(status: u16, detail: String) -> Self {
        let detail = redact_announcement_detail(&detail);
        let detail = detail.trim();
        Self {
            accepted: false,
            status: Some(status),
            detail: if detail.is_empty() {
                None
            } else {
                Some(detail.chars().take(300).collect())
            },
        }
    }

    pub fn from_bool(accepted: bool) -> Self {
        if accepted {
            Self::accepted()
        } else {
            Self {
                accepted: false,
                status: None,
                detail: None,
            }
        }
    }
}

/// Ergebnis eines `POST /whispers`-Aufrufs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperOutcome {
    pub accepted: bool,
    pub status: Option<u16>,
    pub detail: Option<String>,
}

impl WhisperOutcome {
    pub fn accepted() -> Self {
        Self {
            accepted: true,
            status: None,
            detail: None,
        }
    }

    pub fn rejected(status: u16, detail: String) -> Self {
        let detail = redact_announcement_detail(&detail);
        let detail = detail.trim();
        Self {
            accepted: false,
            status: Some(status),
            detail: if detail.is_empty() {
                None
            } else {
                Some(detail.chars().take(300).collect())
            },
        }
    }
}

fn mask_secret(value: &str) -> String {
    if value.is_empty() {
        "[redacted]".to_string()
    } else {
        format!("[redacted:{}]", value.len().min(999))
    }
}

struct AnnouncementSecretPatterns {
    header: Regex,
    bearer: Regex,
    quoted_kv: Regex,
    kv: Regex,
    query: Regex,
    jwt: Regex,
}

fn announcement_secret_patterns() -> &'static AnnouncementSecretPatterns {
    static PATTERNS: OnceLock<AnnouncementSecretPatterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        const KEYS: &str = r"access[_-]?token|refresh[_-]?token|id[_-]?token|client[_-]?secret|authorization";
        let compile = |p: &str| Regex::new(p).unwrap_or_else(|_| Regex::new(r"$.^").unwrap());
        AnnouncementSecretPatterns {
            header: compile(r"(?i)\b(authorization\s*[:=]\s*(?:bearer\s+)?)([^\s,;}]+)"),
            bearer: compile(r"(?i)\b(bearer\s+)([A-Za-z0-9._~+/=-]{8,})"),
            quoted_kv: compile(&format!(
                r#"(?i)((?:"|')(?:{KEYS})(?:"|')\s*:\s*)("[^"]*"|'[^']*'|[^\s,;}}]+)"#
            )),
            kv: compile(&format!(
                r#"(?i)\b({KEYS})(\s*[:=]\s*)("[^"]+"|'[^']+'|[^\s,;&}}]+)"#
            )),
            query: compile(&format!(r"(?i)\b({KEYS})=([^&\s]+)")),
            jwt: compile(r"\beyJ[a-zA-Z0-9_-]{8,}\.[a-zA-Z0-9._-]{8,}\.[a-zA-Z0-9._-]{8,}\b"),
        }
    })
}

fn redact_announcement_detail(raw: &str) -> String {
    let p = announcement_secret_patterns();
    let s = p
        .header
        .replace_all(raw, |c: &regex::Captures| format!("{}{}", &c[1], mask_secret(&c[2])));
    let s = p
        .bearer
        .replace_all(&s, |c: &regex::Captures| format!("{}{}", &c[1], mask_secret(&c[2])));
    let s = p
        .quoted_kv
        .replace_all(&s, |c: &regex::Captures| format!("{}{}", &c[1], mask_secret(&c[2])));
    let s = p.kv.replace_all(&s, |c: &regex::Captures| {
        format!("{}{}{}", &c[1], &c[2], mask_secret(&c[3]))
    });
    let s = p
        .query
        .replace_all(&s, |c: &regex::Captures| format!("{}={}", &c[1], mask_secret(&c[2])));
    p.jwt.replace_all(&s, mask_secret("[jwt]").as_str()).into_owned()
}

/// Drop-Reason aus der Helix-Antwort auf `POST /chat/messages`.
///
/// Port: `moderation.py:1490–1498`.
#[derive(Debug, Deserialize, Clone)]
struct DropReason {
    pub code: String,
    pub message: String,
}

/// Einzelnes `data[0]`-Objekt aus der Send-Message-Antwort.
#[derive(Debug, Deserialize)]
struct SendMessageData {
    pub is_sent: bool,
    pub drop_reason: Option<DropReason>,
}

/// Vollständige Antwort auf `POST /chat/messages`.
#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    pub data: Vec<SendMessageData>,
}

/// Ergebnis eines Ban- oder Unban-Aufrufs.
///
/// Port: `_auto_ban_and_cleanup` + `_unban_user` (moderation.py Z. 1679–1903).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BanOutcome {
    /// 200/201/202 — Ban ausgeführt.
    Banned,
    /// 400 mit „already banned" im Body.
    AlreadyBanned,
    /// 200/204 (Unban).
    Unbanned,
    /// 403 — Bot hat keine Moderator-Rechte.
    Forbidden,
    /// Sonstiger HTTP-Fehler.
    Failed { status: u16, body: String },
}

/// Payload für `POST /moderation/bans`.
#[derive(Debug, Serialize)]
struct BanPayload<'a> {
    data: BanData<'a>,
}

#[derive(Debug, Serialize)]
struct BanData<'a> {
    user_id: &'a str,
    reason: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<u32>,
}

/// Einzelner Twitch-User aus `GET /users`.
#[derive(Debug, Deserialize, Clone)]
pub struct HelixUserInfo {
    pub id: String,
    pub login: String,
    pub display_name: String,
    /// ISO-8601-Timestamp (TEXT in Helix-Antwort).
    /// Port: `fetch_users(ids=[...])` in `bot.py:1617–1627`.
    pub created_at: String,
}

/// Antwort auf `GET /users`.
#[derive(Debug, Deserialize)]
struct UsersResponse {
    pub data: Vec<HelixUserInfo>,
}

/// Payload für `POST /chat/announcements`.
#[derive(Debug, Serialize)]
struct AnnouncementPayload<'a> {
    message: &'a str,
    color: &'a str,
}

/// Ein einzelner Chatter aus `GET /chat/chatters` (Datenquelle des
/// Lurker-/Snapshot-Pollers, Block 6).
///
/// Port: `twitch_api.py:get_chatters_result` — die Daten-Items tragen
/// `user_id`/`user_login`/`user_name`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Chatter {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub user_login: String,
    #[serde(default)]
    pub user_name: String,
}

/// Cursor-Pagination-Block einer Helix-Antwort.
#[derive(Debug, Default, Deserialize)]
struct ChattersPagination {
    #[serde(default)]
    cursor: Option<String>,
}

/// Eine Seite von `GET /chat/chatters`.
#[derive(Debug, Deserialize)]
struct ChattersPage {
    #[serde(default)]
    data: Vec<Chatter>,
    #[serde(default)]
    pagination: ChattersPagination,
}

// ---------------------------------------------------------------------------
// HelixClient-Erweiterung
// ---------------------------------------------------------------------------

impl HelixClient {
    /// Sendet eine Chat-Nachricht via `POST /chat/messages`.
    ///
    /// HTTP-200 mit `is_sent=false` → [`SendOutcome::Dropped`] (kein Fehler!).
    /// `drop_reason.code` `sender_banned`/`sender_timedout` → TimeoutGuard-Trigger
    /// (im oberen Layer, nicht hier).
    ///
    /// Port: `moderation.py:1389–1542`, Weg 2 (Helix API).
    /// Scope: `user:write:chat`.
    pub async fn send_chat_message(
        &self,
        broadcaster_id: &str,
        sender_id: &str,
        message: &str,
        user_token: &str,
    ) -> Result<SendOutcome, HelixError> {
        let url = format!("{}/chat/messages", self.helix_config().helix_base);
        let body = serde_json::json!({
            "broadcaster_id": broadcaster_id,
            "sender_id": sender_id,
            "message": message,
        });
        let resp = self
            .http_client()
            .post(&url)
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        match status {
            200 => {
                // Helix gibt 200 zurück auch wenn die Nachricht verworfen wurde —
                // is_sent=false + drop_reason auswerten.
                let parsed: SendMessageResponse = resp.json().await.map_err(HelixError::Http)?;
                if let Some(item) = parsed.data.first() {
                    if !item.is_sent {
                        let (code, msg) = item
                            .drop_reason
                            .as_ref()
                            .map(|r| (r.code.clone(), r.message.clone()))
                            .unwrap_or_else(|| ("unknown".to_string(), String::new()));
                        return Ok(SendOutcome::Dropped { code, message: msg });
                    }
                }
                Ok(SendOutcome::Sent)
            }
            204 => Ok(SendOutcome::Sent),
            status => {
                let body_text = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Chat-Send: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                let snippet: String = body_text.chars().take(300).collect();
                Ok(SendOutcome::HttpError {
                    status,
                    body: snippet,
                })
            }
        }
    }

    /// Sendet einen Whisper via `POST /whispers`.
    ///
    /// Scope: `user:manage:whispers`; `from_user_id` muss zur User-Token-
    /// Identität passen.
    pub async fn send_whisper(
        &self,
        from_user_id: &str,
        to_user_id: &str,
        message: &str,
        user_token: &str,
    ) -> Result<WhisperOutcome, HelixError> {
        let url = format!("{}/whispers", self.helix_config().helix_base);
        let body = serde_json::json!({ "message": message });
        let resp = self
            .http_client()
            .post(&url)
            .query(&[("from_user_id", from_user_id), ("to_user_id", to_user_id)])
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        match status {
            200 | 204 => Ok(WhisperOutcome::accepted()),
            status => {
                let body_text = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Whisper: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                Ok(WhisperOutcome::rejected(status, body_text))
            }
        }
    }

    /// Sendet eine Kanal-Ankündigung via `POST /chat/announcements`.
    ///
    /// `color` — `"blue"`, `"green"`, `"orange"`, `"purple"` (Default: `"purple"`).
    ///
    /// Port: `moderation.py:1293–1387`.
    /// Scope: `moderator:manage:announcements`.
    pub async fn send_announcement_detailed(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        message: &str,
        color: &str,
        user_token: &str,
    ) -> Result<AnnouncementOutcome, HelixError> {
        let url = format!("{}/chat/announcements", self.helix_config().helix_base);
        let payload = AnnouncementPayload { message, color };
        let resp = self
            .http_client()
            .post(&url)
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .header("Content-Type", "application/json")
            .query(&[
                ("broadcaster_id", broadcaster_id),
                ("moderator_id", moderator_id),
            ])
            .json(&payload)
            .send()
            .await?;

        let status = resp.status().as_u16();
        if matches!(status, 200 | 204) {
            return Ok(AnnouncementOutcome::accepted());
        }
        let body = match resp.text().await {
            Ok(body) => body,
            Err(error) => {
                tracing::warn!(%error, status, "Twitch Announcement: Fehlerbody nicht lesbar");
                String::new()
            }
        };
        Ok(AnnouncementOutcome::rejected(status, body))
    }

    pub async fn send_announcement(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        message: &str,
        color: &str,
        user_token: &str,
    ) -> Result<bool, HelixError> {
        self.send_announcement_detailed(broadcaster_id, moderator_id, message, color, user_token)
            .await
            .map(|outcome| outcome.accepted)
    }

    /// Bannt einen User via `POST /moderation/bans`.
    ///
    /// `duration` = None → permanenter Ban; Some(n) → Timeout in Sekunden.
    ///
    /// Port: `moderation.py:1679–1816`.
    /// Scope: `moderator:manage:banned_users`.
    pub async fn ban_user(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        user_id: &str,
        reason: &str,
        duration: Option<u32>,
        user_token: &str,
    ) -> Result<BanOutcome, HelixError> {
        let url = format!("{}/moderation/bans", self.helix_config().helix_base);
        let payload = BanPayload {
            data: BanData {
                user_id,
                reason,
                duration,
            },
        };
        let resp = self
            .http_client()
            .post(&url)
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .header("Content-Type", "application/json")
            .query(&[
                ("broadcaster_id", broadcaster_id),
                ("moderator_id", moderator_id),
            ])
            .json(&payload)
            .send()
            .await?;

        let status = resp.status().as_u16();
        match status {
            200..=202 => Ok(BanOutcome::Banned),
            400 => {
                let body_text = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Ban: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                if body_text.to_lowercase().contains("already banned") {
                    Ok(BanOutcome::AlreadyBanned)
                } else {
                    let snippet: String = body_text.chars().take(300).collect();
                    Ok(BanOutcome::Failed {
                        status,
                        body: snippet,
                    })
                }
            }
            403 => Ok(BanOutcome::Forbidden),
            status => {
                let body_text = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Ban: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                let snippet: String = body_text.chars().take(300).collect();
                Ok(BanOutcome::Failed {
                    status,
                    body: snippet,
                })
            }
        }
    }

    /// Hebt einen Ban auf via `DELETE /moderation/bans`.
    ///
    /// Port: `moderation.py:1831–1903` (`_unban_user`).
    /// Scope: `moderator:manage:banned_users`.
    pub async fn unban_user(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        user_id: &str,
        user_token: &str,
    ) -> Result<BanOutcome, HelixError> {
        let url = format!("{}/moderation/bans", self.helix_config().helix_base);
        let resp = self
            .http_client()
            .delete(&url)
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .query(&[
                ("broadcaster_id", broadcaster_id),
                ("moderator_id", moderator_id),
                ("user_id", user_id),
            ])
            .send()
            .await?;

        let status = resp.status().as_u16();
        match status {
            200 | 204 => Ok(BanOutcome::Unbanned),
            403 => Ok(BanOutcome::Forbidden),
            status => {
                let body_text = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Unban: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                let snippet: String = body_text.chars().take(300).collect();
                Ok(BanOutcome::Failed {
                    status,
                    body: snippet,
                })
            }
        }
    }

    /// Löscht eine Chat-Nachricht via `DELETE /moderation/chat`.
    ///
    /// Port: `moderation.py:1631–1666` (Schritt 1 in `_auto_ban_and_cleanup`).
    /// Scope: `moderator:manage:chat_messages`.
    pub async fn delete_chat_message(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        message_id: &str,
        user_token: &str,
    ) -> Result<bool, HelixError> {
        let url = format!("{}/moderation/chat", self.helix_config().helix_base);
        let resp = self
            .http_client()
            .delete(&url)
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .query(&[
                ("broadcaster_id", broadcaster_id),
                ("moderator_id", moderator_id),
                ("message_id", message_id),
            ])
            .send()
            .await?;

        Ok(matches!(resp.status().as_u16(), 200 | 204))
    }

    /// Holt User-Infos (inkl. `created_at`) für eine Liste von User-IDs.
    ///
    /// Port: `bot.py:1617–1627` — Account-Alter-Eskalation.
    /// Rückgabe: Vec sortiert wie Eingabe (unbekannte IDs werden weggelassen).
    pub async fn get_users_created_at(
        &self,
        ids: &[&str],
        user_token: &str,
    ) -> Result<Vec<HelixUserInfo>, HelixError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let url = format!("{}/users", self.helix_config().helix_base);
        let params: Vec<(&str, &str)> = ids.iter().map(|id| ("id", *id)).collect();
        let resp = self
            .http_client()
            .get(&url)
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .query(&params)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(vec![]);
        }
        let parsed: UsersResponse = resp.json().await.map_err(HelixError::Http)?;
        Ok(parsed.data)
    }

    /// Holt User-Info für einen einzelnen Login-Namen.
    ///
    /// Port: `_resolve_existing_twitch_users` (moderation.py Z. 358–427).
    pub async fn get_user_by_login(
        &self,
        login: &str,
        user_token: &str,
    ) -> Result<Option<HelixUserInfo>, HelixError> {
        let url = format!("{}/users", self.helix_config().helix_base);
        let resp = self
            .http_client()
            .get(&url)
            .header("Client-Id", &self.helix_config().client_id)
            .header("Authorization", format!("Bearer {user_token}"))
            .query(&[("login", login)])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }
        let parsed: UsersResponse = resp.json().await.map_err(HelixError::Http)?;
        Ok(parsed.data.into_iter().next())
    }

    /// Alle Chatter eines Channels via `GET /chat/chatters` (Cursor-Pagination
    /// über sämtliche Seiten, `first=1000`). Datenquelle für den Block-6-Lurker-
    /// Poller.
    ///
    /// `broadcaster_id == moderator_id`, wenn der Streamer selbst seinen Chat
    /// abfragt. 403 → [`HelixError::NotModerator`] (Trigger für Mod-Self-Heal im
    /// oberen Layer); sonstiges non-200 → [`HelixError::Status`].
    ///
    /// Port: `twitch_api.py:get_chatters_result`.
    /// Scope: `moderator:read:chatters`.
    pub async fn get_chatters(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        user_token: &str,
    ) -> Result<Vec<Chatter>, HelixError> {
        let url = format!("{}/chat/chatters", self.helix_config().helix_base);
        let mut out: Vec<Chatter> = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut params: Vec<(&str, String)> = vec![
                ("broadcaster_id", broadcaster_id.to_string()),
                ("moderator_id", moderator_id.to_string()),
                ("first", "1000".to_string()),
            ];
            if let Some(cursor) = &after {
                params.push(("after", cursor.clone()));
            }
            let resp = self
                .http_client()
                .get(&url)
                .header("Client-Id", &self.helix_config().client_id)
                .header("Authorization", format!("Bearer {user_token}"))
                .query(&params)
                .send()
                .await?;

            let status = resp.status().as_u16();
            if status == 403 {
                return Err(HelixError::NotModerator);
            }
            if status != 200 {
                return Err(HelixError::Status { status });
            }

            let page: ChattersPage = resp.json().await.map_err(HelixError::Http)?;
            let empty = page.data.is_empty();
            out.extend(page.data);
            after = page.pagination.cursor;
            if after.is_none() || empty {
                break;
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen (intern)
// ---------------------------------------------------------------------------

/// Parst einen Helix-`created_at`-String zu [`DateTime<Utc>`].
///
/// Helix liefert RFC-3339 z. B. `"2024-01-15T12:00:00Z"`.
pub fn parse_created_at(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ---------------------------------------------------------------------------
// Tests (Wiremock)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{HelixClient, HelixConfig};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Baut einen HelixClient gegen einen MockServer (ohne App-Token-Präfetch).
    async fn mock_client(server: &MockServer) -> HelixClient {
        // App-Token-Endpunkt — nur falls intern aufgerufen.
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "app-tok", "expires_in": 3600
            })))
            .mount(server)
            .await;
        let mut cfg = HelixConfig::new("cid", "sec");
        cfg.helix_base = format!("{}/helix", server.uri());
        cfg.token_url = format!("{}/oauth2/token", server.uri());
        HelixClient::new(cfg).unwrap()
    }

    // -----------------------------------------------------------------------
    // send_chat_message
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn send_chat_200_is_sent_true() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(header("Authorization", "Bearer bot-tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"message_id": "abc", "is_sent": true}]
            })))
            .mount(&server)
            .await;
        let result = client
            .send_chat_message("111", "bot1", "Hallo!", "bot-tok")
            .await
            .unwrap();
        assert_eq!(result, SendOutcome::Sent);
    }

    #[tokio::test]
    async fn send_chat_200_is_sent_false_ergibt_dropped() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "message_id": "abc",
                    "is_sent": false,
                    "drop_reason": {"code": "channel_settings", "message": "Blocked by channel settings"}
                }]
            })))
            .mount(&server)
            .await;
        let result = client
            .send_chat_message("111", "bot1", "msg", "bot-tok")
            .await
            .unwrap();
        assert!(
            matches!(result, SendOutcome::Dropped { ref code, .. } if code == "channel_settings"),
            "erwartet Dropped(channel_settings), bekam {result:?}"
        );
    }

    #[tokio::test]
    async fn send_chat_401_ergibt_http_error() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;
        let result = client
            .send_chat_message("111", "bot1", "msg", "bad-tok")
            .await
            .unwrap();
        assert!(
            matches!(result, SendOutcome::HttpError { status: 401, .. }),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn send_chat_dropped_sender_banned() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "message_id": "xyz",
                    "is_sent": false,
                    "drop_reason": {"code": "sender_banned", "message": "Sender is banned"}
                }]
            })))
            .mount(&server)
            .await;
        let result = client
            .send_chat_message("111", "bot1", "msg", "tok")
            .await
            .unwrap();
        assert!(
            matches!(result, SendOutcome::Dropped { ref code, .. } if code == "sender_banned"),
            "{result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // send_announcement
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn announcement_200_ergibt_true() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/announcements"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("moderator_id", "bot1"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        assert!(
            client
                .send_announcement("111", "bot1", "Ankündigung", "purple", "tok")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn announcement_204_ergibt_true() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/announcements"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(
            client
                .send_announcement("111", "bot1", "msg", "blue", "tok")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn announcement_401_ergibt_false() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/announcements"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert!(
            !client
                .send_announcement("111", "bot1", "msg", "purple", "tok")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn announcement_detail_enthaelt_status_und_body() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/announcements"))
            .respond_with(ResponseTemplate::new(403).set_body_string("missing scope"))
            .mount(&server)
            .await;
        let outcome = client
            .send_announcement_detailed("111", "bot1", "msg", "purple", "tok")
            .await
            .unwrap();
        assert!(!outcome.accepted);
        assert_eq!(outcome.status, Some(403));
        assert_eq!(outcome.detail.as_deref(), Some("missing scope"));
    }

    #[tokio::test]
    async fn announcement_detail_redigiert_tokenartige_werte() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        // JWT-förmige Fixture zur Laufzeit zusammengesetzt, damit kein
        // zusammenhängendes Token-Literal im Quelltext steht (Secret-Scanner).
        let seg = "eyJ";
        let jwt = format!(
            "{seg}hbGciOiJIUzI1NiJ9.{seg}zdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
        let body = format!(
            r#"{{"error":"bad","access_token":"secret-access-12345","refresh_token":"secret-refresh-12345","id_token":"{jwt}","client_secret":"secret-client-12345","authorization":"Bearer secret-bearer-12345"}}"#
        );
        Mock::given(method("POST"))
            .and(path("/helix/chat/announcements"))
            .respond_with(ResponseTemplate::new(401).set_body_string(body))
            .mount(&server)
            .await;

        let outcome = client
            .send_announcement_detailed("111", "bot1", "msg", "purple", "tok")
            .await
            .unwrap();

        let detail = outcome.detail.unwrap();
        assert_eq!(outcome.status, Some(401));
        assert!(detail.contains("access_token"));
        assert!(detail.contains("[redacted:"));
        assert!(!detail.contains("secret-access-12345"), "detail={detail}");
        assert!(!detail.contains("secret-refresh-12345"), "detail={detail}");
        assert!(!detail.contains("secret-client-12345"), "detail={detail}");
        assert!(!detail.contains("secret-bearer-12345"), "detail={detail}");
        assert!(!detail.contains(jwt.as_str()), "detail={detail}");
    }

    #[tokio::test]
    async fn whisper_204_ergibt_accepted() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/whispers"))
            .and(query_param("from_user_id", "bot1"))
            .and(query_param("to_user_id", "raider1"))
            .and(header("Authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let outcome = client
            .send_whisper("bot1", "raider1", "Bitte kurz Hallo sagen.", "tok")
            .await
            .unwrap();

        assert!(outcome.accepted);
        assert_eq!(outcome.status, None);
    }

    #[tokio::test]
    async fn whisper_403_liefert_status_und_body() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/whispers"))
            .respond_with(ResponseTemplate::new(403).set_body_string("missing scope"))
            .mount(&server)
            .await;

        let outcome = client
            .send_whisper("bot1", "raider1", "msg", "tok")
            .await
            .unwrap();

        assert!(!outcome.accepted);
        assert_eq!(outcome.status, Some(403));
        assert_eq!(outcome.detail.as_deref(), Some("missing scope"));
    }

    // -----------------------------------------------------------------------
    // ban_user
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn ban_user_200_ergibt_banned() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/moderation/bans"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("moderator_id", "bot1"))
            .and(header("Authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":[]})))
            .mount(&server)
            .await;
        let result = client
            .ban_user("111", "bot1", "user99", "Spam", None, "tok")
            .await
            .unwrap();
        assert_eq!(result, BanOutcome::Banned);
    }

    #[tokio::test]
    async fn ban_user_400_already_banned() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/moderation/bans"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string(r#"{"message":"user is already banned"}"#),
            )
            .mount(&server)
            .await;
        let result = client
            .ban_user("111", "bot1", "user99", "Spam", None, "tok")
            .await
            .unwrap();
        assert_eq!(result, BanOutcome::AlreadyBanned);
    }

    #[tokio::test]
    async fn ban_user_403_ergibt_forbidden() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/moderation/bans"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;
        let result = client
            .ban_user("111", "bot1", "user99", "Spam", None, "tok")
            .await
            .unwrap();
        assert_eq!(result, BanOutcome::Forbidden);
    }

    // -----------------------------------------------------------------------
    // unban_user
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn unban_user_204_ergibt_unbanned() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("DELETE"))
            .and(path("/helix/moderation/bans"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("moderator_id", "bot1"))
            .and(query_param("user_id", "user99"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let result = client
            .unban_user("111", "bot1", "user99", "tok")
            .await
            .unwrap();
        assert_eq!(result, BanOutcome::Unbanned);
    }

    #[tokio::test]
    async fn unban_user_403_ergibt_forbidden() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("DELETE"))
            .and(path("/helix/moderation/bans"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let result = client
            .unban_user("111", "bot1", "user99", "tok")
            .await
            .unwrap();
        assert_eq!(result, BanOutcome::Forbidden);
    }

    // -----------------------------------------------------------------------
    // delete_chat_message
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn delete_message_204_ok() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("DELETE"))
            .and(path("/helix/moderation/chat"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("moderator_id", "bot1"))
            .and(query_param("message_id", "msg-abc"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(
            client
                .delete_chat_message("111", "bot1", "msg-abc", "tok")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn delete_message_401_ergibt_false() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("DELETE"))
            .and(path("/helix/moderation/chat"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert!(
            !client
                .delete_chat_message("111", "bot1", "msg-abc", "bad-tok")
                .await
                .unwrap()
        );
    }

    // -----------------------------------------------------------------------
    // get_users_created_at
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_users_created_at_liefert_user_infos() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "123",
                    "login": "testuser",
                    "display_name": "TestUser",
                    "created_at": "2024-01-15T12:00:00Z"
                }]
            })))
            .mount(&server)
            .await;
        let users = client
            .get_users_created_at(&["123"], "tok")
            .await
            .unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].login, "testuser");
        let dt = parse_created_at(&users[0].created_at);
        assert!(dt.is_some(), "created_at muss parsbar sein");
    }

    #[tokio::test]
    async fn get_users_created_at_leer_gibt_leer_zurueck() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        let users = client.get_users_created_at(&[], "tok").await.unwrap();
        assert!(users.is_empty());
    }

    // -----------------------------------------------------------------------
    // get_user_by_login
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_user_by_login_findet_user() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(query_param("login", "nani"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "456",
                    "login": "nani",
                    "display_name": "Nani",
                    "created_at": "2022-03-01T00:00:00Z"
                }]
            })))
            .mount(&server)
            .await;
        let user = client.get_user_by_login("nani", "tok").await.unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().id, "456");
    }

    #[tokio::test]
    async fn get_user_by_login_nicht_gefunden_gibt_none() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": []})),
            )
            .mount(&server)
            .await;
        let user = client.get_user_by_login("nobody", "tok").await.unwrap();
        assert!(user.is_none());
    }

    // -----------------------------------------------------------------------
    // get_chatters
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_chatters_folgt_cursor_ueber_zwei_seiten() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        // Seite 1: Cursor gesetzt → Poller fragt nach.
        Mock::given(method("GET"))
            .and(path("/helix/chat/chatters"))
            .and(query_param("broadcaster_id", "111"))
            .and(query_param("moderator_id", "111"))
            .and(header("Authorization", "Bearer bot-tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"user_id": "1", "user_login": "alice", "user_name": "Alice"},
                    {"user_id": "2", "user_login": "bob", "user_name": "Bob"}
                ],
                "pagination": {"cursor": "page2"}
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        // Seite 2: per Cursor; kein weiterer Cursor → Ende.
        Mock::given(method("GET"))
            .and(path("/helix/chat/chatters"))
            .and(query_param("after", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"user_id": "3", "user_login": "carol", "user_name": "Carol"}
                ],
                "pagination": {}
            })))
            .mount(&server)
            .await;

        let chatters = client.get_chatters("111", "111", "bot-tok").await.unwrap();
        assert_eq!(chatters.len(), 3, "beide Seiten zusammengeführt");
        assert_eq!(chatters[0].user_login, "alice");
        assert_eq!(chatters[2].user_login, "carol");
    }

    #[tokio::test]
    async fn get_chatters_403_ergibt_not_moderator() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/chat/chatters"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;
        let err = client
            .get_chatters("111", "999", "bot-tok")
            .await
            .unwrap_err();
        assert!(
            matches!(err, HelixError::NotModerator),
            "403 muss NotModerator sein, war {err:?}"
        );
    }

    #[tokio::test]
    async fn get_chatters_500_ergibt_status_fehler() {
        let server = MockServer::start().await;
        let client = mock_client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/chat/chatters"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let err = client
            .get_chatters("111", "111", "bot-tok")
            .await
            .unwrap_err();
        assert!(matches!(err, HelixError::Status { status: 500 }), "{err:?}");
    }
}
