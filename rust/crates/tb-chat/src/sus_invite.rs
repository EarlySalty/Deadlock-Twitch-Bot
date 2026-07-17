//! Verdächtige Discord-Invite-Erkennung im Twitch-Chat.
//!
//! Port von `_check_sus_discord_invite` (bot/chat/moderation.py Z. 843–899).
//!
//! # Vertrag
//!
//! 1. Regex `discord\.gg/[A-Za-z0-9]+` (moderation.py Z. 776, re.IGNORECASE).
//! 2. Moderatoren und Broadcaster werden NICHT geflaggt (moderation.py Z. 858–859).
//! 3. Etablierte Chatter werden übersprungen (moderation.py Z. 779–813, Z. 862–863).
//!    Schwellen: sessions `>= 3` OR messages `>= 40` OR first_seen_at `>= 14 Tage` alt.
//!    Python-Kommentar nennt 20 Nachrichten, der Code prüft 40 — Code gewinnt.
//! 4. Cooldown: 300 Sekunden pro (channel, chatter) (moderation.py Z. 871).
//! 5. Aktion: KEIN Ban / Delete. Dieses Modul trifft nur die ENTSCHEIDUNG und
//!    gibt einen [`SusInviteHit`] zurück — die Pipeline schreibt daraufhin die
//!    Review-Log-Zeile (status="SUSPICIOUS_DISCORD_INVITE", reason="discord.gg
//!    link in partner chat") und feuert den Discord-Alert (kind="sus_invite"),
//!    exakt wie Python (moderation.py Z. 882–899).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use regex::Regex;
use sqlx::PgPool;
use tracing::warn;

use crate::types::ChatMessageEvent;

// ---------------------------------------------------------------------------
// Konstanten — exakt aus moderation.py Z. 776 / Z. 871
// ---------------------------------------------------------------------------

/// Regex für Discord-Invite-Links (moderation.py Z. 776, re.IGNORECASE).
fn discord_invite_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)discord\.gg/[A-Za-z0-9]+").expect("DISCORD_INVITE_RE ist konstant")
    })
}

/// Cooldown pro (channel, chatter) in Sekunden (moderation.py Z. 871: `< 300.0`).
const SUS_INVITE_COOLDOWN_SECS: u64 = 300;

/// Schwelle Sessions: >= 3 gilt als etablierter Chatter (moderation.py Z. 803).
const ESTABLISHED_SESSIONS: i64 = 3;

/// Schwelle Nachrichten: >= 40 gilt als etablierter Chatter (moderation.py Z. 803, Code-Realität).
const ESTABLISHED_MESSAGES: i64 = 40;

/// Schwelle Alter first_seen_at: >= 14 Tage (moderation.py Z. 808–809).
const ESTABLISHED_DAYS: i64 = 14;

// ---------------------------------------------------------------------------
// SusInviteCheck
// ---------------------------------------------------------------------------

/// Bestätigter Verdachts-Treffer — die Pipeline schreibt damit Review-Log +
/// Discord-Alert (moderation.py Z. 882–899).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SusInviteHit {
    pub chatter_login: String,
    pub chatter_id: String,
    pub content: String,
}

/// Prüft Nachrichten auf verdächtige Discord-Invite-Links.
///
/// # Verwendung
///
/// ```rust,ignore
/// let check = SusInviteCheck::new(pool.clone());
/// if let Some(hit) = check.check(&event, "streamer_login").await { /* loggen+alerten */ }
/// ```
pub struct SusInviteCheck {
    pool: PgPool,
    /// Cooldown-Map: (channel_login, chatter_login) → letzter Aufruf-Zeitpunkt.
    /// Entspricht `_sus_invite_cooldown` (moderation.py Z. 865–867).
    cooldowns: Mutex<HashMap<(String, String), Instant>>,
}

