//! Partner-Setup nach erfolgreicher OAuth-Autorisierung.
//!
//! Zeilengenauer Port von
//! `bot/raid/services/partner_setup_service.py` (`sync_partner_state_after_auth`
//! Z. 136, `_record_first_login` Z. 211, `complete_setup_for_streamer` Z. 391)
//! und `bot/storage/partner_registry.py::promote_streamer_to_partner` (Z. 782)
//! inkl. Helfer (`load_streamer_identity` Z. 455, `upsert_streamer_identity`
//! Z. 499, `_normalize_related_tables` Z. 726, `_load_streamer_row` Z. 130)
//! sowie `bot/storage/pg.py::backfill_tracked_stats_from_category` (Z. 1404).
//!
//! # Prod-Schema (Dump 11.6.)
//!
//! `twitch_partners`: alle Timestamp-Spalten sind **TEXT** (ISO-Strings aus
//! Python `datetime.now(UTC).isoformat()`), Flags **INTEGER** (0/1),
//! `live_ping_role_id` **BIGINT**, `id` **BIGSERIAL**.
//! `twitch_streamer_identities.created_at/updated_at`: TEXT, geschrieben via
//! `CURRENT_TIMESTAMP` (DB-seitiger Assignment-Cast — identisch zu Python).
//! `twitch_streamers` (Quelle, nach Partner-DB-Konsolidierung): nur noch
//! Identitäts-Spalten; die früheren Partner-Spalten existieren nicht mehr.
//!
//! # Bewusste Abweichung von Python (Bugfix, 12.6.)
//!
//! Python `promote_streamer_to_partner` setzt bei nicht übergebenen Parametern
//! `require_discord_link`/`silent_ban`/`silent_raid` immer auf 0,
//! `live_ping_role_id` auf NULL und `live_ping_enabled` auf 1 — weil die
//! `_row_value`-Aufrufe auf die Quell-Zeile mit Literal-Spalten (0/NULL/1)
//! bzw. Nicht-None-Defaults arbeiten und der eigentlich vorgesehene Fallback
//! auf den aktiven Partner (`default=_bool_int(_row_value(active_row, ...))`)
//! dadurch toter Code ist. Folge in Prod: Jede Re-Autorisierung wipet die
//! Partner-Einstellungen. Dieser Port stellt die offensichtliche Intention
//! her: Bei UNSET fallen die fünf Felder auf den Wert des aktiven Partners
//! zurück (Erst-Promotion ohne aktive Zeile verhält sich identisch zu Python).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

// ---------------------------------------------------------------------------
// Helfer
// ---------------------------------------------------------------------------

/// Python `datetime.now(UTC).isoformat()` — ISO-8601 mit Mikrosekunden und
/// `+00:00`-Suffix, z. B. `2026-06-12T14:23:45.123456+00:00`.
pub fn now_iso(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
}

/// Python `normalize_discord_user_id` (`bot/discord_role_sync.py:62`):
/// trim, nur Ziffern erlaubt, sonst None.
pub fn normalize_discord_user_id(raw: Option<&str>) -> Option<String> {
    let candidate = raw.unwrap_or("").trim();
    if !candidate.is_empty() && candidate.chars().all(|c| c.is_ascii_digit()) {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// Python `_bool_int(value, default)`: None → default, sonst 0/1 je Truthiness.
fn bool_int(value: Option<i64>, default: i32) -> i32 {
    match value {
        None => default,
        Some(v) => i32::from(v != 0),
    }
}

/// Python `_HARD_PAUSE_REASONS = frozenset({"blocked", "bot_banned"})`:
/// Hard-Kills, die ein OAuth-Followup NICHT aufheben darf.
const HARD_PAUSE_REASONS: [&str; 2] = ["blocked", "bot_banned"];

/// Normalisierter Hard-Kill-Grund (`strip().lower()`), falls die Pause ein
/// Hard-Kill ist; sonst `None`. Python `pause_reason in _HARD_PAUSE_REASONS`.
fn hard_pause_reason(technical_pause_reason: Option<&str>) -> Option<String> {
    let normalized = technical_pause_reason?.trim().to_lowercase();
    HARD_PAUSE_REASONS
        .contains(&normalized.as_str())
        .then_some(normalized)
}

/// Trim + leere Strings zu None (Python `str(x or "").strip() or None`).
fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Fehler
// ---------------------------------------------------------------------------

/// Fehler im Partner-Setup-Fluss.
#[derive(Debug)]
pub enum PartnerSetupError {
    /// Python `ValueError("twitch_login_and_user_id_required")`.
    InvalidIdentity,
    /// Datenbankfehler.
    Db(sqlx::Error),
}

impl std::fmt::Display for PartnerSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentity => write!(f, "twitch_login_and_user_id_required"),
            Self::Db(e) => write!(f, "db error: {e}"),
        }
    }
}

impl std::error::Error for PartnerSetupError {}

impl From<sqlx::Error> for PartnerSetupError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

// ---------------------------------------------------------------------------
// Zeilen-Typen
// ---------------------------------------------------------------------------

