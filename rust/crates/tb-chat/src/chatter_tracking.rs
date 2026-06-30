//! Chatter-Persistenz pro Session — Port von `_track_chat_health` (moderation.py Z. 2098–2361).
//!
//! Schreibt bei jeder eingehenden Chat-Nachricht (in EINER Transaktion):
//! - `twitch_raw_chat_ingest_health` — Heartbeat-Upsert (message_at bei jedem
//!   partner-getrackten Event, insert_ok_at/insert_error_at je nach Ausgang;
//!   Spalten sind TEXT, Format ISO-Sekunden wie Python `isoformat(timespec="seconds")`)
//! - `twitch_chat_messages` — Roh-Nachricht inkl. Klartext (moderation.py Z. 2214–2238)
//! - `twitch_session_chatters` — Upsert (first_message_at, messages++, last_seen_at,
//!   is_first_time_streamer, confirmed_first_ever BLEIBT NULL/unverändert)
//! - `twitch_chatter_rollup` — Upsert (total_messages++, total_sessions++ wenn neue Session)
//!
//! Gate-Reihenfolge:
//! 1. Channel-Login vorhanden
//! 2. Chatter-Login vorhanden (sonst: nur Health-Heartbeat)
//! 3. Known-Bot-Filter (sonst: nur Health-Heartbeat)
//! 4. Session-Resolver (`twitch_stream_sessions` offene Session, 60s-Cache)
//! 5. Target-Game-Gate (`twitch_live_state`, Fallback offene Session) —
//!    standardmäßig übersprungen (Chat aller Spiele wird gespeichert), siehe
//!    `TB_CHAT_PERSIST_ALL_GAMES`.
//!
//! Kein Partner-Gate: Daten werden für alle Kanäle gesammelt, in denen der Bot
//! aktiv ist. Die Trennung Partner/Nicht-Partner erfolgt beim Abfragen, nicht
//! schon beim Schreiben.
//!
//! Der Raw-Aktivitäts-Zähler für Promos (`_record_raw_chat_message`,
//! moderation.py Z. 2173) lebt in [`crate::promos::PromoEngine::record_raw_message`]
//! und wird von der Pipeline an derselben Stelle aufgerufen.
//!
//! Fehler werden geloggt und nicht propagiert (der Python-Aufrufer wrappt in
//! try/except — Rust: warn-Log + Health-Error-Write).

use crate::types::ChatMessageEvent;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use sqlx::PgPool;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Konstanten
// ---------------------------------------------------------------------------

/// Ziel-Spielname für den Target-Game-Gate-Check (constants.py Z. 21
/// `TWITCH_TARGET_GAME_NAME = "Deadlock"`, lowercased via bot.py Z. 170).
const TARGET_GAME: &str = "deadlock";

/// Known-Chat-Bots (chat_bots.py Z. 8–19 `KNOWN_CHAT_BOTS`).
const KNOWN_CHAT_BOTS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

/// Session-Cache-TTL (bot.py Z. 2174: 60 Sekunden).
const SESSION_CACHE_TTL_SECS: i64 = 60;

/// Fehlertext-Limit für `last_raw_chat_error` (moderation.py Z. 159: 300).
const RAW_CHAT_ERROR_LIMIT: usize = 300;

// ---------------------------------------------------------------------------
// Öffentliche API
// ---------------------------------------------------------------------------

/// Tracked eine eingehende Chat-Nachricht in `twitch_chat_messages`,
/// `twitch_session_chatters`, `twitch_chatter_rollup` und pflegt den
/// `twitch_raw_chat_ingest_health`-Heartbeat. Fehler werden intern geloggt.
pub struct ChatterTracker {
    pool: PgPool,
    /// login → (session_id oder None, Unix-Sekunden des Cache-Eintrags).
    /// Python cached auch None-Ergebnisse (bot.py Z. 2170–2176).
    session_cache: DashMap<String, (Option<i64>, i64)>,
    /// Wenn `true` (Default), wird das Target-Game-Gate (Gate 6) übersprungen
    /// und Chat aller Spiele persistiert — nötig, damit Scam-Accounts auch
    /// außerhalb von Deadlock-Sessions erfasst werden. Abschaltbar über
    /// `TB_CHAT_PERSIST_ALL_GAMES=0` (zurück zum Deadlock-only-Verhalten).
    persist_all_games: bool,
}

