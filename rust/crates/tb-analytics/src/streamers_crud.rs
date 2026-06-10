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
/// Hinweis: Nur für bereits aktive Partner (`status = 'active'`). Eine Promotion von
/// `twitch_streamers` → `twitch_partners` ist Bestandteil eines späteren Schritts.
pub async fn verify_streamer(
    pool: &PgPool,
    login: &str,
) -> Result<VerifyStreamerResult, sqlx::Error> {
    let updated = sqlx::query(
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
    .await?;

    if updated.rows_affected() > 0 {
        Ok(VerifyStreamerResult::Verified)
    } else {
        Ok(VerifyStreamerResult::NotAPartner)
    }
}

// ── POST /streamers/{login}/archive ──────────────────────────────────────────

/// Archivierungs-/Block-Modus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveMode {
    Archive,
    Unarchive,
    Block,
    Unblock,
}

impl ArchiveMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "archive" | "on" | "set" => Some(Self::Archive),
            "unarchive" | "off" | "unset" | "restore" => Some(Self::Unarchive),
            "block" | "blocked" | "ban" => Some(Self::Block),
            "unblock" | "allow" => Some(Self::Unblock),
            _ => None,
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
///
/// Gibt `false` zurück wenn kein Eintrag gefunden wurde (→ 404).
///
/// PostgreSQL-spezifisch: `UPDATE ... FROM`-Syntax im discord_flag/profile-Code.
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("DB-Verbindung");

        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema");

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
                streamer_login          TEXT,
                game_name               TEXT,
                had_deadlock_in_session BOOLEAN DEFAULT FALSE,
                started_at              TIMESTAMPTZ,
                ended_at                TIMESTAMPTZ
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sc_list_empty").await;
        let result = list_streamers(&pool, "Deadlock").await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn add_fuegt_streamer_ein() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sc_remove_nf").await;

        let result = remove_streamer(&pool, "nichtvorhanden").await.unwrap();
        assert!(matches!(result, RemoveStreamerResult::NotFound));
    }

    #[tokio::test]
    async fn verify_setzt_is_verified_bei_aktivem_partner() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sc_verify").await;

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('testpartner', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = verify_streamer(&pool, "testpartner").await.unwrap();
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
    async fn verify_gibt_not_a_partner_fuer_unbekannten_login() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sc_verify_nap").await;

        let result = verify_streamer(&pool, "niemand").await.unwrap();
        assert!(matches!(result, VerifyStreamerResult::NotAPartner));
    }

    #[tokio::test]
    async fn set_discord_flag_setzt_is_on_discord() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
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
}