/// Identitäts-Zeile aus `twitch_streamer_identities` (Python
/// `load_streamer_identity` — genutzt werden nur Discord-Verknüpfung + Name).
#[derive(sqlx::FromRow, Debug)]
struct IdentityRow {
    discord_user_id: Option<String>,
    discord_display_name: Option<String>,
}

/// Quell-Zeile aus `twitch_streamers` (Python `_load_streamer_row`).
///
/// Prod enthält hier nur Twitch-Identität + `created_at`; Discord-Identität
/// liegt in `twitch_streamer_identities` und wird vor der Promotion geladen.
#[derive(sqlx::FromRow, Debug)]
struct SourceStreamerRow {
    /// `created_at` ist TIMESTAMPTZ — als `::text` selektiert, damit der
    /// `partnered_at`-Fallback dasselbe DB-Rendering erhält wie Pythons
    /// datetime-Bind in eine TEXT-Spalte.
    created_at: Option<String>,
}

/// Aktive Partner-Zeile (Python `_load_partner_row` mit status='active').
///
/// Die Identitäts-Spalten des Python-SELECTs (JOIN auf
/// `twitch_streamer_identities`) entfallen: Sie dienen dort nur als Fallback
/// für UNSET-Identitäts-Kwargs — der Followup-Pfad übergibt die Identität
/// immer explizit.
#[derive(sqlx::FromRow, Debug)]
struct ActivePartnerRow {
    id: i64,
    require_discord_link: Option<i32>,
    last_description: Option<String>,
    last_link_ok: Option<i32>,
    added_by: Option<String>,
    last_link_checked_at: Option<String>,
    manual_partner_opt_out: Option<i32>,
    raid_bot_enabled: Option<i32>,
    silent_ban: Option<i32>,
    silent_raid: Option<i32>,
    live_ping_role_id: Option<i64>,
    live_ping_enabled: Option<i32>,
    partnered_at: Option<String>,
    /// B11-PR-7: Hard-Kill-Grund. `{blocked, bot_banned}` darf NICHT durch einen
    /// OAuth-Followup aufgehoben werden (Python `_HARD_PAUSE_REASONS`).
    technical_pause_reason: Option<String>,
}

/// Ergebnis der Promotion (Python-Rückgabe-Dict von
/// `promote_streamer_to_partner`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotedIdentity {
    pub twitch_login: String,
    pub twitch_user_id: String,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub is_on_discord: Option<i32>,
    /// B11-PR-7: `false`, wenn die Promotion durch einen Hard-Kill
    /// (`technical_pause_reason` ∈ {blocked, bot_banned}) als No-op abgewiesen
    /// wurde (Python `reactivate_partner_after_valid_auth` → `reactivated: False`).
    pub reactivated: bool,
    /// Gesetzter Hard-Kill-Grund bei abgewiesener Promotion, sonst `None`
    /// (Python-Rückgabefeld `reason`).
    pub hard_pause_reason: Option<String>,
}

/// Argumente für `promote_streamer_to_partner` — exakt die Parameter, die der
/// OAuth-Followup-Pfad übergibt (`partner_setup_service.py:172-201`); alle
/// übrigen Python-Kwargs bleiben UNSET und laufen über die Zeilen-Fallbacks.
#[derive(Debug, Clone)]
pub struct PromotePartnerArgs {
    pub twitch_login: String,
    pub twitch_user_id: String,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub is_on_discord: i32,
    /// Steuert `manual_partner_opt_out=0` + `raid_bot_enabled=1`.
    pub activate_partner_features: bool,
    /// Python-Default `True`: Quelle nach Promotion aus `twitch_streamers`
    /// löschen.
    pub clear_source: bool,
}

// ---------------------------------------------------------------------------
// DB-Primitive
// ---------------------------------------------------------------------------

