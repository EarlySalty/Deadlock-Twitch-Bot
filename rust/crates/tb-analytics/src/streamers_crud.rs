//! CRUD-Queries für Streamer-Verwaltung (Schritt 3b).
//!
//! Alle Schreiboperationen arbeiten direkt auf `twitch_streamers` und/oder
//! `twitch_partners`. Discord-Nebeneffekte (Rollen-Sync, EventSub) sind
//! bewusst ausgelassen — kommen in Schritt 5/6.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

// ── GET /streamers ────────────────────────────────────────────────────────────

/// Eine Zeile der Admin-Streamer-Liste — Feldnamen sind der JSON-Vertrag
/// (identisch zu Pythons `_dashboard_list_sync`-Spalten).
///
/// Quelle ist seit der Partner-DB-Konsolidierung der View
/// `twitch_partners_all_state` (Partner-Lifecycle), NICHT mehr
/// `twitch_streamers` — dessen Duplikat-Spalten (u. a. `is_verified`)
/// wurden gedroppt. Timestamps im View sind TEXT; die Raid-Auth-Felder
/// kommen als echte TIMESTAMPTZ/BOOLEAN aus `twitch_raid_auth`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StreamerListRow {
    pub twitch_login: String,
    pub twitch_user_id: Option<String>,
    pub manual_verified_permanent: Option<i32>,
    pub manual_verified_until: Option<String>,
    pub manual_verified_at: Option<String>,
    pub manual_partner_opt_out: Option<i32>,
    pub archived_at: Option<String>,
    pub is_on_discord: Option<i32>,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub raid_bot_enabled: Option<i32>,
    pub raid_auth_enabled: Option<bool>,
    pub raid_needs_reauth: Option<bool>,
    pub raid_authorized_at: Option<DateTime<Utc>>,
    pub raid_token_expires_at: Option<DateTime<Utc>>,
    pub last_deadlock_stream_at: Option<DateTime<Utc>>,
}

/// Gibt alle aktiven Partner zurück (Python `_dashboard_list_sync`):
/// Partner-View + Raid-Auth-Join + letzter Deadlock-Stream je Login.
/// `target_game` für die Deadlock-Erkennung in den Sessions (z. B. "Deadlock").
pub async fn list_streamers(
    pool: &PgPool,
    target_game: &str,
) -> Result<Vec<StreamerListRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT s.twitch_login,
               COALESCE(NULLIF(s.twitch_user_id, ''), NULLIF(a.twitch_user_id, '')) AS twitch_user_id,
               s.manual_verified_permanent,
               s.manual_verified_until,
               s.manual_verified_at,
               s.manual_partner_opt_out,
               s.archived_at,
               s.is_on_discord,
               s.discord_user_id,
               s.discord_display_name,
               s.raid_bot_enabled,
               a.raid_enabled AS raid_auth_enabled,
               a.needs_reauth AS raid_needs_reauth,
               a.authorized_at AS raid_authorized_at,
               a.token_expires_at AS raid_token_expires_at,
               sess.last_deadlock_stream_at
          FROM twitch_partners_all_state s
          LEFT JOIN twitch_raid_auth a
            ON (
                 s.twitch_user_id IS NOT NULL
                 AND s.twitch_user_id = a.twitch_user_id
               )
            OR (
                 s.twitch_user_id IS NULL
                 AND LOWER(s.twitch_login) = LOWER(a.twitch_login)
               )
          LEFT JOIN (
               SELECT LOWER(streamer_login) AS streamer_login,
                      MAX(CASE
                            WHEN had_deadlock_in_session
                                 OR LOWER(COALESCE(game_name,'')) = LOWER($1)
                            THEN COALESCE(ended_at, started_at)
                      END) AS last_deadlock_stream_at
                 FROM twitch_stream_sessions
                GROUP BY LOWER(streamer_login)
          ) AS sess
            ON sess.streamer_login = LOWER(s.twitch_login)
         WHERE s.status = 'active'
          ORDER BY s.twitch_login
        "#,
    )
    .bind(target_game)
    .fetch_all(pool)
    .await
}

// ── POST /streamers (Add) ─────────────────────────────────────────────────────

/// Ergebnis von `add_streamer`.
#[derive(Debug)]
pub enum AddStreamerResult {
    /// Streamer war bereits aktiv (archived_at IS NULL).
    AlreadyExists,
    /// Erfolgreich hinzugefügt oder user_id aktualisiert.
    Added,
}