impl ChatterTracker {
    pub fn new(pool: PgPool) -> Self {
        Self::with_persist_all_games(pool, persist_all_games_from_env())
    }

    /// Wie [`Self::new`], aber mit explizitem Game-Gate-Flag (Tests/Konfig).
    pub fn with_persist_all_games(pool: PgPool, persist_all_games: bool) -> Self {
        Self {
            pool,
            session_cache: DashMap::new(),
            persist_all_games,
        }
    }

    /// Haupteintrittspunkt — entspricht `_track_chat_health` (moderation.py Z. 2098).
    /// Fehler-isoliert: loggt und kehrt bei jedem Fehler zurück.
    pub async fn track(&self, event: &ChatMessageEvent) {
        let login = event.broadcaster_user_login.to_lowercase();
        if login.is_empty() {
            debug!("chatter_tracking: fehlender channel-login — skip");
            return;
        }

        let now = Utc::now();
        let ts_iso = iso_seconds(&now);
        let chatter_login = event.chatter_user_login.to_lowercase();

        // Gate 3: Chatter-Login vorhanden (Python Z. 2144–2155: Health-Heartbeat trotzdem).
        if chatter_login.is_empty() {
            self.persist_health(&login, Some(&ts_iso), None, None, None).await;
            return;
        }

        // Gate 4: Known-Bot-Filter (Python Z. 2156–2166: Health-Heartbeat trotzdem).
        if KNOWN_CHAT_BOTS.contains(&chatter_login.as_str()) {
            self.persist_health(&login, Some(&ts_iso), None, None, None).await;
            return;
        }

        // Gate 5: Session-Resolver (bot.py Z. 2168: offene Session, 60s-Cache).
        let session_id = match self.resolve_session_id(&login).await {
            Ok(id) => id,
            Err(e) => {
                warn!(channel = %login, "chatter_tracking: session-resolve DB-Fehler — {e}");
                return;
            }
        };
        let Some(session_id) = session_id else {
            self.persist_health(&login, Some(&ts_iso), None, None, None).await;
            debug!(channel = %login, "chatter_tracking: keine offene Session — skip");
            return;
        };

        // Gate 6: Target-Game-Gate (moderation.py Z. 2191 + 2008–2080).
        // Standardmäßig übersprungen (persist_all_games=true): Chat wird für jedes
        // Spiel gespeichert, damit Scam-Accounts auch außerhalb von Deadlock erfasst
        // werden. Mit TB_CHAT_PERSIST_ALL_GAMES=0 gilt wieder Deadlock-only.
        if !self.persist_all_games {
            match self.is_target_game_live(&login, session_id).await {
                Ok(true) => {}
                Ok(false) => {
                    self.persist_health(&login, Some(&ts_iso), None, None, None).await;
                    debug!(channel = %login, "chatter_tracking: target-game gate blocked — skip");
                    return;
                }
                Err(e) => {
                    warn!(channel = %login, "chatter_tracking: game-gate DB-Fehler — {e}");
                    return;
                }
            }
        }

        // --- Schreiben (eine Transaktion, Python Z. 2206–2343) ---
        if let Err(e) = self
            .write_tracked_message(event, &login, &chatter_login, session_id, now, &ts_iso)
            .await
        {
            let error_text = truncate_raw_chat_error(&e.to_string());
            self.persist_health(
                &login,
                Some(&ts_iso),
                None,
                Some(&ts_iso),
                Some(error_text.as_deref()),
            )
            .await;
            warn!(
                channel = %login,
                chatter = %chatter_login,
                "chatter_tracking: insert fehlgeschlagen — {e}"
            );
        }
    }

    /// Offene Session via `twitch_stream_sessions` (bot.py Z. 2168–2190).
    /// Cached auch None-Ergebnisse für 60s — exakt wie Python.
    async fn resolve_session_id(&self, login: &str) -> Result<Option<i64>, sqlx::Error> {
        let now_secs = Utc::now().timestamp();
        if let Some(entry) = self.session_cache.get(login) {
            let (cached_id, cached_at) = *entry;
            if now_secs - cached_at < SESSION_CACHE_TTL_SECS {
                return Ok(cached_id);
            }
        }

        let session_id: Option<i64> = sqlx::query_scalar!(
            "SELECT id::bigint AS \"id!\" FROM twitch_stream_sessions \
             WHERE streamer_login = $1 AND ended_at IS NULL \
             ORDER BY started_at DESC \
             LIMIT 1",
            login,
        )
        .fetch_optional(&self.pool)
        .await?;

        self.session_cache
            .insert(login.to_string(), (session_id, now_secs));
        Ok(session_id)
    }