/// Python `load_streamer_identity` (Discord-Zweig leer — der Followup-Pfad
/// fragt immer per user_id + login an).
async fn load_streamer_identity(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<IdentityRow>, sqlx::Error> {
    sqlx::query_as!(
        IdentityRow,
        r#"
        SELECT discord_user_id AS "discord_user_id?",
               discord_display_name AS "discord_display_name?"
        FROM twitch_streamer_identities
        WHERE ($1 <> '' AND twitch_user_id = $1)
           OR ($2 <> '' AND LOWER(twitch_login) = $2)
        ORDER BY updated_at DESC
        LIMIT 1
        "#,
        twitch_user_id.trim(),
        normalized_or_empty(twitch_login)
    )
    .fetch_optional(pool)
    .await
}

/// Python `_normalize_login`: `normalize_twitch_login(login) or ""`.
fn normalized_or_empty(login: &str) -> String {
    tb_domain::normalize_twitch_login(login).unwrap_or_default()
}

async fn load_streamer_source_row(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<SourceStreamerRow>, sqlx::Error> {
    sqlx::query_as!(
        SourceStreamerRow,
        r#"
        SELECT created_at::text AS "created_at?"
        FROM twitch_streamers
        WHERE ($1 <> '' AND twitch_user_id = $1)
           OR ($2 <> '' AND LOWER(twitch_login) = $2)
        ORDER BY
            CASE WHEN $1 <> '' AND twitch_user_id = $1 THEN 0 ELSE 1 END,
            LOWER(twitch_login)
        LIMIT 1
        "#,
        twitch_user_id,
        twitch_login
    )
    .fetch_optional(&mut **tx)
    .await
}

async fn load_active_partner_row(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<ActivePartnerRow>, sqlx::Error> {
    sqlx::query_as!(
        ActivePartnerRow,
        r#"
        SELECT
            p.id AS "id!",
            p.require_discord_link AS "require_discord_link?",
            p.last_description AS "last_description?",
            p.last_link_ok AS "last_link_ok?",
            p.added_by AS "added_by?",
            p.last_link_checked_at AS "last_link_checked_at?",
            p.manual_partner_opt_out AS "manual_partner_opt_out?",
            p.raid_bot_enabled AS "raid_bot_enabled?",
            p.silent_ban AS "silent_ban?",
            p.silent_raid AS "silent_raid?",
            p.live_ping_role_id AS "live_ping_role_id?",
            COALESCE(p.live_ping_enabled, 1) AS "live_ping_enabled?",
            p.partnered_at AS "partnered_at?",
            p.technical_pause_reason AS "technical_pause_reason?"
        FROM twitch_partners p
        WHERE (($1 <> '' AND p.twitch_user_id = $1)
            OR ($2 <> '' AND LOWER(p.twitch_login) = $2))
          AND p.status = 'active'
        ORDER BY p.id DESC
        LIMIT 1
        "#,
        twitch_user_id,
        twitch_login
    )
    .fetch_optional(&mut **tx)
    .await
}

/// Prüft, ob eine **nicht-aktive** Partner-Zeile den OAuth-Followup dauerhaft
/// blockiert: `technical_pause_reason` ∈ {blocked, bot_banned}.
///
/// Dient als zweite Schranke in `promote_streamer_to_partner`: der
/// aktive-Zeile-Guard (`hard_pause_reason` auf `active_row`) deckt nur den Fall
/// ab, in dem noch eine aktive Zeile existiert. Ist der Partner hingegen
/// ausgeschieden (status ≠ 'active'), gibt es keine aktive Zeile, und ein
/// Re-OAuth darf nur administrative Hard-Kills nicht umgehen. Reine
/// `admin_archived_at`-Historie blockiert Reauth nicht.
async fn load_inactive_block_reason(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<Option<String>> = sqlx::query_scalar!(
        r#"
        SELECT technical_pause_reason AS "technical_pause_reason?"
          FROM twitch_partners
         WHERE (($1 <> '' AND twitch_user_id = $1)
             OR ($2 <> '' AND LOWER(twitch_login) = $2))
           AND status <> 'active'
           AND LOWER(TRIM(COALESCE(technical_pause_reason, ''))) = ANY(ARRAY['blocked', 'bot_banned'])
         ORDER BY id DESC
         LIMIT 1
        "#,
        twitch_user_id,
        twitch_login
    )
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.and_then(|pause_reason| hard_pause_reason(pause_reason.as_deref())))
}

/// Python `upsert_streamer_identity` (`partner_registry.py:499`).
async fn upsert_streamer_identity(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
    discord_user_id: Option<&str>,
    discord_display_name: Option<&str>,
    is_on_discord: Option<i32>,
) -> Result<(), sqlx::Error> {
    let normalized_user_id = twitch_user_id.trim();
    let normalized_login = normalized_or_empty(twitch_login);
    if normalized_user_id.is_empty() || normalized_login.is_empty() {
        return Ok(());
    }
    let normalized_discord = non_empty(discord_user_id);
    let normalized_display = non_empty(discord_display_name);
    // Python: `_bool_int(is_on_discord, default=1 if discord_id else 0)
    //          if (is_on_discord is not None or discord_id) else None`
    let is_on_discord_value: Option<i32> =
        if is_on_discord.is_some() || normalized_discord.is_some() {
            Some(bool_int(
                is_on_discord.map(i64::from),
                i32::from(normalized_discord.is_some()),
            ))
        } else {
            None
        };

    if let Some(ref discord_id) = normalized_discord {
        sqlx::query!(
            r#"
            UPDATE twitch_streamer_identities
            SET discord_user_id = NULL,
                discord_display_name = NULL,
                is_on_discord = 0,
                updated_at = CURRENT_TIMESTAMP
            WHERE discord_user_id = $1
              AND twitch_user_id <> $2
            "#,
            discord_id,
            normalized_user_id
        )
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query!(
        r#"
        INSERT INTO twitch_streamer_identities (
            twitch_user_id, twitch_login, discord_user_id, discord_display_name,
            is_on_discord, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, COALESCE($5, 0), CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            twitch_login = EXCLUDED.twitch_login,
            discord_user_id = COALESCE(EXCLUDED.discord_user_id, twitch_streamer_identities.discord_user_id),
            discord_display_name = COALESCE(EXCLUDED.discord_display_name, twitch_streamer_identities.discord_display_name),
            is_on_discord = COALESCE(EXCLUDED.is_on_discord, twitch_streamer_identities.is_on_discord),
            updated_at = CURRENT_TIMESTAMP
        "#,
        normalized_user_id,
        &normalized_login,
        normalized_discord.as_deref(),
        normalized_display.as_deref(),
        is_on_discord_value
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Python `_normalize_related_tables`: vier UPDATEs, Fehler je Statement
/// werden geloggt und übersprungen. In einer Postgres-Transaktion würde ein
/// Fehler die Transaktion abbrechen — deshalb Savepoint pro Statement.
async fn normalize_related_tables(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<(), sqlx::Error> {
    if twitch_user_id.is_empty() || twitch_login.is_empty() {
        return Ok(());
    }
    let statements: [&str; 4] = [
        r#"
        UPDATE twitch_raid_auth
        SET twitch_login = $1,
            twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), $2)
        WHERE twitch_user_id = $2
           OR LOWER(twitch_login) = LOWER($1)
        "#,
        r#"
        UPDATE streamer_plans
        SET twitch_login = $1,
            twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), $2)
        WHERE twitch_user_id = $2
           OR LOWER(COALESCE(twitch_login, '')) = LOWER($1)
        "#,
        r#"
        UPDATE twitch_partner_raid_scores
        SET twitch_login = $1,
            twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), $2)
        WHERE twitch_user_id = $2
           OR LOWER(COALESCE(twitch_login, '')) = LOWER($1)
        "#,
        r#"
        UPDATE twitch_live_state
        SET streamer_login = $1
        WHERE twitch_user_id = $2
           OR LOWER(streamer_login) = LOWER($1)
        "#,
    ];
    for (idx, sql) in statements.iter().enumerate() {
        let savepoint = format!("partner_setup_norm_{idx}");
        sqlx::query(&format!("SAVEPOINT {savepoint}"))
            .execute(&mut **tx)
            .await?;
        let result = sqlx::query(sql)
            .bind(twitch_login)
            .bind(twitch_user_id)
            .execute(&mut **tx)
            .await;
        match result {
            Ok(_) => {
                sqlx::query(&format!("RELEASE SAVEPOINT {savepoint}"))
                    .execute(&mut **tx)
                    .await?;
            }
            Err(e) => {
                tracing::warn!(
                    statement = idx,
                    "normalize_related_tables: Statement fehlgeschlagen (übersprungen): {e}"
                );
                sqlx::query(&format!("ROLLBACK TO SAVEPOINT {savepoint}"))
                    .execute(&mut **tx)
                    .await?;
            }
        }
    }
    Ok(())
}

/// Python `backfill_tracked_stats_from_category` (`pg.py:1404`) — idempotent
/// via NOT-EXISTS-Guard. Gibt die Anzahl kopierter Zeilen zurück.
///
/// Intern genutzt über [`backfill_tracked_stats_best_effort`], damit ein Fehler
/// hier nie die Partner-Promotion-Transaktion zurückrollt.
async fn backfill_tracked_stats_from_category(
    tx: &mut Transaction<'_, Postgres>,
    login: &str,
) -> Result<u64, sqlx::Error> {
    let normalized = login.trim().to_lowercase();
    if normalized.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query!(
        r#"
        INSERT INTO twitch_stats_tracked
            (ts_utc, streamer, viewer_count, is_partner, game_name, stream_title, tags)
        SELECT c.ts_utc, c.streamer, c.viewer_count, c.is_partner,
               c.game_name, c.stream_title, c.tags
          FROM twitch_stats_category c
         WHERE LOWER(c.streamer) = $1
           AND NOT EXISTS (
               SELECT 1
                 FROM twitch_stats_tracked t
                WHERE LOWER(t.streamer) = LOWER(c.streamer)
                  AND t.ts_utc = c.ts_utc
           )
        "#,
        &normalized
    )
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected())
}