/// Fügt einen Streamer in `twitch_streamers` ein (upsert).
///
/// Entspricht dem Python-Pfad in `upsert_non_partner_streamer`:
/// - Wenn bereits aktiv (archived_at IS NULL): `AlreadyExists` zurückgeben.
/// - Sonst: INSERT mit `is_monitored_only = 0`, ON CONFLICT DO UPDATE user_id.
///
/// `twitch_streamer_identities` wird nur befüllt wenn `user_id` bekannt ist.
pub async fn add_streamer(
    pool: &PgPool,
    login: &str,
    user_id: Option<&str>,
) -> Result<AddStreamerResult, sqlx::Error> {
    let login = login.to_lowercase();

    // Prüfen ob bereits aktiv (`SELECT 1` ist INT4 — als i32 dekodieren)
    let exists: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) AND archived_at IS NULL",
    )
    .bind(&login)
    .fetch_optional(pool)
    .await?;

    if exists.is_some() {
        return Ok(AddStreamerResult::AlreadyExists);
    }

    // Upsert in twitch_streamers
    sqlx::query(
        r#"
        INSERT INTO twitch_streamers (
            twitch_login,
            twitch_user_id,
            is_on_discord,
            is_monitored_only,
            created_at
        ) VALUES ($1, $2, 0, 0, NOW())
        ON CONFLICT (twitch_login) DO UPDATE SET
            twitch_user_id = COALESCE(EXCLUDED.twitch_user_id, twitch_streamers.twitch_user_id),
            archived_at = NULL,
            is_monitored_only = 0
        "#,
    )
    .bind(&login)
    .bind(user_id)
    .execute(pool)
    .await?;

    // Identity-Eintrag wenn user_id bekannt
    if let Some(uid) = user_id {
        if !uid.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO twitch_streamer_identities (
                    twitch_user_id,
                    twitch_login,
                    is_on_discord,
                    created_at,
                    updated_at
                ) VALUES ($1, $2, 0, NOW(), NOW())
                ON CONFLICT (twitch_user_id) DO UPDATE SET
                    twitch_login = EXCLUDED.twitch_login,
                    updated_at = NOW()
                "#,
            )
            .bind(uid)
            .bind(&login)
            .execute(pool)
            .await?;
        }
    }

    Ok(AddStreamerResult::Added)
}

// ── DELETE /streamers/{login} (Remove) ───────────────────────────────────────

/// Ergebnis von `remove_streamer`.
#[derive(Debug)]
pub enum RemoveStreamerResult {
    /// Aktiver Eintrag archiviert (archived_at gesetzt, twitch_live_state gelöscht).
    Archived,
    /// Kein aktiver Eintrag — direktes DELETE (war nie aktiv oder bereits archiviert).
    Deleted,
    /// Login unbekannt.
    NotFound,
}

/// Entfernt/departnert einen Streamer.
///
/// Entspricht dem Python-Pfad in `_cmd_remove`:
/// 1. UPDATE archived_at = NOW() WHERE archived_at IS NULL (aktiver Eintrag)
/// 2. Wenn kein Update: DELETE (war nie aktiv)
/// 3. DELETE FROM twitch_live_state
pub async fn remove_streamer(
    pool: &PgPool,
    login: &str,
) -> Result<RemoveStreamerResult, sqlx::Error> {
    let login = login.to_lowercase();

    // Schritt 1: Aktiven Eintrag archivieren
    let updated = sqlx::query(
        "UPDATE twitch_streamers SET archived_at = NOW() WHERE LOWER(twitch_login) = LOWER($1) AND archived_at IS NULL",
    )
    .bind(&login)
    .execute(pool)
    .await?;

    let result = if updated.rows_affected() > 0 {
        RemoveStreamerResult::Archived
    } else {
        // Schritt 2: Falls nichts archiviert wurde, direktes DELETE
        let deleted =
            sqlx::query("DELETE FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1)")
                .bind(&login)
                .execute(pool)
                .await?;

        if deleted.rows_affected() > 0 {
            RemoveStreamerResult::Deleted
        } else {
            RemoveStreamerResult::NotFound
        }
    };

    // Schritt 3: Live-State löschen (auch wenn NotFound — idempotent)
    sqlx::query("DELETE FROM twitch_live_state WHERE LOWER(streamer_login) = LOWER($1)")
        .bind(&login)
        .execute(pool)
        .await?;

    Ok(result)
}

// ── POST /streamers/{login}/verify ───────────────────────────────────────────

/// Ergebnis von `verify_streamer`.
#[derive(Debug)]
pub enum VerifyStreamerResult {
    /// Verifikation gesetzt.
    Verified,
    /// Kein aktiver Partner-Eintrag — Verifikation nicht möglich.
    NotAPartner,
}

/// Setzt `manual_verified_permanent = 1, manual_verified_at = NOW()` in `twitch_partners`.
///
/// Parität Python `_dashboard_verify_storage_step`:
/// - mode "permanent" → dauerhaft verifizieren
/// - mode "temp"      → befristet (30 Tage) verifizieren: manual_verified_at = NOW(), manual_verified_until = NOW()+30d
/// - mode "clear"     → Verifizierung zurücksetzen
/// - Alles andere     → "permanent"-Semantik (Python-Fallback)
///
/// Nur für aktive Partner (`status = 'active'`). Eine Promotion von
/// `twitch_streamers` → `twitch_partners` ist Bestandteil eines späteren Schritts.
pub async fn verify_streamer(
    pool: &PgPool,
    login: &str,
    mode: &str,
) -> Result<VerifyStreamerResult, sqlx::Error> {
    let mode_clean = mode.trim().to_lowercase();
    let updated = match mode_clean.as_str() {
        "temp" => sqlx::query(
            r#"
            UPDATE twitch_partners
            SET manual_verified_permanent = 0,
                manual_verified_at = NOW(),
                manual_verified_until = NOW() + INTERVAL '30 days'
            WHERE LOWER(twitch_login) = LOWER($1)
              AND COALESCE(status, '') = 'active'
            "#,
        )
        .bind(login)
        .execute(pool)
        .await?
        .rows_affected(),
        "clear" => sqlx::query(
            r#"
            UPDATE twitch_partners
            SET manual_verified_permanent = 0,
                manual_verified_at = NULL,
                manual_verified_until = NULL
            WHERE LOWER(twitch_login) = LOWER($1)
              AND COALESCE(status, '') = 'active'
            "#,
        )
        .bind(login)
        .execute(pool)
        .await?
        .rows_affected(),
        // "permanent" und alle anderen Werte → dauerhaft verifizieren
        _ => sqlx::query(
            r#"
            UPDATE twitch_partners
            SET manual_verified_permanent = 1,
                manual_verified_at = NOW()
            WHERE LOWER(twitch_login) = LOWER($1)
              AND COALESCE(status, '') = 'active'
            "#,
        )
        .bind(login)
        .execute(pool)
        .await?
        .rows_affected(),
    };

    if updated > 0 {
        Ok(VerifyStreamerResult::Verified)
    } else {
        Ok(VerifyStreamerResult::NotAPartner)
    }
}