    /// Target-Game-Gate (`_is_target_game_live_for_chat`, moderation.py Z. 2008–2080):
    /// `twitch_live_state` zuerst; wenn keine Zeile existiert, Fallback auf die
    /// offene Session (`game_name`).
    async fn is_target_game_live(
        &self,
        login: &str,
        session_id: i64,
    ) -> Result<bool, sqlx::Error> {
        let live_row = sqlx::query!(
            "SELECT COALESCE(is_live, 0) AS \"is_live!\", last_game \
             FROM twitch_live_state \
             WHERE streamer_login = $1",
            login,
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = live_row {
            let game = row.last_game.unwrap_or_default();
            return Ok(row.is_live != 0 && game.trim().to_lowercase() == TARGET_GAME);
        }

        // Fallback: game_name der offenen Session (moderation.py Z. 2051–2073).
        let game_name: Option<String> = sqlx::query_scalar!(
            "SELECT game_name FROM twitch_stream_sessions \
             WHERE id = $1 AND ended_at IS NULL",
            session_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        Ok(game_name
            .map(|g| g.trim().to_lowercase() == TARGET_GAME)
            .unwrap_or(false))
    }

    /// Erfolgs-Transaktion: Health-Heartbeat + Roh-Nachricht + Rollup +
    /// Session-Chatters + Health-OK (moderation.py Z. 2206–2343).
    async fn write_tracked_message(
        &self,
        event: &ChatMessageEvent,
        login: &str,
        chatter_login: &str,
        session_id: i64,
        now: DateTime<Utc>,
        ts_iso: &str,
    ) -> Result<(), sqlx::Error> {
        let chatter_id: Option<&str> = if event.chatter_user_id.is_empty() {
            None
        } else {
            Some(&event.chatter_user_id)
        };
        let message_id: Option<&str> = if event.message_id.is_empty() {
            None
        } else {
            Some(&event.message_id)
        };
        // NUL-Bytes entfernen + Command-Flag (Python Z. 2168–2171, Prefix "!").
        let content = event.message.text.replace('\u{0}', "");
        let is_command = content.trim_start().starts_with('!');

        let mut tx = self.pool.begin().await?;

        // 1. Health-Heartbeat: message_at (Python Z. 2208–2212).
        upsert_ingest_health(&mut tx, login, Some(ts_iso), None, None, None).await?;

        // 2. Roh-Nachricht (Python Z. 2214–2238). message_ts = timestamptz.
        sqlx::query!(
            "INSERT INTO twitch_chat_messages \
             (session_id, streamer_login, chatter_login, chatter_id, message_id, \
              message_ts, is_command, content) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            session_id,
            login,
            chatter_login,
            chatter_id,
            message_id,
            now,
            is_command,
            &content,
        )
        .execute(&mut *tx)
        .await?;

        // 3. Rollup + Session-Chatters lesen (Python Z. 2241–2257).
        let existing = sqlx::query!(
            "SELECT COALESCE(messages, 0) AS \"messages!\", \
                    is_first_time_streamer, \
                    COALESCE(seen_via_chatters_api, FALSE) AS \"seen_via_chatters_api!\" \
             FROM twitch_session_chatters \
             WHERE session_id = $1 AND chatter_login = $2",
            session_id,
            chatter_login,
        )
        .fetch_optional(&mut *tx)
        .await?;

        let rollup = sqlx::query!(
            "SELECT COALESCE(total_messages, 0) AS \"total_messages!\", \
                    COALESCE(total_sessions, 0) AS \"total_sessions!\" \
             FROM twitch_chatter_rollup \
             WHERE streamer_login = $1 AND chatter_login = $2",
            login,
            chatter_login,
        )
        .fetch_optional(&mut *tx)
        .await?;

        // is_first_global: kein Rollup-Eintrag = Erstkontakt (Python Z. 2259).
        let is_first_global = rollup.is_none();

        // 4. Rollup schreiben (Python Z. 2260–2282).
        match rollup {
            Some(_) => {
                let sessions_inc: i32 = if existing.is_none() { 1 } else { 0 };
                sqlx::query!(
                    "UPDATE twitch_chatter_rollup \
                     SET total_messages = total_messages + 1, \
                         total_sessions = total_sessions + $1, \
                         last_seen_at   = $2, \
                         chatter_id     = COALESCE(chatter_id, $3) \
                     WHERE streamer_login = $4 AND chatter_login = $5",
                    sessions_inc,
                    now,
                    chatter_id,
                    login,
                    chatter_login,
                )
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query!(
                    "INSERT INTO twitch_chatter_rollup \
                     (streamer_login, chatter_login, chatter_id, first_seen_at, last_seen_at, \
                      total_messages, total_sessions) \
                     VALUES ($1, $2, $3, $4, $5, 1, 1)",
                    login,
                    chatter_login,
                    chatter_id,
                    now,
                    now,
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        // 5. Session-Chatters schreiben (Python Z. 2284–2336).
        match existing {
            Some(existing) => {
                // Lurker-Zeilen (seen_via_chatters_api=true, messages=0) neu labeln
                // (Python Z. 2291–2296).
                let resolved_first_global = if (existing.seen_via_chatters_api
                    && existing.messages == 0)
                    || existing.is_first_time_streamer.is_none()
                {
                    is_first_global
                } else {
                    existing.is_first_time_streamer.unwrap_or(false)
                };

                sqlx::query!(
                    "UPDATE twitch_session_chatters \
                     SET messages               = messages + 1, \
                         last_seen_at           = $1, \
                         seen_via_chatters_api  = FALSE, \
                         is_first_time_streamer = $2, \
                         chatter_id             = COALESCE(chatter_id, $3) \
                     WHERE session_id = $4 AND chatter_login = $5",
                    now,
                    resolved_first_global,
                    chatter_id,
                    session_id,
                    chatter_login,
                )
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query!(
                    "INSERT INTO twitch_session_chatters \
                     (session_id, streamer_login, chatter_login, chatter_id, first_message_at, \
                      messages, is_first_time_streamer, seen_via_chatters_api, last_seen_at) \
                     VALUES ($1, $2, $3, $4, $5, 1, $6, FALSE, $7)",
                    session_id,
                    login,
                    chatter_login,
                    chatter_id,
                    now,
                    is_first_global,
                    now,
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        // 6. Health-OK: insert_ok_at + Fehler löschen (Python Z. 2338–2343).
        upsert_ingest_health(&mut tx, login, None, Some(ts_iso), None, Some(None)).await?;

        tx.commit().await?;

        debug!(
            channel = %login,
            chatter = %chatter_login,
            session_id,
            is_first_global,
            "chatter_tracking: persisted"
        );

        Ok(())
    }

    /// Health-Upsert außerhalb einer Transaktion — best-effort, Fehler nur debug
    /// (`_persist_raw_chat_ingest_health`, moderation.py Z. 232–257).
    async fn persist_health(
        &self,
        login: &str,
        message_at: Option<&str>,
        ok_at: Option<&str>,
        error_at: Option<&str>,
        error_update: Option<Option<&str>>,
    ) {
        let mut conn = match self.pool.acquire().await {
            Ok(c) => c,
            Err(e) => {
                debug!(channel = %login, "chatter_tracking: health acquire fehlgeschlagen — {e}");
                return;
            }
        };
        if let Err(e) =
            upsert_ingest_health(&mut conn, login, message_at, ok_at, error_at, error_update).await
        {
            debug!(channel = %login, "chatter_tracking: health upsert fehlgeschlagen — {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------------

/// Liest `TB_CHAT_PERSIST_ALL_GAMES` — Default `true` (Chat aller Spiele
/// speichern). Nur `0`/`false` (case-insensitive) schaltet auf Deadlock-only.
fn persist_all_games_from_env() -> bool {
    std::env::var("TB_CHAT_PERSIST_ALL_GAMES")
        .map(|v| {
            let v = v.trim();
            !(v == "0" || v.eq_ignore_ascii_case("false"))
        })
        .unwrap_or(true)
}

/// ISO-Timestamp mit Sekunden-Auflösung — exakt Python
/// `datetime.now(UTC).isoformat(timespec="seconds")` (TEXT-Spalten!).
fn iso_seconds(ts: &DateTime<Utc>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

/// Fehlertext normalisieren: CR/LF → Space, trim, max 300 Zeichen, leer → None
/// (`_truncate_raw_chat_error`, moderation.py Z. 159–163).
fn truncate_raw_chat_error(value: &str) -> Option<String> {
    let text = value.replace(['\r', '\n'], " ").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text.chars().take(RAW_CHAT_ERROR_LIMIT).collect())
    }
}

/// Upsert auf `twitch_raw_chat_ingest_health` — Spalten sind TEXT, COALESCE
/// hält bestehende Werte; Fehlertext wird nur überschrieben wenn
/// `error_update` gesetzt ist (`_upsert_raw_chat_ingest_health_row`,
/// moderation.py Z. 165–230).
///
/// `error_update`: `None` = nicht anfassen, `Some(None)` = auf NULL setzen,
/// `Some(Some(text))` = Text setzen.
async fn upsert_ingest_health(
    conn: &mut sqlx::PgConnection,
    login: &str,
    message_at: Option<&str>,
    ok_at: Option<&str>,
    error_at: Option<&str>,
    error_update: Option<Option<&str>>,
) -> Result<(), sqlx::Error> {
    let updated_at = iso_seconds(&Utc::now());
    let should_update_error = error_update.is_some();
    let error_value: Option<&str> = error_update.flatten();
    // lag_seconds = 0 sobald irgendein Timestamp übergeben wurde (Python Z. 182–184).
    let lag_seconds: Option<i32> = if message_at.is_some() || ok_at.is_some() || error_at.is_some()
    {
        Some(0)
    } else {
        None
    };

    sqlx::query!(
        "INSERT INTO twitch_raw_chat_ingest_health ( \
             streamer_login, \
             last_raw_chat_message_at, \
             last_raw_chat_insert_ok_at, \
             last_raw_chat_insert_error_at, \
             last_raw_chat_error, \
             raw_chat_lag_seconds, \
             updated_at \
         ) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (streamer_login) DO UPDATE SET \
             last_raw_chat_message_at = COALESCE( \
                 EXCLUDED.last_raw_chat_message_at, \
                 twitch_raw_chat_ingest_health.last_raw_chat_message_at \
             ), \
             last_raw_chat_insert_ok_at = COALESCE( \
                 EXCLUDED.last_raw_chat_insert_ok_at, \
                 twitch_raw_chat_ingest_health.last_raw_chat_insert_ok_at \
             ), \
             last_raw_chat_insert_error_at = COALESCE( \
                 EXCLUDED.last_raw_chat_insert_error_at, \
                 twitch_raw_chat_ingest_health.last_raw_chat_insert_error_at \
             ), \
             last_raw_chat_error = CASE \
                 WHEN $8 THEN EXCLUDED.last_raw_chat_error \
                 ELSE twitch_raw_chat_ingest_health.last_raw_chat_error \
             END, \
             raw_chat_lag_seconds = COALESCE( \
                 EXCLUDED.raw_chat_lag_seconds, \
                 twitch_raw_chat_ingest_health.raw_chat_lag_seconds \
             ), \
             updated_at = EXCLUDED.updated_at",
        login,
        message_at,
        ok_at,
        error_at,
        error_value,
        lag_seconds,
        &updated_at,
        should_update_error,
    )
    .execute(conn)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_bots_erkannt() {
        assert!(KNOWN_CHAT_BOTS.contains(&"nightbot"));
        assert!(KNOWN_CHAT_BOTS.contains(&"streamlabs"));
        assert!(KNOWN_CHAT_BOTS.contains(&"fossabot"));
        assert!(!KNOWN_CHAT_BOTS.contains(&"echternutzer"));
        assert!(!KNOWN_CHAT_BOTS.contains(&""));
    }

    #[test]
    fn iso_seconds_format_paritaet() {
        // Python: datetime.now(UTC).isoformat(timespec="seconds")
        // → "2026-06-12T13:45:01+00:00"
        let ts = DateTime::parse_from_rfc3339("2026-06-12T13:45:01.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(iso_seconds(&ts), "2026-06-12T13:45:01+00:00");
    }

    #[test]
    fn fehlertext_trunkierung() {
        assert_eq!(truncate_raw_chat_error(""), None);
        assert_eq!(truncate_raw_chat_error("   "), None);
        assert_eq!(
            truncate_raw_chat_error("zeile1\nzeile2\rzeile3"),
            Some("zeile1 zeile2 zeile3".to_string())
        );
        let long = "x".repeat(400);
        assert_eq!(truncate_raw_chat_error(&long).unwrap().len(), 300);
    }

    #[test]
    fn command_flag_logik() {
        // is_command = getrimmter Content startet mit "!" (Python Z. 2171)
        assert!("  !invite".trim_start().starts_with('!'));
        assert!(!"hallo !invite".trim_start().starts_with('!'));
    }

    #[test]
    fn persist_all_games_env_parsing() {
        // Default (unset) → true; "0"/"false" → false; alles andere → true.
        assert!(persist_all_games_from_env()); // TB_CHAT_PERSIST_ALL_GAMES nicht gesetzt
    }
}

// ---------------------------------------------------------------------------
// DB-Tests (gegen TB_TEST_DATABASE_URL) — verifizieren das Game-Gate-Verhalten
// ---------------------------------------------------------------------------

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::types::ChatMessageBody;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            setup_schema(&dsn, $schema).await
        }};
    }

    /// Frisches Schema mit genau den Tabellen, die `track()` liest/schreibt.
    async fn setup_schema(dsn: &str, schema: &str) -> PgPool {
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
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();

        for ddl in [
            "CREATE TABLE twitch_stream_sessions (id BIGINT PRIMARY KEY, streamer_login TEXT, \
             started_at TIMESTAMPTZ DEFAULT now(), ended_at TIMESTAMPTZ, game_name TEXT)",
            "CREATE TABLE twitch_live_state (streamer_login TEXT PRIMARY KEY, is_live INT, \
             last_game TEXT)",
            "CREATE TABLE twitch_chat_messages (session_id BIGINT, streamer_login TEXT, \
             chatter_login TEXT, chatter_id TEXT, message_id TEXT, message_ts TIMESTAMPTZ, \
             is_command BOOL, content TEXT)",
            "CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, \
             chatter_login TEXT, chatter_id TEXT, first_message_at TIMESTAMPTZ, messages INT, \
             is_first_time_streamer BOOL, seen_via_chatters_api BOOL, last_seen_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_chatter_rollup (streamer_login TEXT, chatter_login TEXT, \
             chatter_id TEXT, first_seen_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ, \
             total_messages INT, total_sessions INT)",
            "CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, \
             last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, \
             last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT, \
             raw_chat_lag_seconds INT, updated_at TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    fn event(channel: &str, chatter: &str, text: &str) -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "b1".into(),
            broadcaster_user_login: channel.into(),
            chatter_user_id: "c1".into(),
            chatter_user_login: chatter.into(),
            message_id: "m1".into(),
            message: ChatMessageBody {
                text: text.into(),
                fragments: vec![],
            },
            ..Default::default()
        }
    }

    /// Offene Session, die NICHT im Zielspiel läuft (Arc Raiders statt Deadlock)
    /// und keinen `twitch_live_state`-Eintrag hat → Fallback auf Session-Game.
    async fn open_session_non_deadlock(pool: &PgPool) {
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, ended_at, game_name) \
             VALUES (1, 'cheazycrust', NULL, 'Arc Raiders')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn msg_count(pool: &PgPool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_chat_messages")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn persistiert_nicht_deadlock_wenn_flag_an() {
        let pool = pool_or_skip!("ct_persist_all");
        open_session_non_deadlock(&pool).await;
        let tracker = ChatterTracker::with_persist_all_games(pool.clone(), true);
        tracker
            .track(&event("cheazycrust", "sophiaa_star", "Howdy Howdy"))
            .await;
        assert_eq!(
            msg_count(&pool).await,
            1,
            "Nachricht muss trotz Nicht-Deadlock-Game gespeichert werden"
        );
    }

    #[tokio::test]
    async fn skippt_nicht_deadlock_wenn_flag_aus() {
        let pool = pool_or_skip!("ct_persist_deadlock_only");
        open_session_non_deadlock(&pool).await;
        let tracker = ChatterTracker::with_persist_all_games(pool.clone(), false);
        tracker
            .track(&event("cheazycrust", "sophiaa_star", "Howdy Howdy"))
            .await;
        assert_eq!(
            msg_count(&pool).await,
            0,
            "Deadlock-only: Nicht-Deadlock-Game darf NICHT gespeichert werden"
        );
    }
}