/// Best-Effort-Wrapper: startet eine eigene Transaktion, damit ein Backfill-Fehler
/// die bereits committete Partner-Promotion nicht berührt.
async fn backfill_tracked_stats_best_effort(pool: &PgPool, login: &str) -> u64 {
    let mut tx = match pool.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("backfill_tracked_stats: pool.begin() fehlgeschlagen für {login}: {e}");
            return 0;
        }
    };
    match backfill_tracked_stats_from_category(&mut tx, login).await {
        Ok(n) => {
            if let Err(e) = tx.commit().await {
                tracing::warn!("backfill_tracked_stats: commit fehlgeschlagen für {login}: {e}");
                0
            } else {
                n
            }
        }
        Err(e) => {
            tracing::warn!("backfill_tracked_stats_from_category nicht-fatal für {login}: {e}");
            0
        }
    }
}

/// Python `_record_first_login` (`partner_setup_service.py:211`):
/// COALESCE bewahrt den ersten Timestamp, vollständig idempotent.
/// Fehler werden geloggt, nicht propagiert (Python-Parität).
pub async fn record_first_login(pool: &PgPool, twitch_user_id: &str, twitch_login: &str) {
    let now = now_iso(Utc::now());
    let result = sqlx::query!(
        r#"
        INSERT INTO streamer_plans (twitch_user_id, twitch_login, first_login_at)
        VALUES ($1, $2, $3)
        ON CONFLICT (twitch_user_id) DO UPDATE SET
            first_login_at = COALESCE(streamer_plans.first_login_at, EXCLUDED.first_login_at)
        "#,
        twitch_user_id,
        twitch_login,
        &now
    )
    .execute(pool)
    .await;
    match result {
        Ok(_) => tracing::info!("Recorded first_login_at for {twitch_login} ({twitch_user_id})"),
        Err(e) => tracing::error!(
            "Failed to record first_login_at for {twitch_login} ({twitch_user_id}): {e}"
        ),
    }
}