// ── POST /streamers/{login}/archive ──────────────────────────────────────────

/// Archivierungs-/Block-Modus.
///
/// Parität Python `_dashboard_archive`:
/// - "archive" | "on" | "set"              → Archive
/// - "unarchive" | "off" | "unset" | "restore" → Unarchive
/// - "block" | "blocked" | "ban"           → Block
/// - "unblock" | "allow"                   → Unblock
/// - "toggle_block" | "block_toggle"       → ToggleBlock
/// - alles andere (inkl. "toggle")         → Toggle
///
/// Python gibt NIEMALS 400 für unbekannte modi — unbekannte Werte fallen immer
/// durch auf Toggle. Deshalb ist `parse` infallibel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveMode {
    Archive,
    Unarchive,
    Block,
    Unblock,
    ToggleBlock,
    /// Fallback für unbekannte Werte und explizit "toggle" — entspricht
    /// Python-`else: desired = "toggle"`.
    Toggle,
}

impl ArchiveMode {
    /// Parst einen mode-String; gibt immer `Some(...)` zurück.
    /// Unbekannte Werte → `Toggle` (Python-Semantik, kein 400).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "archive" | "on" | "set" => Self::Archive,
            "unarchive" | "off" | "unset" | "restore" => Self::Unarchive,
            "block" | "blocked" | "ban" => Self::Block,
            "unblock" | "allow" => Self::Unblock,
            "toggle_block" | "block_toggle" => Self::ToggleBlock,
            // "toggle" und alle unbekannten Werte
            _ => Self::Toggle,
        }
    }
}

/// Archiviert, reaktiviert, blockiert oder entsperrt einen Partner-Eintrag.
///
/// Entspricht `set_streamer_archive_state` / `set_streamer_block_state` in Python.
///
/// - Archive/Unarchive: `admin_archived_at` in `twitch_partners`
/// - Block: `technical_pause_reason = 'blocked', manual_partner_opt_out = 1, raid_bot_enabled = 0`
/// - Unblock: `technical_pause_reason = NULL, manual_partner_opt_out = 0`
/// - Toggle: `admin_archived_at` umschalten (NULL→NOW() oder NOW()→NULL)
/// - ToggleBlock: `technical_pause_reason` umschalten ('blocked'↔NULL)
///
/// Gibt `false` zurück wenn kein Eintrag gefunden wurde (→ 404).
pub async fn archive_streamer(
    pool: &PgPool,
    login: &str,
    mode: ArchiveMode,
) -> Result<bool, sqlx::Error> {
    let rows = match mode {
        ArchiveMode::Archive => sqlx::query(
            r#"
                UPDATE twitch_partners
                SET admin_archived_at = NOW()
                WHERE LOWER(twitch_login) = LOWER($1)
                  AND admin_archived_at IS NULL
                "#,
        )
        .bind(login)
        .execute(pool)
        .await?
        .rows_affected(),
        ArchiveMode::Unarchive => sqlx::query(
            r#"
                UPDATE twitch_partners
                SET admin_archived_at = NULL
                WHERE LOWER(twitch_login) = LOWER($1)
                  AND admin_archived_at IS NOT NULL
                "#,
        )
        .bind(login)
        .execute(pool)
        .await?
        .rows_affected(),
        ArchiveMode::Block => sqlx::query(
            r#"
                UPDATE twitch_partners
                SET technical_pause_reason = 'blocked',
                    manual_partner_opt_out = 1,
                    raid_bot_enabled = 0
                WHERE LOWER(twitch_login) = LOWER($1)
                "#,
        )
        .bind(login)
        .execute(pool)
        .await?
        .rows_affected(),
        ArchiveMode::Unblock => sqlx::query(
            r#"
                UPDATE twitch_partners
                SET technical_pause_reason = NULL,
                    manual_partner_opt_out = 0
                WHERE LOWER(twitch_login) = LOWER($1)
                  AND LOWER(COALESCE(technical_pause_reason, '')) = 'blocked'
                "#,
        )
        .bind(login)
        .execute(pool)
        .await?
        .rows_affected(),
        ArchiveMode::ToggleBlock => {
            // Aktuellen Wert lesen, dann gegenteilig setzen
            let current: Option<(Option<String>,)> = sqlx::query_as(
                "SELECT technical_pause_reason FROM twitch_partners WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
            )
            .bind(login)
            .fetch_optional(pool)
            .await?;
            let Some((pause_reason,)) = current else {
                return Ok(false);
            };
            let currently_blocked = pause_reason
                .as_deref()
                .map(|s| s.to_lowercase() == "blocked")
                .unwrap_or(false);
            if currently_blocked {
                sqlx::query(
                    "UPDATE twitch_partners SET technical_pause_reason = NULL, manual_partner_opt_out = 0 WHERE LOWER(twitch_login) = LOWER($1)",
                )
                .bind(login)
                .execute(pool)
                .await?
                .rows_affected()
            } else {
                sqlx::query(
                    "UPDATE twitch_partners SET technical_pause_reason = 'blocked', manual_partner_opt_out = 1, raid_bot_enabled = 0 WHERE LOWER(twitch_login) = LOWER($1)",
                )
                .bind(login)
                .execute(pool)
                .await?
                .rows_affected()
            }
        }
        ArchiveMode::Toggle => {
            // Aktuellen admin_archived_at lesen und umschalten
            let current: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
                "SELECT admin_archived_at FROM twitch_partners WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
            )
            .bind(login)
            .fetch_optional(pool)
            .await?;
            let Some((archived_at,)) = current else {
                return Ok(false);
            };
            if archived_at.is_some() {
                // Bereits archiviert → reaktivieren
                sqlx::query(
                    "UPDATE twitch_partners SET admin_archived_at = NULL WHERE LOWER(twitch_login) = LOWER($1)",
                )
                .bind(login)
                .execute(pool)
                .await?
                .rows_affected()
            } else {
                // Aktiv → archivieren
                sqlx::query(
                    "UPDATE twitch_partners SET admin_archived_at = NOW() WHERE LOWER(twitch_login) = LOWER($1)",
                )
                .bind(login)
                .execute(pool)
                .await?
                .rows_affected()
            }
        }
    };

    Ok(rows > 0)
}