impl SusInviteCheck {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cooldowns: Mutex::new(HashMap::new()),
        }
    }

    /// Prüft eine Nachricht auf verdächtige Discord-Invite-Links.
    ///
    /// Port von `_check_sus_discord_invite` (bot/chat/moderation.py Z. 843–899).
    /// `None` bei allen Nicht-Treffer-Fällen; `Some(hit)` wenn die Pipeline
    /// Review-Log + Alert auslösen soll.
    pub async fn check(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
    ) -> Option<SusInviteHit> {
        let content = event.text();
        if !discord_invite_re().is_match(content) {
            return None;
        }

        let chatter_login = event.chatter_user_login.to_lowercase();
        if chatter_login.is_empty() {
            return None;
        }

        // Moderatoren und Broadcaster überspringen (moderation.py Z. 858–859)
        if event.is_mod_or_broadcaster() {
            return None;
        }

        // Etablierte Chatter überspringen (moderation.py Z. 862–863)
        if self
            .is_established_chatter(channel_login, &chatter_login)
            .await
        {
            return None;
        }

        // Cooldown-Check (moderation.py Z. 865–873)
        if !self.cooldown_ok(channel_login, &chatter_login) {
            return None;
        }

        warn!(
            "Sus Discord-Invite in #{} von {}: {}",
            channel_login,
            chatter_login,
            // Zeichen-basiert kürzen (Python content[:200]); Byte-Slice könnte an
            // einer Multibyte-Grenze panischen.
            content.chars().take(200).collect::<String>()
        );

        Some(SusInviteHit {
            chatter_login,
            chatter_id: event.chatter_user_id.clone(),
            content: content.to_string(),
        })
    }

    /// Prüft ob ein Chatter als etabliert gilt.
    ///
    /// Port von `_is_established_chatter` (moderation.py Z. 779–813).
    /// Schwellen: sessions >= 3 OR messages >= 40 OR first_seen_at < now - 14d.
    /// Fail-safe: bei DB-Fehler false (moderation.py Z. 811–812: `except: pass; return False`).
    async fn is_established_chatter(&self, channel_login: &str, chatter_login: &str) -> bool {
        #[derive(sqlx::FromRow)]
        struct RollupRow {
            total_sessions: Option<i32>,
            total_messages: Option<i32>,
            first_seen_at: Option<chrono::DateTime<chrono::Utc>>,
        }

        let row = sqlx::query_as!(
            RollupRow,
            "SELECT total_sessions AS \"total_sessions?\", \
                    total_messages AS \"total_messages?\", \
                    first_seen_at AS \"first_seen_at?\" \
             FROM twitch_chatter_rollup \
             WHERE streamer_login = $1 AND chatter_login = $2 \
             LIMIT 1",
            channel_login,
            chatter_login,
        )
        .fetch_optional(&self.pool)
        .await;

        let row = match row {
            Ok(Some(r)) => r,
            Ok(None) => return false,
            Err(e) => {
                tracing::debug!(
                    "is_established_chatter DB-Fehler (channel={channel_login}, chatter={chatter_login}): {e}"
                );
                return false;
            }
        };

        let sessions = row.total_sessions.unwrap_or(0) as i64;
        let messages = row.total_messages.unwrap_or(0) as i64;

        if sessions >= ESTABLISHED_SESSIONS || messages >= ESTABLISHED_MESSAGES {
            return true;
        }

        if let Some(first_seen) = row.first_seen_at {
            let age_days = (chrono::Utc::now() - first_seen).num_days();
            if age_days >= ESTABLISHED_DAYS {
                return true;
            }
        }

        false
    }

    /// Cooldown-Check + Aktualisierung.
    fn cooldown_ok(&self, channel_login: &str, chatter_login: &str) -> bool {
        let mut map = self.cooldowns.lock().expect("SusInviteCheck cooldown lock");
        let key = (channel_login.to_string(), chatter_login.to_string());
        let now = Instant::now();
        if let Some(&last) = map.get(&key) {
            if now.duration_since(last).as_secs() < SUS_INVITE_COOLDOWN_SECS {
                return false;
            }
        }
        map.insert(key, now);
        true
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests (keine DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatBadge, ChatMessageBody};

    fn make_event(text: &str, is_mod: bool) -> ChatMessageEvent {
        let mut badges = vec![];
        if is_mod {
            badges.push(ChatBadge {
                set_id: "moderator".to_string(),
                id: String::new(),
                info: String::new(),
            });
        }
        ChatMessageEvent {
            broadcaster_user_id: "ch1".to_string(),
            broadcaster_user_login: "streamer1".to_string(),
            broadcaster_user_name: "Streamer1".to_string(),
            chatter_user_id: "u1".to_string(),
            chatter_user_login: "user1".to_string(),
            chatter_user_name: "User1".to_string(),
            message_id: "m1".to_string(),
            message: ChatMessageBody {
                text: text.to_string(),
                fragments: vec![],
            },
            badges,
            color: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn discord_invite_regex_matcht() {
        assert!(discord_invite_re().is_match("komm auf discord.gg/abc123 rüber"));
        assert!(discord_invite_re().is_match("discord.gg/XYZ"));
        // Groß-/Kleinschreibung egal (re.IGNORECASE)
        assert!(discord_invite_re().is_match("DISCORD.GG/abc"));
    }

    #[test]
    fn discord_invite_regex_kein_match_allgemein() {
        // Allgemeines Reden über Discord ohne Invite-Link
        assert!(!discord_invite_re().is_match("ich spiele auf discord"));
        assert!(!discord_invite_re().is_match("discord ist cool"));
    }

    #[test]
    fn discord_invite_regex_keine_tld_ohne_slash() {
        // Ohne Slash-Code kein Match
        assert!(!discord_invite_re().is_match("discord.gg"));
    }

    #[test]
    fn mod_wird_nicht_geflaggt() {
        let event = make_event("komm auf discord.gg/test123", true);
        // is_mod_or_broadcaster gibt true → würde early-return auslösen
        assert!(event.is_mod_or_broadcaster());
    }

    #[test]
    fn normaler_user_wird_geprüft() {
        let event = make_event("komm auf discord.gg/test123", false);
        assert!(!event.is_mod_or_broadcaster());
    }

    #[test]
    fn established_schwellen_konstanten() {
        // Werte exakt aus moderation.py Z. 803, 808
        assert_eq!(ESTABLISHED_SESSIONS, 3);
        assert_eq!(ESTABLISHED_MESSAGES, 40);
        assert_eq!(ESTABLISHED_DAYS, 14);
    }

    #[test]
    fn cooldown_konstante() {
        assert_eq!(SUS_INVITE_COOLDOWN_SECS, 300);
    }
}

// ---------------------------------------------------------------------------
// DB-Tests (nur wenn TB_TEST_DATABASE_URL gesetzt)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use std::str::FromStr;

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::PgPool;

    use super::*;
    use crate::types::ChatMessageBody;

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap()
    }

    async fn create_tables(pool: &PgPool) {
        // twitch_chatter_rollup — prod-Schema für established-Chatter-Test
        sqlx::query(
            "CREATE TABLE twitch_chatter_rollup (
                streamer_login TEXT NOT NULL,
                chatter_login TEXT NOT NULL,
                chatter_id TEXT NOT NULL DEFAULT '',
                first_seen_at TIMESTAMPTZ,
                last_seen_at TIMESTAMPTZ,
                total_messages INTEGER NOT NULL DEFAULT 0,
                total_sessions INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (streamer_login, chatter_login)
            )",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    fn make_event_db(text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "ch1".to_string(),
            broadcaster_user_login: "streamer1".to_string(),
            broadcaster_user_name: "Streamer1".to_string(),
            chatter_user_id: "u42".to_string(),
            chatter_user_login: "spammer".to_string(),
            chatter_user_name: "Spammer".to_string(),
            message_id: "m1".to_string(),
            message: ChatMessageBody {
                text: text.to_string(),
                fragments: vec![],
            },
            badges: vec![],
            color: String::new(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn invite_link_liefert_hit() {
        let pool = pool_or_skip!("si_log_test");
        create_tables(&pool).await;

        let check = SusInviteCheck::new(pool.clone());
        let hit = check
            .check(&make_event_db("discord.gg/abc123"), "streamer1")
            .await;

        let hit = hit.expect("Invite-Link muss Hit liefern");
        assert_eq!(hit.chatter_login, "spammer");
        assert_eq!(hit.chatter_id, "u42");
        assert_eq!(hit.content, "discord.gg/abc123");
    }

    #[tokio::test]
    async fn etablierter_chatter_kein_hit() {
        let pool = pool_or_skip!("si_established");
        create_tables(&pool).await;

        // Chatter mit >= 3 Sessions einfügen → gilt als etabliert
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup \
             (streamer_login, chatter_login, total_sessions, total_messages) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind("streamer1")
        .bind("spammer")
        .bind(5i32) // >= 3 → etabliert
        .bind(10i32)
        .execute(&pool)
        .await
        .unwrap();

        let check = SusInviteCheck::new(pool.clone());
        let hit = check
            .check(&make_event_db("discord.gg/abc123"), "streamer1")
            .await;
        assert!(hit.is_none(), "Etablierter Chatter darf keinen Hit liefern");
    }

    #[tokio::test]
    async fn cooldown_verhindert_doppel_hit() {
        let pool = pool_or_skip!("si_cooldown");
        create_tables(&pool).await;

        let check = SusInviteCheck::new(pool.clone());
        let first = check
            .check(&make_event_db("discord.gg/abc123"), "streamer1")
            .await;
        let second = check
            .check(&make_event_db("discord.gg/xyz456"), "streamer1")
            .await;

        assert!(first.is_some());
        assert!(second.is_none(), "Cooldown muss zweiten Hit verhindern");
    }

    #[tokio::test]
    async fn kein_invite_link_kein_hit() {
        let pool = pool_or_skip!("si_no_link");
        create_tables(&pool).await;

        let check = SusInviteCheck::new(pool.clone());
        let hit = check
            .check(&make_event_db("ich spiele auf discord"), "streamer1")
            .await;
        assert!(hit.is_none());
    }

    #[tokio::test]
    async fn chatter_mit_vielen_nachrichten_etabliert() {
        let pool = pool_or_skip!("si_msgs_established");
        create_tables(&pool).await;

        // 40 Nachrichten = genau die Schwelle (messages >= 40)
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup \
             (streamer_login, chatter_login, total_sessions, total_messages) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind("streamer1")
        .bind("spammer")
        .bind(1i32)
        .bind(40i32) // == 40 → etabliert
        .execute(&pool)
        .await
        .unwrap();

        let check = SusInviteCheck::new(pool.clone());
        let hit = check
            .check(&make_event_db("discord.gg/abc"), "streamer1")
            .await;
        assert!(hit.is_none(), "40 Nachrichten = etabliert → kein Hit");
    }
}