// ---------------------------------------------------------------------------
// promote_streamer_to_partner
// ---------------------------------------------------------------------------

/// Python `promote_streamer_to_partner` (`partner_registry.py:782`) für den
/// OAuth-Followup-Parametersatz. Läuft vollständig in der übergebenen
/// Transaktion (Promotion + Identity-Upsert + Normalisierung + Clear-Source).
pub async fn promote_streamer_to_partner(
    tx: &mut Transaction<'_, Postgres>,
    args: &PromotePartnerArgs,
    now: DateTime<Utc>,
) -> Result<PromotedIdentity, PartnerSetupError> {
    let normalized_login = normalized_or_empty(&args.twitch_login);
    let normalized_user_id = args.twitch_user_id.trim().to_string();
    if normalized_login.is_empty() || normalized_user_id.is_empty() {
        return Err(PartnerSetupError::InvalidIdentity);
    }

    let source_row = load_streamer_source_row(tx, &normalized_user_id, &normalized_login).await?;
    let active_row = load_active_partner_row(tx, &normalized_user_id, &normalized_login).await?;
    let active = active_row.as_ref();

    // B11-PR-7: Hard-Pause-Guard — aktive Zeile (Python `reactivate_partner_after_valid_auth`,
    // `partner_registry.py:1366`). Ein OAuth-Followup darf einen Hard-Kill
    // ({blocked, bot_banned}) NICHT reaktivieren — sonst würde der UPDATE unten
    // `technical_pause_reason = NULL` setzen und den Bann bedingungslos aufheben.
    // No-op: keine Schreibzugriffe, Pause + Deaktivierung bleiben unangetastet.
    if let Some(reason) =
        hard_pause_reason(active.and_then(|a| a.technical_pause_reason.as_deref()))
    {
        return Ok(PromotedIdentity {
            twitch_login: normalized_login,
            twitch_user_id: normalized_user_id,
            discord_user_id: None,
            discord_display_name: None,
            is_on_discord: None,
            reactivated: false,
            hard_pause_reason: Some(reason),
        });
    }

    // Nicht-aktive-Zeile-Guard: ausgeschiedene / archivierte / gebannte Partner
    // haben keine aktive Zeile mehr — der Guard oben greift dort nicht.
    // Ein Re-OAuth darf NICHT heimlich eine neue aktive Zeile anlegen.
    if let Some(reason) =
        load_inactive_block_reason(tx, &normalized_user_id, &normalized_login).await?
    {
        tracing::warn!(
            login = %normalized_login,
            %reason,
            "promote_streamer_to_partner: blockiert durch nicht-aktive Partner-Zeile (ausgeschieden/gebannt)"
        );
        return Ok(PromotedIdentity {
            twitch_login: normalized_login,
            twitch_user_id: normalized_user_id,
            discord_user_id: None,
            discord_display_name: None,
            is_on_discord: None,
            reactivated: false,
            hard_pause_reason: Some(reason),
        });
    }

    // Partner-Werte: explizite Parameter des Followup-Pfads + Zeilen-Fallbacks.
    // Für require_discord_link/silent_ban/silent_raid/live_ping_* gilt der im
    // Modul-Header dokumentierte Bugfix: aktiven Wert bewahren statt wipen.
    let require_discord_link = bool_int(
        active.and_then(|a| a.require_discord_link).map(i64::from),
        0,
    );
    let last_description = active.and_then(|a| a.last_description.clone());
    let last_link_ok = active.and_then(|a| a.last_link_ok);
    let added_by = active.and_then(|a| a.added_by.clone());
    let last_link_checked_at = active.and_then(|a| a.last_link_checked_at.clone());
    // Quelle in twitch_streamers gedroppt → Python-Literal NULL.
    let next_link_check_at: Option<String> = None;
    let (manual_partner_opt_out, raid_bot_enabled) = if args.activate_partner_features {
        (0i32, 1i32)
    } else {
        (
            bool_int(
                active.and_then(|a| a.manual_partner_opt_out).map(i64::from),
                0,
            ),
            bool_int(active.and_then(|a| a.raid_bot_enabled).map(i64::from), 0),
        )
    };
    let silent_ban = bool_int(active.and_then(|a| a.silent_ban).map(i64::from), 0);
    let silent_raid = bool_int(active.and_then(|a| a.silent_raid).map(i64::from), 0);
    let live_ping_role_id = active.and_then(|a| a.live_ping_role_id);
    let live_ping_enabled = bool_int(active.and_then(|a| a.live_ping_enabled).map(i64::from), 1);

    // Identität kommt aus expliziten Followup-Parametern. Der Sync-Pfad liest
    // bestehende Werte vorher aus `twitch_streamer_identities`; `twitch_streamers`
    // trägt in Prod keine Discord-Spalten mehr.
    let identity_discord_user_id = non_empty(args.discord_user_id.as_deref());
    let identity_display_name = non_empty(args.discord_display_name.as_deref());
    let identity_is_on_discord = Some(bool_int(Some(i64::from(args.is_on_discord)), 0));

    upsert_streamer_identity(
        tx,
        &normalized_user_id,
        &normalized_login,
        identity_discord_user_id.as_deref(),
        identity_display_name.as_deref(),
        identity_is_on_discord,
    )
    .await?;

    let effective_partnered_at = active
        .and_then(|a| a.partnered_at.clone())
        .or_else(|| source_row.as_ref().and_then(|s| s.created_at.clone()))
        .unwrap_or_else(|| now_iso(now));

    if let Some(active) = active {
        sqlx::query!(
            r#"
            UPDATE twitch_partners
            SET twitch_login = $1,
                require_discord_link = $2,
                last_description = $3,
                last_link_ok = $4,
                added_by = $5,
                last_link_checked_at = $6,
                next_link_check_at = $7,
                manual_partner_opt_out = $8,
                raid_bot_enabled = $9,
                silent_ban = $10,
                silent_raid = $11,
                live_ping_role_id = $12,
                live_ping_enabled = $13,
                partnered_at = $14,
                admin_archived_at = NULL,
                departnered_at = NULL,
                technical_pause_reason = NULL,
                status = 'active'
            WHERE id = $15
            "#,
            &normalized_login,
            require_discord_link,
            last_description.as_deref(),
            last_link_ok,
            added_by.as_deref(),
            last_link_checked_at.as_deref(),
            next_link_check_at.as_deref(),
            manual_partner_opt_out,
            raid_bot_enabled,
            silent_ban,
            silent_raid,
            live_ping_role_id,
            live_ping_enabled,
            &effective_partnered_at,
            active.id
        )
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query!(
            r#"
            INSERT INTO twitch_partners (
                twitch_user_id, twitch_login, require_discord_link, last_description,
                last_link_ok, added_by, last_link_checked_at, next_link_check_at,
                manual_partner_opt_out, raid_bot_enabled, silent_ban, silent_raid,
                live_ping_role_id, live_ping_enabled, partnered_at,
                admin_archived_at, departnered_at, technical_pause_reason, status
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                      $14, $15, NULL, NULL, NULL, 'active')
            "#,
            &normalized_user_id,
            &normalized_login,
            require_discord_link,
            last_description.as_deref(),
            last_link_ok,
            added_by.as_deref(),
            last_link_checked_at.as_deref(),
            next_link_check_at.as_deref(),
            manual_partner_opt_out,
            raid_bot_enabled,
            silent_ban,
            silent_raid,
            live_ping_role_id,
            live_ping_enabled,
            &effective_partnered_at
        )
        .execute(&mut **tx)
        .await?;
    }

    normalize_related_tables(tx, &normalized_user_id, &normalized_login).await?;

    if args.clear_source {
        sqlx::query!(
            r#"
            DELETE FROM twitch_streamers
            WHERE twitch_user_id = $1
               OR LOWER(twitch_login) = LOWER($2)
            "#,
            &normalized_user_id,
            &normalized_login
        )
        .execute(&mut **tx)
        .await?;
    }

    Ok(PromotedIdentity {
        twitch_login: normalized_login,
        twitch_user_id: normalized_user_id,
        discord_user_id: identity_discord_user_id,
        discord_display_name: identity_display_name,
        is_on_discord: identity_is_on_discord,
        reactivated: true,
        hard_pause_reason: None,
    })
}