// ── POST /streamers/{login}/discord-flag ─────────────────────────────────────

/// Setzt `is_on_discord` für einen Streamer.
///
/// Dual-Pfad (PostgreSQL-spezifisch: `UPDATE ... FROM`):
/// - Aktiver Partner (`twitch_partners.status = 'active'`): Update via `twitch_streamer_identities`
/// - Non-Partner: Update in `twitch_streamers`
///
/// Gibt `false` zurück wenn Login unbekannt.
pub async fn set_discord_flag(
    pool: &PgPool,
    login: &str,
    is_on_discord: bool,
) -> Result<bool, sqlx::Error> {
    let val: i32 = if is_on_discord { 1 } else { 0 };

    // Partner-Pfad: über twitch_streamer_identities (PostgreSQL UPDATE...FROM)
    let partner_rows = sqlx::query(
        r#"
        UPDATE twitch_streamer_identities si
        SET is_on_discord = $2,
            updated_at = NOW()
        FROM twitch_partners p
        WHERE p.twitch_user_id = si.twitch_user_id
          AND LOWER(p.twitch_login) = LOWER($1)
          AND COALESCE(p.status, '') = 'active'
        "#,
    )
    .bind(login)
    .bind(val)
    .execute(pool)
    .await?
    .rows_affected();

    if partner_rows > 0 {
        return Ok(true);
    }

    // Non-Partner-Pfad: twitch_streamers direkt
    let streamer_rows = sqlx::query(
        r#"
        UPDATE twitch_streamers
        SET is_on_discord = $2
        WHERE LOWER(twitch_login) = LOWER($1)
          AND archived_at IS NULL
        "#,
    )
    .bind(login)
    .bind(val)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(streamer_rows > 0)
}

// ── POST /streamers/{login}/discord-profile ───────────────────────────────────

/// Setzt Discord-User-ID + Display-Name für einen Streamer.
///
/// Dual-Pfad (PostgreSQL-spezifisch: `UPDATE ... FROM`):
/// - Aktiver Partner: Update `twitch_streamer_identities` (nach Deduplizierung)
/// - Non-Partner: Update `twitch_streamers`
///
/// Deduplizierung: Andere Identity-Einträge mit gleicher `discord_user_id` werden genullt.
///
/// Gibt `false` zurück wenn Login unbekannt.
pub async fn set_discord_profile(
    pool: &PgPool,
    login: &str,
    discord_user_id: Option<&str>,
    discord_display_name: Option<&str>,
    mark_member: bool,
) -> Result<bool, sqlx::Error> {
    let is_on_discord: i32 = if mark_member { 1 } else { 0 };

    // Deduplizierung: Andere Identity-Einträge mit gleicher discord_user_id nullen
    if let Some(did) = discord_user_id {
        if !did.is_empty() {
            let target_uid: Option<(Option<String>,)> = sqlx::query_as(
                "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1)",
            )
            .bind(login)
            .fetch_optional(pool)
            .await?;

            if let Some((Some(uid),)) = target_uid {
                sqlx::query(
                    r#"
                    UPDATE twitch_streamer_identities
                    SET discord_user_id = NULL,
                        discord_display_name = NULL,
                        is_on_discord = 0,
                        updated_at = NOW()
                    WHERE discord_user_id = $1
                      AND twitch_user_id <> $2
                    "#,
                )
                .bind(did)
                .bind(&uid)
                .execute(pool)
                .await?;
            }
        }
    }

    // Partner-Pfad: twitch_streamer_identities (PostgreSQL UPDATE...FROM)
    let partner_rows = sqlx::query(
        r#"
        UPDATE twitch_streamer_identities si
        SET discord_user_id = COALESCE($2, si.discord_user_id),
            discord_display_name = COALESCE($3, si.discord_display_name),
            is_on_discord = $4,
            updated_at = NOW()
        FROM twitch_partners p
        WHERE p.twitch_user_id = si.twitch_user_id
          AND LOWER(p.twitch_login) = LOWER($1)
          AND COALESCE(p.status, '') = 'active'
        "#,
    )
    .bind(login)
    .bind(discord_user_id)
    .bind(discord_display_name)
    .bind(is_on_discord)
    .execute(pool)
    .await?
    .rows_affected();

    if partner_rows > 0 {
        return Ok(true);
    }

    // Non-Partner-Pfad: twitch_streamers
    let streamer_rows = sqlx::query(
        r#"
        UPDATE twitch_streamers
        SET discord_user_id = COALESCE($2, discord_user_id),
            discord_display_name = COALESCE($3, discord_display_name),
            is_on_discord = $4
        WHERE LOWER(twitch_login) = LOWER($1)
          AND archived_at IS NULL
        "#,
    )
    .bind(login)
    .bind(discord_user_id)
    .bind(discord_display_name)
    .bind(is_on_discord)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(streamer_rows > 0)
}