// ---------------------------------------------------------------------------
// Ports für externe Seiteneffekte
// ---------------------------------------------------------------------------

/// Discord-Seiteneffekte (Python: lokaler Discord-Bot; Rust: Master-Broker).
#[async_trait]
pub trait DiscordDirectoryPort: Send + Sync {
    /// Python `resolve_discord_display_name`: bevorzugt global_name →
    /// display_name → name; None bei Fehlern.
    async fn resolve_display_name(&self, discord_user_id: &str) -> Option<String>;
    /// Python `sync_streamer_role(should_have_role=True)` — Fehler werden
    /// von der Implementierung geloggt, nie propagiert.
    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str);

    /// Python `sync_streamer_role(should_have_role=False)`: entzieht die
    /// Streamer-Rolle wieder (Departner/Deautorisierung). Fehler werden NUR
    /// geloggt, nie propagiert — kein Hard-Fail (B10).
    ///
    /// Default: No-op mit Warnung. Implementierungen ohne Broker-Anbindung
    /// (Tests, interne-API-Pfad) müssen das nicht überschreiben; der
    /// Broker-Adapter ([`BrokerDiscordDirectory`] im tb-bot-Bin) tut es.
    async fn revoke_streamer_role(&self, discord_user_id: &str, _reason: &str) {
        tracing::debug!(
            "revoke_streamer_role für {discord_user_id} ist in dieser DiscordDirectoryPort-Implementierung ein No-op"
        );
    }
}