// ── GET /stats ────────────────────────────────────────────────────────────────

/// Kompakte Übersichts-Statistik (Parität Python `_dashboard_stats` / `_compute_stats`).
///
/// Liefert aggregierte Metriken aus `twitch_stream_sessions`.
/// `hour_from`/`hour_to`: Stunden-Fenster (UTC-Stunde 0–23). `None` = kein Filter.
/// `streamer`: Login-Filter. `None` = alle aktiven Partner.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StatsRow {
    pub total_sessions: Option<i64>,
    pub avg_viewers: Option<f64>,
    pub peak_viewers: Option<i64>,
    pub total_duration_hours: Option<f64>,
    pub total_follower_delta: Option<i64>,
}

pub async fn stats(
    pool: &PgPool,
    hour_from: Option<i32>,
    hour_to: Option<i32>,
    streamer: Option<&str>,
) -> Result<StatsRow, sqlx::Error> {
    // hour_from / hour_to filtern auf EXTRACT(HOUR FROM started_at)
    let row: StatsRow = sqlx::query_as(
        r#"
        SELECT COUNT(*)::BIGINT                                                AS total_sessions,
               AVG(avg_viewers)::FLOAT8                                        AS avg_viewers,
               MAX(peak_viewers)::BIGINT                                       AS peak_viewers,
               COALESCE(SUM(duration_seconds / 3600.0)::FLOAT8, 0.0)          AS total_duration_hours,
               COALESCE(SUM(follower_delta)::BIGINT, 0)                        AS total_follower_delta
          FROM twitch_stream_sessions
         WHERE ($1::INT IS NULL OR EXTRACT(HOUR FROM started_at AT TIME ZONE 'UTC') >= $1)
           AND ($2::INT IS NULL OR EXTRACT(HOUR FROM started_at AT TIME ZONE 'UTC') <= $2)
           AND ($3::TEXT IS NULL OR LOWER(streamer_login) = LOWER($3))
        "#,
    )
    .bind(hour_from)
    .bind(hour_to)
    .bind(streamer)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

// ── GET /analytics/streamer/{login} ──────────────────────────────────────────

/// Aggregierte Streamer-Analytik für N Tage (Parität Python `_dashboard_streamer_analytics_data`).
///
/// Gibt Überblick + letzte 20 Sessions zurück — analog zu `_dashboard_streamer_overview_sync`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StreamerStats30dRow {
    pub total_sessions: Option<i64>,
    pub total_duration_seconds: Option<i64>,
    pub avg_avg_viewers: Option<f64>,
    pub max_peak_viewers: Option<i64>,
    pub total_follower_delta: Option<i64>,
    pub total_unique_chatters: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RecentSessionRow {
    pub id: Option<i64>,
    pub stream_id: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i64>,
    pub avg_viewers: Option<f64>,
    pub peak_viewers: Option<i64>,
    pub follower_delta: Option<i64>,
    pub stream_title: Option<String>,
}

pub async fn streamer_analytics(
    pool: &PgPool,
    login: &str,
    days: i32,
) -> Result<(StreamerStats30dRow, Vec<RecentSessionRow>), sqlx::Error> {
    let agg: StreamerStats30dRow = sqlx::query_as(
        r#"
        SELECT COUNT(*)::BIGINT                                        AS total_sessions,
               COALESCE(SUM(duration_seconds)::BIGINT, 0)             AS total_duration_seconds,
               AVG(avg_viewers)::FLOAT8                                AS avg_avg_viewers,
               MAX(peak_viewers)::BIGINT                               AS max_peak_viewers,
               COALESCE(SUM(follower_delta)::BIGINT, 0)               AS total_follower_delta,
               COALESCE(SUM(unique_chatters)::BIGINT, 0)              AS total_unique_chatters
          FROM twitch_stream_sessions
         WHERE LOWER(streamer_login) = LOWER($1)
           AND started_at > NOW() - ($2 || ' days')::INTERVAL
        "#,
    )
    .bind(login)
    .bind(days)
    .fetch_one(pool)
    .await?;

    let sessions: Vec<RecentSessionRow> = sqlx::query_as(
        r#"
        SELECT id, stream_id, started_at, duration_seconds,
               avg_viewers, peak_viewers, follower_delta, stream_title
          FROM twitch_stream_sessions
         WHERE LOWER(streamer_login) = LOWER($1)
         ORDER BY started_at DESC
         LIMIT 20
        "#,
    )
    .bind(login)
    .fetch_all(pool)
    .await?;

    Ok((agg, sessions))
}

// ── GET /analytics/comparison ─────────────────────────────────────────────────

/// Vergleichs-Statistik (Parität Python `_dashboard_comparison_stats_sync`).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ComparisonCategoryRow {
    pub avg_viewers: Option<f64>,
    pub peak_viewers: Option<f64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ComparisonTopStreamerRow {
    pub streamer_login: String,
    pub avg_viewers: Option<f64>,
}

pub async fn analytics_comparison(
    pool: &PgPool,
    days: i32,
) -> Result<
    (
        ComparisonCategoryRow,
        ComparisonCategoryRow,
        Vec<ComparisonTopStreamerRow>,
    ),
    sqlx::Error,
> {
    let category: ComparisonCategoryRow = sqlx::query_as(
        r#"
        SELECT AVG(viewer_count)::FLOAT8 AS avg_viewers,
               MAX(viewer_count)::FLOAT8 AS peak_viewers
          FROM twitch_stats_category
         WHERE ts_utc > NOW() - ($1 || ' days')::INTERVAL
        "#,
    )
    .bind(days)
    .fetch_one(pool)
    .await?;

    let tracked: ComparisonCategoryRow = sqlx::query_as(
        r#"
        SELECT AVG(viewer_count)::FLOAT8 AS avg_viewers,
               MAX(viewer_count)::FLOAT8 AS peak_viewers
          FROM twitch_stats_tracked
         WHERE ts_utc > NOW() - ($1 || ' days')::INTERVAL
        "#,
    )
    .bind(days)
    .fetch_one(pool)
    .await?;

    let top: Vec<ComparisonTopStreamerRow> = sqlx::query_as(
        r#"
        SELECT streamer_login,
               AVG(avg_viewers)::FLOAT8 AS avg_viewers
          FROM twitch_stream_sessions
         WHERE started_at > NOW() - ($1 || ' days')::INTERVAL
         GROUP BY streamer_login
         ORDER BY avg_viewers DESC NULLS LAST
         LIMIT 5
        "#,
    )
    .bind(days)
    .fetch_all(pool)
    .await?;

    Ok((category, tracked, top))
}

// ── GET /sessions/{session_id} ────────────────────────────────────────────────

/// Detail-Daten einer einzelnen Stream-Session (Parität Python `_dashboard_session_detail_sync`).
#[derive(Debug, Serialize)]
pub struct SessionDetail {
    pub session: serde_json::Value,
    pub timeline: Vec<serde_json::Value>,
    pub top_chatters: Vec<serde_json::Value>,
}

pub async fn session_detail(
    pool: &PgPool,
    session_id: i64,
) -> Result<Option<SessionDetail>, sqlx::Error> {
    // Session-Stammdaten als dynamische JSON-Row
    let session_row: Option<serde_json::Value> = sqlx::query_scalar(
        r#"
        SELECT row_to_json(s)
          FROM (
               SELECT id, stream_id, streamer_login, started_at, ended_at,
                      duration_seconds, avg_viewers, peak_viewers,
                      follower_delta, stream_title, unique_chatters,
                      had_deadlock_in_session, game_name
                 FROM twitch_stream_sessions
                WHERE id = $1
               ) s
        "#,
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    let Some(session) = session_row else {
        return Ok(None);
    };

    // Viewer-Timeline (twitch_session_viewers)
    let timeline: Vec<serde_json::Value> = sqlx::query_scalar(
        r#"
        SELECT row_to_json(t)
          FROM (
               SELECT minutes_from_start, viewer_count
                 FROM twitch_session_viewers
                WHERE session_id = $1
                ORDER BY minutes_from_start ASC
               ) t
        "#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Top-Chatter (twitch_session_chatters)
    let top_chatters: Vec<serde_json::Value> = sqlx::query_scalar(
        r#"
        SELECT row_to_json(c)
          FROM (
               SELECT chatter_login, messages
                 FROM twitch_session_chatters
                WHERE session_id = $1
                ORDER BY messages DESC
                LIMIT 10
               ) c
        "#,
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    Ok(Some(SessionDetail {
        session,
        timeline,
        top_chatters,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!(
                            "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt"
                        );
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("DB-Verbindung");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");

        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamers (
                twitch_login        TEXT PRIMARY KEY,
                twitch_user_id      TEXT,
                discord_user_id     TEXT,
                discord_display_name TEXT,
                is_on_discord       INTEGER DEFAULT 0,
                is_verified         INTEGER DEFAULT 0,
                is_monitored_only   INTEGER DEFAULT 0,
                created_at          TIMESTAMPTZ DEFAULT NOW(),
                archived_at         TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamer_identities (
                twitch_user_id      TEXT PRIMARY KEY,
                twitch_login        TEXT NOT NULL,
                discord_user_id     TEXT,
                discord_display_name TEXT,
                is_on_discord       INTEGER DEFAULT 0,
                created_at          TIMESTAMPTZ DEFAULT NOW(),
                updated_at          TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamer_identities");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login TEXT PRIMARY KEY,
                is_live        INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id                       SERIAL PRIMARY KEY,
                twitch_login             TEXT NOT NULL,
                twitch_user_id           TEXT,
                status                   TEXT DEFAULT 'active',
                manual_verified_permanent INTEGER DEFAULT 0,
                manual_verified_at       TIMESTAMPTZ,
                manual_verified_until    TIMESTAMPTZ,
                admin_archived_at        TIMESTAMPTZ,
                technical_pause_reason   TEXT,
                manual_partner_opt_out   INTEGER DEFAULT 0,
                raid_bot_enabled         INTEGER DEFAULT 1
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_partners");

        // Quellen der neuen Listen-Query: Partner-View (im Test als Tabelle
        // mit den referenzierten Spalten), Raid-Auth und Sessions.
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners_all_state (
                twitch_login             TEXT,
                twitch_user_id           TEXT,
                manual_verified_permanent INTEGER DEFAULT 0,
                manual_verified_until    TEXT,
                manual_verified_at       TEXT,
                manual_partner_opt_out   INTEGER DEFAULT 0,
                archived_at              TEXT,
                is_on_discord            INTEGER DEFAULT 0,
                discord_user_id          TEXT,
                discord_display_name    TEXT,
                raid_bot_enabled         INTEGER DEFAULT 1,
                status                   TEXT DEFAULT 'active'
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_partners_all_state");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_auth (
                twitch_user_id   TEXT PRIMARY KEY,
                twitch_login     TEXT,
                raid_enabled     BOOLEAN,
                needs_reauth     BOOLEAN,
                authorized_at    TIMESTAMPTZ,
                token_expires_at TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_raid_auth");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id                      BIGSERIAL PRIMARY KEY,
                stream_id               TEXT,
                streamer_login          TEXT,
                game_name               TEXT,
                had_deadlock_in_session BOOLEAN DEFAULT FALSE,
                started_at              TIMESTAMPTZ,
                ended_at                TIMESTAMPTZ,
                duration_seconds        BIGINT,
                avg_viewers             FLOAT8,
                peak_viewers            BIGINT,
                follower_delta          BIGINT,
                followers_start         BIGINT,
                followers_end           BIGINT,
                stream_title            TEXT,
                unique_chatters         BIGINT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_stream_sessions");

        sqlx::query(
            "TRUNCATE twitch_streamers, twitch_streamer_identities, twitch_live_state, twitch_partners, twitch_partners_all_state, twitch_raid_auth, twitch_stream_sessions RESTART IDENTITY",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");

        pool
    }

    #[tokio::test]
    async fn list_gibt_leere_liste_bei_leerer_db() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_list_empty").await;
        let result = list_streamers(&pool, "Deadlock").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn add_fuegt_streamer_ein() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_add").await;

        let result = add_streamer(&pool, "testuser", Some("12345"))
            .await
            .unwrap();
        assert!(matches!(result, AddStreamerResult::Added));

        // add schreibt Nicht-Partner nach twitch_streamers — die Admin-Liste
        // zeigt nur aktive Partner (Python-Semantik), daher Direkt-Check.
        let row: (String, Option<String>) = sqlx::query_as(
            "SELECT twitch_login, twitch_user_id FROM twitch_streamers WHERE twitch_login='testuser'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "testuser");
        assert_eq!(row.1.as_deref(), Some("12345"));
    }

    #[tokio::test]
    async fn list_liefert_partner_mit_raid_auth_und_deadlock_session() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_list_rich").await;

        sqlx::query(
            "INSERT INTO twitch_partners_all_state
                (twitch_login, twitch_user_id, manual_verified_permanent, status, raid_bot_enabled)
             VALUES ('drag', '42', 1, 'active', 1),
                    ('archiviert', '99', 0, 'archived', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth
                (twitch_user_id, twitch_login, raid_enabled, needs_reauth, authorized_at)
             VALUES ('42', 'drag', TRUE, FALSE, NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions
                (streamer_login, game_name, had_deadlock_in_session, started_at, ended_at)
             VALUES ('Drag', 'Deadlock', TRUE, NOW() - INTERVAL '3 hours', NOW() - INTERVAL '1 hour')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let list = list_streamers(&pool, "Deadlock").await.unwrap();
        assert_eq!(list.len(), 1, "nur status='active'");
        let row = &list[0];
        assert_eq!(row.twitch_login, "drag");
        assert_eq!(row.twitch_user_id.as_deref(), Some("42"));
        assert_eq!(row.manual_verified_permanent, Some(1));
        assert_eq!(row.raid_auth_enabled, Some(true));
        assert_eq!(row.raid_needs_reauth, Some(false));
        assert!(row.raid_authorized_at.is_some());
        assert!(
            row.last_deadlock_stream_at.is_some(),
            "Session-Join (case-insensitiver Login)"
        );
    }

    #[tokio::test]
    async fn add_gibt_already_exists_bei_doppeltem_add() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_add_dup").await;

        add_streamer(&pool, "testuser", Some("12345"))
            .await
            .unwrap();
        let result = add_streamer(&pool, "testuser", Some("12345"))
            .await
            .unwrap();
        assert!(matches!(result, AddStreamerResult::AlreadyExists));
    }

    #[tokio::test]
    async fn remove_archiviert_vorhandenen_streamer() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_remove").await;

        add_streamer(&pool, "testuser", None).await.unwrap();
        let result = remove_streamer(&pool, "testuser").await.unwrap();
        assert!(matches!(result, RemoveStreamerResult::Archived));

        let archived: Option<String> = sqlx::query_scalar(
            "SELECT archived_at::text FROM twitch_streamers WHERE twitch_login='testuser'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(archived.is_some(), "archiviert statt gelistet");
    }

    #[tokio::test]
    async fn remove_gibt_not_found_bei_unbekanntem_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_remove_nf").await;

        let result = remove_streamer(&pool, "nichtvorhanden").await.unwrap();
        assert!(matches!(result, RemoveStreamerResult::NotFound));
    }

    #[tokio::test]
    async fn verify_setzt_is_verified_bei_aktivem_partner() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_verify").await;

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('testpartner', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = verify_streamer(&pool, "testpartner", "permanent").await.unwrap();
        assert!(matches!(result, VerifyStreamerResult::Verified));

        let row: (Option<i32>,) = sqlx::query_as(
            "SELECT manual_verified_permanent FROM twitch_partners WHERE LOWER(twitch_login) = 'testpartner'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, Some(1));
    }

    #[tokio::test]
    async fn verify_temp_setzt_until_datum() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_verify_temp").await;

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('tmppartner', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = verify_streamer(&pool, "tmppartner", "temp").await.unwrap();
        assert!(matches!(result, VerifyStreamerResult::Verified));

        let row: (Option<i32>, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT manual_verified_permanent, manual_verified_until FROM twitch_partners WHERE LOWER(twitch_login) = 'tmppartner'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, Some(0), "temp = kein permanent-Flag");
        assert!(row.1.is_some(), "manual_verified_until muss gesetzt sein");
    }

    #[tokio::test]
    async fn verify_gibt_not_a_partner_fuer_unbekannten_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_verify_nap").await;

        let result = verify_streamer(&pool, "niemand", "permanent").await.unwrap();
        assert!(matches!(result, VerifyStreamerResult::NotAPartner));
    }

    #[tokio::test]
    async fn archive_mode_parse_ist_infallibel() {
        // Unbekannte Werte → Toggle (Python-Semantik, kein 400)
        assert_eq!(ArchiveMode::parse("ungueltig"), ArchiveMode::Toggle);
        assert_eq!(ArchiveMode::parse(""), ArchiveMode::Toggle);
        assert_eq!(ArchiveMode::parse("toggle"), ArchiveMode::Toggle);
        assert_eq!(ArchiveMode::parse("archive"), ArchiveMode::Archive);
        assert_eq!(ArchiveMode::parse("UNARCHIVE"), ArchiveMode::Unarchive);
        assert_eq!(ArchiveMode::parse("block"), ArchiveMode::Block);
        assert_eq!(ArchiveMode::parse("unblock"), ArchiveMode::Unblock);
        assert_eq!(ArchiveMode::parse("toggle_block"), ArchiveMode::ToggleBlock);
        assert_eq!(ArchiveMode::parse("ban"), ArchiveMode::Block);
        assert_eq!(ArchiveMode::parse("restore"), ArchiveMode::Unarchive);
    }

    #[tokio::test]
    async fn archive_toggle_wechselt_archived_state() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_archive_toggle").await;

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('toggler', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Toggle → archivieren
        let ok = archive_streamer(&pool, "toggler", ArchiveMode::Toggle)
            .await
            .unwrap();
        assert!(ok);
        let archived: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT admin_archived_at FROM twitch_partners WHERE twitch_login='toggler'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(archived.is_some(), "erster Toggle → archiviert");

        // Toggle → reaktivieren
        let ok = archive_streamer(&pool, "toggler", ArchiveMode::Toggle)
            .await
            .unwrap();
        assert!(ok);
        let archived2: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT admin_archived_at FROM twitch_partners WHERE twitch_login='toggler'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(archived2.is_none(), "zweiter Toggle → reaktiviert");
    }

    #[tokio::test]
    async fn set_discord_flag_setzt_is_on_discord() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_discord_flag").await;

        add_streamer(&pool, "testuser", None).await.unwrap();
        let ok = set_discord_flag(&pool, "testuser", true).await.unwrap();
        assert!(ok);

        let row: (Option<i32>,) = sqlx::query_as(
            "SELECT is_on_discord FROM twitch_streamers WHERE LOWER(twitch_login) = 'testuser'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, Some(1));
    }

    #[tokio::test]
    async fn set_discord_profile_setzt_felder() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_discord_profile").await;

        add_streamer(&pool, "profileuser", None).await.unwrap();
        let ok = set_discord_profile(
            &pool,
            "profileuser",
            Some("123456789"),
            Some("TestName"),
            true,
        )
        .await
        .unwrap();
        assert!(ok);

        let row: (Option<String>, Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT discord_user_id, discord_display_name, is_on_discord FROM twitch_streamers WHERE LOWER(twitch_login) = 'profileuser'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0.as_deref(), Some("123456789"));
        assert_eq!(row.1.as_deref(), Some("TestName"));
        assert_eq!(row.2, Some(1));
    }

    #[tokio::test]
    async fn stats_gibt_leere_aggregation_bei_leerer_db() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_stats_empty").await;

        let row = stats(&pool, None, None, None).await.unwrap();
        assert_eq!(row.total_sessions, Some(0));
    }

    #[tokio::test]
    async fn streamer_analytics_gibt_aggregation_zurueck() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_analytics").await;

        sqlx::query(
            "INSERT INTO twitch_stream_sessions
                (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers)
             VALUES ('analyticsuser', NOW() - INTERVAL '1 hour', NOW(), 3600, 100.0, 200)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (agg, sessions) = streamer_analytics(&pool, "analyticsuser", 30)
            .await
            .unwrap();
        assert_eq!(agg.total_sessions, Some(1));
        assert!(!sessions.is_empty());
    }

    #[tokio::test]
    async fn session_detail_gibt_none_fuer_unbekannte_id() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_sc_session_nf").await;

        let result = session_detail(&pool, 99999).await.unwrap();
        assert!(result.is_none());
    }
}