/// Moderator-Einsetzung via Helix (Python `complete_setup` Schritt 4).
/// Die Implementierung behandelt 200/204 (Erfolg), 422 und 400+"already a mod"
/// (bereits Mod) sowie alle übrigen Fälle (Warning) selbst — kein Fehler nach
/// außen (Python-Parität: nur Logging, kein Abbruch).
#[async_trait]
pub trait ModeratorInstallPort: Send + Sync {
    async fn add_channel_moderator(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        streamer_access_token: &str,
    );
}

/// Chat-Nachricht in einen Partner-Kanal senden. Interim: Delegation an den
/// Python-Chat-Prozess via interner API (`POST /streamers/{login}/chat-action`)
/// bis zum Chat-Cutover (Welle B). `Ok(false)` = Chat-Bot nicht verfügbar.
#[async_trait]
pub trait ChatGreeterPort: Send + Sync {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        message: &str,
    ) -> Result<bool, String>;
}

// ---------------------------------------------------------------------------
// PartnerSetupService
// ---------------------------------------------------------------------------

/// Begrüßungsnachrichten (Python `partner_setup_service.py:481-524`, wörtlich).
const GREETING_MESSAGES: [&str; 3] = [
    "Deadlock Chatbot Guard verbunden! 🎮",
    "Commands für alle: !ping (Bot-Status) | !clip [beschreibung] (Clip erstellen) | !raid_history (letzte Raids)",
    "Mod-Commands: !raid / !traid (Raid starten) | !raid_status (Bot-Status) | !uban / !unban (letzten Auto-Ban aufheben) | !silentban / !silentraid (Benachrichtigungen an/aus)",
];

/// Orchestriert die OAuth-Followups (Python `PartnerSetupService`).
///
/// `stream_went_live_fn` (Python Schritt 6: sofortige stream.offline-Sub wenn
/// gerade live) ist bewusst NICHT portiert: Der native Poll-Loop (Default 15 s,
/// `poll_interval_seconds`) nimmt frisch promotete Partner beim nächsten Tick
/// auf — die Python-Begründung war die 15–45-Minuten-Lücke des Python-Pollers.
pub struct PartnerSetupService {
    pool: PgPool,
    discord: Arc<dyn DiscordDirectoryPort>,
    moderator: Arc<dyn ModeratorInstallPort>,
    greeter: Arc<dyn ChatGreeterPort>,
    /// Python `self._bot_id() or TWITCH_BOT_USER_ID`; None → Moderator- und
    /// Chat-Schritt entfallen (früher Return wie Python).
    bot_user_id: Option<String>,
    /// Python `asyncio.sleep(2)` vor der ersten Nachricht.
    greeting_initial_pause: Duration,
    /// Python `asyncio.sleep(1)` zwischen den Nachrichten.
    greeting_message_pause: Duration,
}

impl PartnerSetupService {
    pub fn new(
        pool: PgPool,
        discord: Arc<dyn DiscordDirectoryPort>,
        moderator: Arc<dyn ModeratorInstallPort>,
        greeter: Arc<dyn ChatGreeterPort>,
        bot_user_id: Option<String>,
    ) -> Self {
        Self {
            pool,
            discord,
            moderator,
            greeter,
            bot_user_id,
            greeting_initial_pause: Duration::from_secs(2),
            greeting_message_pause: Duration::from_secs(1),
        }
    }

    /// Test-Konstruktor ohne reale Wartezeiten.
    pub fn with_pauses(mut self, initial: Duration, between: Duration) -> Self {
        self.greeting_initial_pause = initial;
        self.greeting_message_pause = between;
        self
    }

    /// Python `sync_partner_state_after_auth` (Z. 136): Discord-Verknüpfung
    /// auflösen, Partner promoten, Stats backfillen, Streamer-Rolle setzen.
    /// Gibt die finale Discord-User-ID zurück (oder None).
    pub async fn sync_partner_state_after_auth(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        state_discord_user_id: Option<&str>,
        activate_partner_features: bool,
    ) -> Result<Option<String>, PartnerSetupError> {
        let provided_discord_id = normalize_discord_user_id(state_discord_user_id);

        let identity = load_streamer_identity(&self.pool, twitch_user_id, twitch_login).await?;
        let existing_discord_id = identity
            .as_ref()
            .and_then(|r| normalize_discord_user_id(r.discord_user_id.as_deref()));
        let existing_display_name = identity
            .as_ref()
            .and_then(|r| non_empty(r.discord_display_name.as_deref()));

        let final_discord_id = provided_discord_id.or(existing_discord_id);
        let final_display_name = match (&existing_display_name, &final_discord_id) {
            (Some(name), _) => Some(name.clone()),
            (None, Some(discord_id)) => self.discord.resolve_display_name(discord_id).await,
            (None, None) => None,
        };
        let is_on_discord_value = i32::from(final_discord_id.is_some());

        // Promotion in eigener Transaktion — isoliert von Backfill, damit ein
        // Backfill-Fehler die Partner-Zeile nicht zurückrollt.
        {
            let mut tx = self.pool.begin().await?;
            promote_streamer_to_partner(
                &mut tx,
                &PromotePartnerArgs {
                    twitch_login: twitch_login.to_string(),
                    twitch_user_id: twitch_user_id.to_string(),
                    discord_user_id: final_discord_id.clone(),
                    discord_display_name: final_display_name,
                    is_on_discord: is_on_discord_value,
                    activate_partner_features,
                    clear_source: true,
                },
                Utc::now(),
            )
            .await?;
            tx.commit().await?;
        }

        // Stats-Backfill best-effort in eigener Transaktion (kein Rollback der Promotion).
        let copied = backfill_tracked_stats_best_effort(&self.pool, twitch_login).await;
        if copied > 0 {
            tracing::info!(
                "Backfilled {copied} category samples into tracked for {twitch_login} during partner sync"
            );
        }

        if let Some(ref discord_id) = final_discord_id {
            self.discord
                .grant_streamer_role(discord_id, "Twitch-Bot erfolgreich autorisiert")
                .await;
        }
        Ok(final_discord_id)
    }

    /// Python `complete_setup_for_streamer` (Z. 391) — Erst-Autorisierung:
    /// Partner-Sync, first_login, Moderator-Einsetzung, Chat-Begrüßung.
    /// Jeder Schritt ist fehler-isoliert (Python: eigenes try/except).
    ///
    /// `streamer_access_token` kommt direkt aus dem Token-Exchange des
    /// Callbacks (Python lädt ihn frisch aus der DB — im Callback-Kontext
    /// identisch, der Token wurde Sekunden zuvor gespeichert).
    pub async fn complete_setup_for_streamer(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        streamer_access_token: &str,
        state_discord_user_id: Option<&str>,
    ) {
        tracing::info!("Completing setup for streamer {twitch_login} ({twitch_user_id})");

        // Schritt 1: Partner-State-Sync.
        if let Err(e) = self
            .sync_partner_state_after_auth(
                twitch_user_id,
                twitch_login,
                state_discord_user_id,
                true,
            )
            .await
        {
            tracing::error!("sync_partner_state_after_auth failed for {twitch_login}: {e}");
        }

        // Schritt 2: first_login_at (loggt Fehler selbst).
        record_first_login(&self.pool, twitch_user_id, twitch_login).await;

        // Schritt 3: Bot-ID — ohne sie entfallen Moderator + Begrüßung
        // (Python: früher Return).
        let Some(ref bot_user_id) = self.bot_user_id else {
            tracing::warn!(
                "complete_setup: Keine Bot-ID verfügbar — Moderator-Setup und Begrüßung entfallen für {twitch_login}"
            );
            return;
        };

        // Schritt 4: Bot als Moderator einsetzen (Impl loggt alle Ausgänge).
        self.moderator
            .add_channel_moderator(twitch_user_id, bot_user_id, streamer_access_token)
            .await;

        // Schritt 5: Chat-Begrüßung — delegiert an den Python-Chat-Prozess.
        // Der Channel-Join passiert dort über den periodischen
        // Partner-Join-Loop (alle 30 min) bzw. beim Go-Live; die Nachrichten
        // selbst gehen über den Helix-Pfad und brauchen keinen Join.
        // Python umschließt den ganzen Block mit einem try — der erste Fehler
        // bricht die restlichen Nachrichten ab.
        tokio::time::sleep(self.greeting_initial_pause).await;
        let mut greeted = true;
        for (idx, message) in GREETING_MESSAGES.iter().enumerate() {
            if idx > 0 {
                tokio::time::sleep(self.greeting_message_pause).await;
            }
            match self
                .greeter
                .send_partner_chat_message(twitch_login, message)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        "Chat-Begrüßung für {twitch_login}: Chat-Bot nicht verfügbar — restliche Nachrichten übersprungen"
                    );
                    greeted = false;
                    break;
                }
                Err(e) => {
                    tracing::error!("Error sending auth success message to {twitch_login}: {e}");
                    greeted = false;
                    break;
                }
            }
        }
        if greeted {
            tracing::info!("Sent auth success message to {twitch_login}");
        }

        // Schritt 6 (Python stream_went_live_fn): entfällt — der native
        // Poll-Loop (15 s) registriert stream.offline beim nächsten Tick.
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests (ohne DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_iso_entspricht_python_isoformat() {
        let dt = DateTime::parse_from_rfc3339("2026-06-12T14:23:45.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(now_iso(dt), "2026-06-12T14:23:45.123456+00:00");
    }

    #[test]
    fn normalize_discord_user_id_nur_ziffern() {
        assert_eq!(
            normalize_discord_user_id(Some(" 123456 ")),
            Some("123456".to_string())
        );
        assert_eq!(normalize_discord_user_id(Some("abc123")), None);
        assert_eq!(normalize_discord_user_id(Some("")), None);
        assert_eq!(normalize_discord_user_id(None), None);
    }

    #[test]
    fn bool_int_python_paritaet() {
        assert_eq!(bool_int(None, 7), 7);
        assert_eq!(bool_int(Some(0), 1), 0);
        assert_eq!(bool_int(Some(5), 0), 1);
        assert_eq!(bool_int(Some(-1), 0), 1);
    }

    #[test]
    fn non_empty_trimmt() {
        assert_eq!(non_empty(Some("  x  ")), Some("x".to_string()));
        assert_eq!(non_empty(Some("   ")), None);
        assert_eq!(non_empty(None), None);
    }
}
