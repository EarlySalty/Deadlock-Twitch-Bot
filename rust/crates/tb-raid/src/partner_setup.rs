use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

pub fn now_iso(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%S%.6f+00:00").to_string()
}

pub fn normalize_discord_user_id(raw: Option<&str>) -> Option<String> {
    let candidate = raw.unwrap_or("").trim();
    if !candidate.is_empty() && candidate.chars().all(|c| c.is_ascii_digit()) {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn bool_int(value: Option<i64>, default: i32) -> i32 {
    match value {
        None => default,
        Some(v) => i32::from(v != 0),
    }
}

const HARD_PAUSE_REASONS: [&str; 2] = ["blocked", "bot_banned"];

fn hard_pause_reason(technical_pause_reason: Option<&str>) -> Option<String> {
    let normalized = technical_pause_reason?.trim().to_lowercase();
    HARD_PAUSE_REASONS
        .contains(&normalized.as_str())
        .then_some(normalized)
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[derive(Debug)]
pub enum PartnerSetupError {
    InvalidIdentity,
    Db(sqlx::Error),
    SignupBlockLookupFailed,
}

impl std::fmt::Display for PartnerSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentity => write!(f, "twitch_login_and_user_id_required"),
            Self::Db(e) => write!(f, "db error: {e}"),
            Self::SignupBlockLookupFailed => write!(f, "signup_block_lookup_failed"),
        }
    }
}

impl std::error::Error for PartnerSetupError {}

impl From<sqlx::Error> for PartnerSetupError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

#[derive(sqlx::FromRow, Debug)]
struct IdentityRow {
    discord_user_id: Option<String>,
    discord_display_name: Option<String>,
}

#[derive(sqlx::FromRow, Debug)]
struct SourceStreamerRow {
    created_at: Option<String>,
}

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
    technical_pause_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotedIdentity {
    pub twitch_login: String,
    pub twitch_user_id: String,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub is_on_discord: Option<i32>,
    pub reactivated: bool,
    pub hard_pause_reason: Option<String>,
    pub signup_block: Option<tb_domain::SignupBlock>,
    pub guild_active: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct PartnerSyncOutcome {
    pub discord_user_id: Option<String>,
    pub signup_block: Option<tb_domain::SignupBlock>,
    pub reactivated: bool,
    pub hard_pause_reason: Option<String>,
}

pub const COMMUNITY_GUILD_ID: i64 = 1_289_721_245_281_292_288;
pub const STREAMER_GUILD_SOURCE_ONBOARDING: &str = "onboarding";
pub const STREAMER_GUILD_SOURCE_MIGRATION: &str = "migration";
pub const STREAMER_GUILD_SOURCE_ADMIN: &str = "admin";

#[derive(Debug, Clone)]
pub struct PromotePartnerArgs {
    pub twitch_login: String,
    pub twitch_user_id: String,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub is_on_discord: i32,
    pub activate_partner_features: bool,
    pub clear_source: bool,
    pub guild_id: Option<i64>,
}

async fn load_streamer_identity(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<IdentityRow>, sqlx::Error> {
    sqlx::query_as::<_, IdentityRow>(
        r#"
        SELECT discord_user_id, discord_display_name
        FROM twitch_streamer_identities
        WHERE ($1 <> '' AND twitch_user_id = $1)
           OR ($2 <> '' AND NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(twitch_login) = $2)
        ORDER BY CASE WHEN twitch_user_id = $1 THEN 0 ELSE 1 END, updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(twitch_user_id.trim())
    .bind(normalized_or_empty(twitch_login))
    .fetch_optional(pool)
    .await
}

fn normalized_or_empty(login: &str) -> String {
    tb_domain::normalize_twitch_login(login).unwrap_or_default()
}

async fn load_streamer_source_row(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<SourceStreamerRow>, sqlx::Error> {
    sqlx::query_as::<_, SourceStreamerRow>(
        r#"
        SELECT created_at::text AS created_at
        FROM twitch_streamers
        WHERE ($1 <> '' AND twitch_user_id = $1)
           OR ($2 <> '' AND NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(twitch_login) = $2)
        ORDER BY
            CASE WHEN $1 <> '' AND twitch_user_id = $1 THEN 0 ELSE 1 END,
            LOWER(twitch_login)
        LIMIT 1
        "#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(&mut **tx)
    .await
}

async fn load_active_partner_row(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<ActivePartnerRow>, sqlx::Error> {
    sqlx::query_as::<_, ActivePartnerRow>(
        r#"
        SELECT
            p.id,
            p.require_discord_link,
            p.last_description,
            p.last_link_ok,
            p.added_by,
            p.last_link_checked_at,
            p.manual_partner_opt_out,
            p.raid_bot_enabled,
            p.silent_ban,
            p.silent_raid,
            p.live_ping_role_id,
            COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
            p.partnered_at,
            p.technical_pause_reason
        FROM twitch_partners p
        WHERE (($1 <> '' AND p.twitch_user_id = $1)
            OR ($2 <> '' AND NULLIF(TRIM(p.twitch_user_id), '') IS NULL AND LOWER(p.twitch_login) = $2))
          AND p.status = 'active'
        ORDER BY CASE WHEN p.twitch_user_id = $1 THEN 0 ELSE 1 END, p.id DESC
        LIMIT 1
        "#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(&mut **tx)
    .await
}

async fn load_reactivatable_partner_row(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<ActivePartnerRow>, sqlx::Error> {
    sqlx::query_as::<_, ActivePartnerRow>(
        r#"
        SELECT
            p.id,
            p.require_discord_link,
            p.last_description,
            p.last_link_ok,
            p.added_by,
            p.last_link_checked_at,
            p.manual_partner_opt_out,
            p.raid_bot_enabled,
            p.silent_ban,
            p.silent_raid,
            p.live_ping_role_id,
            COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled,
            p.partnered_at,
            p.technical_pause_reason
        FROM twitch_partners p
        WHERE (($1 <> '' AND p.twitch_user_id = $1)
            OR ($2 <> '' AND NULLIF(TRIM(p.twitch_user_id), '') IS NULL AND LOWER(p.twitch_login) = $2))
          AND p.status <> 'active'
        ORDER BY CASE WHEN p.twitch_user_id = $1 THEN 0 ELSE 1 END, p.id DESC
        LIMIT 1
        "#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(&mut **tx)
    .await
}

async fn load_inactive_block_reason(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<Option<String>> = sqlx::query_scalar(
        r#"
        SELECT technical_pause_reason
          FROM twitch_partners
         WHERE (($1 <> '' AND twitch_user_id = $1)
             OR ($2 <> '' AND NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(twitch_login) = $2))
           AND status <> 'active'
           AND LOWER(TRIM(COALESCE(technical_pause_reason, ''))) = ANY(ARRAY['blocked', 'bot_banned'])
         ORDER BY id DESC
         LIMIT 1
        "#,
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.and_then(|pause_reason| hard_pause_reason(pause_reason.as_deref())))
}

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
           OR (NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(twitch_login) = LOWER($1))
        "#,
        r#"
        UPDATE streamer_plans
        SET twitch_login = $1,
            twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), $2)
        WHERE twitch_user_id = $2
           OR (NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(COALESCE(twitch_login, '')) = LOWER($1))
        "#,
        r#"
        UPDATE twitch_partner_raid_scores
        SET twitch_login = $1,
            twitch_user_id = COALESCE(NULLIF(twitch_user_id, ''), $2)
        WHERE twitch_user_id = $2
           OR (NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(COALESCE(twitch_login, '')) = LOWER($1))
        "#,
        r#"
        UPDATE twitch_live_state
        SET streamer_login = $1
        WHERE twitch_user_id = $2
           OR (NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(streamer_login) = LOWER($1))
        "#,
    ];
    for (idx, sql) in statements.iter().enumerate() {
        let savepoint = format!("partner_setup_norm_{idx}");
        sqlx::query(sqlx::AssertSqlSafe(format!("SAVEPOINT {savepoint}")))
            .execute(&mut **tx)
            .await?;
        let result = sqlx::query(*sql)
            .bind(twitch_login)
            .bind(twitch_user_id)
            .execute(&mut **tx)
            .await;
        match result {
            Ok(_) => {
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "RELEASE SAVEPOINT {savepoint}"
                )))
                .execute(&mut **tx)
                .await?;
            }
            Err(e) => {
                tracing::warn!(
                    statement = idx,
                    "normalize_related_tables: Statement fehlgeschlagen (übersprungen): {e}"
                );
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "ROLLBACK TO SAVEPOINT {savepoint}"
                )))
                .execute(&mut **tx)
                .await?;
            }
        }
    }
    Ok(())
}

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

async fn session_tags(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<Option<Vec<String>>, sqlx::Error> {
    let user_id = twitch_user_id.trim();
    let login = twitch_login.trim().to_lowercase();
    if user_id.is_empty() || login.is_empty() {
        return Ok(None);
    }
    let rows: Vec<(Option<String>,)> = sqlx::query_as(
        r#"
        SELECT tags FROM twitch_stream_sessions
         WHERE COALESCE(tags, '') <> ''
           AND (
                twitch_user_id = $1
                OR (NULLIF(twitch_user_id, '') IS NULL AND lower(streamer_login) = $2)
           )
        "#,
    )
    .bind(user_id)
    .bind(&login)
    .fetch_all(pool)
    .await?;
    let mut tags: Vec<String> = rows
        .into_iter()
        .filter_map(|(raw,)| raw)
        .flat_map(|raw| parse_session_tags(&raw))
        .collect();
    if tags.is_empty() {
        return Ok(None);
    }
    tags.sort();
    tags.dedup();
    Ok(Some(tags))
}

fn parse_session_tags(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    if raw.starts_with('[') {
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_str(raw) {
            return items
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
        }
    }
    raw.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) async fn lock_partner_configuration(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Result<bool, sqlx::Error> {
    let stored_ids: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_partners WHERE twitch_user_id = $1 OR LOWER(twitch_login) = $2 ORDER BY twitch_user_id, ctid FOR UPDATE",
    )
    .bind(twitch_user_id)
    .bind(twitch_login)
    .fetch_all(&mut **tx)
    .await?;
    Ok(stored_ids
        .iter()
        .any(|stored| non_empty(stored.as_deref()).is_some_and(|stored| stored != twitch_user_id)))
}

pub(crate) async fn partner_configuration_inactive(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    twitch_login: &str,
    allow_technical_recovery: bool,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM twitch_partners WHERE (twitch_user_id = $1 OR (NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(twitch_login) = $3)) AND (status <> 'active' OR COALESCE(manual_partner_opt_out, 0) <> 0 OR admin_archived_at IS NOT NULL OR departnered_at IS NOT NULL OR (NULLIF(TRIM(technical_pause_reason), '') IS NOT NULL AND (NOT $2 OR LOWER(TRIM(technical_pause_reason)) NOT LIKE 'token_error%'))))",
    )
    .bind(twitch_user_id)
    .bind(allow_technical_recovery)
    .bind(twitch_login)
    .fetch_one(&mut **tx)
    .await
}

pub async fn attach_streamer_guild(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    guild_id: i64,
    active: bool,
    source: &str,
) -> Result<(), sqlx::Error> {
    let user_id = twitch_user_id.trim();
    if user_id.is_empty() || guild_id <= 0 {
        return Ok(());
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('partner_signup'), hashtext($1::text))")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    let source = match source {
        STREAMER_GUILD_SOURCE_MIGRATION => STREAMER_GUILD_SOURCE_MIGRATION,
        STREAMER_GUILD_SOURCE_ADMIN => STREAMER_GUILD_SOURCE_ADMIN,
        _ => STREAMER_GUILD_SOURCE_ONBOARDING,
    };
    if source == STREAMER_GUILD_SOURCE_ADMIN && !active {
        sqlx::query(
            "UPDATE streamer_guilds SET active = FALSE, deactivated_by = $3, deactivated_at = NOW() WHERE twitch_user_id = $1 AND guild_id = $2 AND (active OR deactivated_by IS DISTINCT FROM $3)",
        )
        .bind(user_id)
        .bind(guild_id)
        .bind(source)
        .execute(&mut **tx)
        .await?;
        return Ok(());
    }
    sqlx::query(
        r#"
        INSERT INTO streamer_guilds (
            twitch_user_id, guild_id, active, source, deactivated_by, deactivated_at
        )
        SELECT $1, g.guild_id,
               $3 AND ($4 <> 'onboarding' OR g.auto_partner_default), $4,
               CASE WHEN $3 AND ($4 <> 'onboarding' OR g.auto_partner_default) THEN NULL ELSE $4 END,
               CASE WHEN $3 AND ($4 <> 'onboarding' OR g.auto_partner_default) THEN NULL ELSE NOW() END
          FROM discord_guilds g
         WHERE g.guild_id = $2 AND g.status = 'active' AND g.removed_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM twitch_exclusions e
                WHERE e.twitch_user_id = $1 AND e.reactivated_at IS NULL
           )
           AND NOT EXISTS (
               SELECT 1 FROM twitch_partner_signup_denylist d WHERE d.twitch_user_id = $1
           )
           AND NOT EXISTS (
               SELECT 1 FROM twitch_partners p
                WHERE p.twitch_user_id = $1
                  AND LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) IN ('blocked', 'bot_banned')
           )
        ON CONFLICT (twitch_user_id, guild_id) DO UPDATE SET
            active = EXCLUDED.active,
            deactivated_by = EXCLUDED.deactivated_by,
            deactivated_at = EXCLUDED.deactivated_at
        WHERE $4 = 'admin'
        "#,
    )
    .bind(user_id)
    .bind(guild_id)
    .bind(active)
    .bind(source)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn restore_legacy_technical_membership(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE streamer_guilds sg
           SET active = TRUE, deactivated_by = NULL, deactivated_at = NULL
          FROM discord_guilds g
         WHERE sg.twitch_user_id = $1 AND sg.guild_id = $2
           AND g.guild_id = sg.guild_id AND g.status = 'active' AND g.removed_at IS NULL
           AND NOT sg.active AND sg.source = 'migration'
           AND sg.deactivated_by = 'migration' AND sg.deactivated_at IS NOT NULL
           AND EXISTS (
               SELECT 1 FROM twitch_partners p
                WHERE p.twitch_user_id = $1 AND p.status = 'active'
                  AND COALESCE(p.manual_partner_opt_out, 0) = 0
                  AND p.admin_archived_at IS NULL AND p.departnered_at IS NULL
                  AND LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) LIKE 'token_error%'
           )
           AND NOT EXISTS (
               SELECT 1 FROM twitch_exclusions e
                WHERE e.twitch_user_id = $1 AND e.reactivated_at IS NULL
           )
           AND NOT EXISTS (
               SELECT 1 FROM twitch_partner_signup_denylist d
                WHERE d.twitch_user_id = $1
                   OR LOWER(d.twitch_login) IN (SELECT LOWER(twitch_login) FROM twitch_partners WHERE twitch_user_id = $1)
           )
           AND NOT EXISTS (
               SELECT 1 FROM twitch_partners p
                WHERE p.twitch_user_id = $1
                  AND (COALESCE(p.manual_partner_opt_out, 0) <> 0
                       OR p.admin_archived_at IS NOT NULL OR p.departnered_at IS NOT NULL
                       OR p.status <> 'active'
                       OR LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) IN ('blocked', 'bot_banned'))
           )
        "#,
    )
    .bind(twitch_user_id)
    .bind(COMMUNITY_GUILD_ID)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn promote_admitted_streamer(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    now: DateTime<Utc>,
) -> Result<PromotedIdentity, PartnerSetupError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('partner_signup'), hashtext($1::text))")
        .bind(twitch_user_id.trim())
        .execute(&mut **tx)
        .await?;
    let identity: Option<(String, Option<String>, Option<String>, i32)> = sqlx::query_as(
        "SELECT twitch_login, discord_user_id, discord_display_name, COALESCE(is_on_discord, 0) FROM twitch_streamer_identities WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id.trim())
    .fetch_optional(&mut **tx)
    .await?;
    let Some((twitch_login, discord_user_id, discord_display_name, is_on_discord)) = identity
    else {
        return Err(PartnerSetupError::InvalidIdentity);
    };
    let existing: Option<i32> = sqlx::query_scalar(
        "SELECT CASE WHEN is_partner_active = 1 AND operational_state = 'active' THEN 1 ELSE 0 END FROM twitch_partners_all_state WHERE twitch_user_id = $1 ORDER BY is_partner_active DESC LIMIT 1",
    )
    .bind(twitch_user_id.trim())
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(active) = existing {
        return Ok(PromotedIdentity {
            twitch_login,
            twitch_user_id: twitch_user_id.trim().to_string(),
            discord_user_id,
            discord_display_name,
            is_on_discord: Some(is_on_discord),
            reactivated: active == 1,
            hard_pause_reason: (active != 1).then(|| "partner_configuration_inactive".to_string()),
            signup_block: None,
            guild_active: None,
        });
    }
    promote_streamer_to_partner(
        tx,
        &PromotePartnerArgs {
            twitch_login,
            twitch_user_id: twitch_user_id.trim().to_string(),
            discord_user_id,
            discord_display_name,
            is_on_discord,
            activate_partner_features: true,
            clear_source: true,
            guild_id: None,
        },
        now,
    )
    .await
}

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

    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('partner_signup'), hashtext($1::text))")
        .bind(&normalized_user_id)
        .execute(&mut **tx)
        .await?;

    if lock_partner_configuration(tx, &normalized_user_id, &normalized_login).await? {
        return Err(PartnerSetupError::InvalidIdentity);
    }

    match crate::signup_denylist::lookup_or_fail_closed(
        &mut **tx,
        Some(&normalized_user_id),
        &normalized_login,
        "promote",
    )
    .await
    {
        Ok(Some(block)) => {
            return Ok(PromotedIdentity {
                twitch_login: normalized_login,
                twitch_user_id: normalized_user_id,
                discord_user_id: None,
                discord_display_name: None,
                is_on_discord: None,
                reactivated: false,
                hard_pause_reason: Some(tb_domain::PROMOTE_BLOCK_REASON.to_string()),
                signup_block: Some(block),
                guild_active: None,
            });
        }
        Ok(None) => {}
        Err(_) => return Err(PartnerSetupError::SignupBlockLookupFailed),
    }

    let exclusion: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM twitch_exclusions WHERE twitch_user_id = $1 AND reactivated_at IS NULL",
    )
    .bind(&normalized_user_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(kind) = exclusion {
        tracing::warn!(twitch_user_id = %normalized_user_id, %kind, "Partner-Aufnahme durch Ausschluss blockiert");
        return Ok(PromotedIdentity {
            twitch_login: normalized_login,
            twitch_user_id: normalized_user_id,
            discord_user_id: None,
            discord_display_name: None,
            is_on_discord: None,
            reactivated: false,
            hard_pause_reason: Some(kind),
            signup_block: None,
            guild_active: None,
        });
    }

    let source_row = load_streamer_source_row(tx, &normalized_user_id, &normalized_login).await?;
    let active_row = load_active_partner_row(tx, &normalized_user_id, &normalized_login).await?;
    let active = active_row.as_ref();

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
            signup_block: None,
            guild_active: None,
        });
    }

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
            signup_block: None,
            guild_active: None,
        });
    }

    let global_deactivation = partner_configuration_inactive(
        tx,
        &normalized_user_id,
        &normalized_login,
        args.activate_partner_features,
    )
    .await?;
    if global_deactivation {
        return Ok(PromotedIdentity {
            twitch_login: normalized_login,
            twitch_user_id: normalized_user_id,
            discord_user_id: None,
            discord_display_name: None,
            is_on_discord: None,
            reactivated: false,
            hard_pause_reason: Some("partner_configuration_inactive".to_string()),
            signup_block: None,
            guild_active: None,
        });
    }

    let active_row = match active_row {
        Some(row) => Some(row),
        None => load_reactivatable_partner_row(tx, &normalized_user_id, &normalized_login).await?,
    };
    let active = active_row.as_ref();
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

    let mut guild_active = None;
    let mut admission_reason = None;
    if args.activate_partner_features
        && (args.guild_id.is_none() || args.guild_id == Some(COMMUNITY_GUILD_ID))
    {
        restore_legacy_technical_membership(tx, &normalized_user_id).await?;
    }
    if let Some(guild_id) = args.guild_id {
        let installed: Option<i64> = sqlx::query_scalar(
            "SELECT guild_id FROM discord_guilds WHERE guild_id = $1 AND status = 'active' AND removed_at IS NULL FOR SHARE",
        )
        .bind(guild_id)
        .fetch_optional(&mut **tx)
        .await?;
        if installed.is_none() {
            admission_reason = Some("guild_unavailable");
        } else {
            attach_streamer_guild(
                tx,
                &normalized_user_id,
                guild_id,
                true,
                STREAMER_GUILD_SOURCE_ONBOARDING,
            )
            .await?;
            guild_active = sqlx::query_scalar(
                "SELECT active FROM streamer_guilds WHERE twitch_user_id = $1 AND guild_id = $2 FOR UPDATE",
            )
            .bind(&normalized_user_id)
            .bind(guild_id)
            .fetch_optional(&mut **tx)
            .await?;
            if guild_active != Some(true) {
                admission_reason = Some("guild_inactive");
            }
        }
    } else {
        let admitted: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM streamer_guilds sg JOIN discord_guilds g ON g.guild_id = sg.guild_id WHERE sg.twitch_user_id = $1 AND sg.active AND g.status = 'active' AND g.removed_at IS NULL)",
        )
        .bind(&normalized_user_id)
        .fetch_one(&mut **tx)
        .await?;
        if !admitted {
            admission_reason = Some("guild_verification_required");
        }
    }
    if let Some(reason) = admission_reason {
        tracing::warn!(
            twitch_user_id = %normalized_user_id,
            guild_id = ?args.guild_id,
            reason,
            "Partner-Aufnahme nicht aktiviert"
        );
        return Ok(PromotedIdentity {
            twitch_login: normalized_login,
            twitch_user_id: normalized_user_id,
            discord_user_id: identity_discord_user_id,
            discord_display_name: identity_display_name,
            is_on_discord: identity_is_on_discord,
            reactivated: false,
            hard_pause_reason: Some(reason.to_string()),
            signup_block: None,
            guild_active,
        });
    }

    let require_discord_link = bool_int(
        active.and_then(|a| a.require_discord_link).map(i64::from),
        0,
    );
    let last_description = active.and_then(|a| a.last_description.clone());
    let last_link_ok = active.and_then(|a| a.last_link_ok);
    let added_by = active.and_then(|a| a.added_by.clone());
    let last_link_checked_at = active.and_then(|a| a.last_link_checked_at.clone());
    let next_link_check_at: Option<String> = None;
    let (manual_partner_opt_out, raid_bot_enabled) = if args.activate_partner_features
        && active.is_none_or(|row| {
            row.technical_pause_reason
                .as_deref()
                .is_some_and(|reason| reason.trim().to_lowercase().starts_with("token_error"))
        }) {
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

    sqlx::query!(
        r#"
        DELETE FROM twitch_raid_blacklist
        WHERE target_id = $1
          AND reason LIKE 'confirmed_external_recruitment_limit%'
        "#,
        &normalized_user_id
    )
    .execute(&mut **tx)
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
                raid_bot_enabled = CASE WHEN raid_admin_enabled THEN $9 ELSE 0 END,
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

    if active.is_none() {
        crate::streamer_referrals::credit_first_activation(
            tx,
            &normalized_user_id,
            &normalized_login,
            now,
        )
        .await?;
    }

    normalize_related_tables(tx, &normalized_user_id, &normalized_login).await?;

    if args.clear_source {
        sqlx::query(
            r#"
            DELETE FROM twitch_streamers
            WHERE twitch_user_id = $1
               OR (NULLIF(TRIM(twitch_user_id), '') IS NULL AND LOWER(twitch_login) = LOWER($2))
            "#,
        )
        .bind(&normalized_user_id)
        .bind(&normalized_login)
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
        signup_block: None,
        guild_active,
    })
}

#[async_trait]
pub trait DiscordDirectoryPort: Send + Sync {
    async fn resolve_display_name(&self, discord_user_id: &str) -> Option<String>;
    async fn grant_streamer_role(&self, discord_user_id: &str, reason: &str);

    async fn revoke_streamer_role(&self, discord_user_id: &str, _reason: &str) {
        tracing::debug!(
            "revoke_streamer_role für {discord_user_id} ist in dieser DiscordDirectoryPort-Implementierung ein No-op"
        );
    }
}

#[async_trait]
pub trait ModeratorInstallPort: Send + Sync {
    async fn add_channel_moderator(
        &self,
        broadcaster_id: &str,
        bot_user_id: &str,
        streamer_access_token: &str,
    ) -> Result<(), String>;
}

#[async_trait]
pub trait ChatGreeterPort: Send + Sync {
    async fn send_partner_chat_message(
        &self,
        twitch_login: &str,
        message: &str,
    ) -> Result<bool, String>;
}

#[async_trait]
pub trait SignupTagEnforcePort: Send + Sync {
    async fn enforce_session_tags(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        tags: &[String],
    ) -> Result<(), sqlx::Error>;
}

const GREETING_MESSAGES: [&str; 3] = [
    "Deadlock Chatbot Guard verbunden! 🎮",
    "Commands für alle: !ping (Bot-Status) | !clip [beschreibung] (Clip erstellen) | !raid_history (letzte Raids)",
    "Mod-Commands: !raid / !traid (Raid starten) | !raid_status (Bot-Status) | !uban / !unban (letzten Auto-Ban aufheben) | !silentban / !silentraid (Benachrichtigungen an/aus)",
];
const GREETING_MESSAGES_EN: [&str; 3] = [
    "Deadlock Chatbot Guard connected! 🎮",
    "Commands for everyone: !ping (bot status) | !clip [description] (create a clip) | !raid_history (recent raids)",
    "Mod commands: !raid / !traid (start a raid) | !raid_status (bot status) | !uban / !unban (undo last auto-ban) | !silentban / !silentraid (notifications on/off)",
];

fn greeting_messages(lang: tb_domain::UiLang) -> [&'static str; 3] {
    match lang {
        tb_domain::UiLang::De => GREETING_MESSAGES,
        tb_domain::UiLang::En => GREETING_MESSAGES_EN,
    }
}

pub struct PartnerSetupService {
    pool: PgPool,
    discord: Arc<dyn DiscordDirectoryPort>,
    moderator: Arc<dyn ModeratorInstallPort>,
    greeter: Arc<dyn ChatGreeterPort>,
    signup_tag_block: Option<Arc<dyn SignupTagEnforcePort>>,
    bot_user_id: Option<String>,
    greeting_initial_pause: Duration,
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
            signup_tag_block: None,
            bot_user_id,
            greeting_initial_pause: Duration::from_secs(2),
            greeting_message_pause: Duration::from_secs(1),
        }
    }

    pub fn with_signup_tag_block(mut self, port: Arc<dyn SignupTagEnforcePort>) -> Self {
        self.signup_tag_block = Some(port);
        self
    }

    pub fn with_pauses(mut self, initial: Duration, between: Duration) -> Self {
        self.greeting_initial_pause = initial;
        self.greeting_message_pause = between;
        self
    }

    pub async fn sync_partner_state_after_auth(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        state_discord_user_id: Option<&str>,
        activate_partner_features: bool,
    ) -> Result<PartnerSyncOutcome, PartnerSetupError> {
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

        if let Some(port) = self.signup_tag_block.as_ref() {
            if let Some(tags) = session_tags(&self.pool, twitch_user_id, twitch_login).await? {
                port.enforce_session_tags(twitch_user_id, twitch_login, &tags)
                    .await?;
            }
        }
        let promoted = {
            let mut tx = self.pool.begin().await?;
            let promoted = promote_streamer_to_partner(
                &mut tx,
                &PromotePartnerArgs {
                    twitch_login: twitch_login.to_string(),
                    twitch_user_id: twitch_user_id.to_string(),
                    discord_user_id: final_discord_id.clone(),
                    discord_display_name: final_display_name,
                    is_on_discord: is_on_discord_value,
                    activate_partner_features,
                    clear_source: true,
                    guild_id: None,
                },
                Utc::now(),
            )
            .await?;
            tx.commit().await?;
            promoted
        };

        if !promoted.reactivated {
            tracing::warn!(
                twitch_user_id,
                twitch_login,
                reason = ?promoted.hard_pause_reason,
                "Partner-Sync ohne Aktivierung, keine Rolle und kein Backfill"
            );
            return Ok(PartnerSyncOutcome {
                discord_user_id: None,
                signup_block: promoted.signup_block,
                reactivated: false,
                hard_pause_reason: promoted.hard_pause_reason,
            });
        }

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
        Ok(PartnerSyncOutcome {
            discord_user_id: final_discord_id,
            signup_block: None,
            reactivated: true,
            hard_pause_reason: None,
        })
    }

    pub async fn complete_setup_for_streamer(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        streamer_access_token: &str,
        state_discord_user_id: Option<&str>,
    ) -> Result<(), PartnerSetupError> {
        tracing::info!("Completing setup for streamer {twitch_login} ({twitch_user_id})");

        let partner_sync_result = self
            .sync_partner_state_after_auth(
                twitch_user_id,
                twitch_login,
                state_discord_user_id,
                true,
            )
            .await;
        if let Err(e) = &partner_sync_result {
            tracing::error!("sync_partner_state_after_auth failed for {twitch_login}: {e}");
        }

        let outcome = partner_sync_result?;
        if !outcome.reactivated {
            tracing::warn!(
                twitch_user_id,
                twitch_login,
                reason = ?outcome.hard_pause_reason,
                "Partner-Setup ohne Aktivierung beendet"
            );
            return Ok(());
        }

        record_first_login(&self.pool, twitch_user_id, twitch_login).await;

        let Some(ref bot_user_id) = self.bot_user_id else {
            tracing::warn!(
                "complete_setup: Keine Bot-ID verfügbar — Moderator-Setup und Begrüßung entfallen für {twitch_login}"
            );
            return Ok(());
        };

        if let Err(error) = sqlx::query(
            "UPDATE twitch_partners SET deadlock_pause_unmodded_at = NULL WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, "complete_setup: Deadlock-Pause-Marker nicht loeschbar für {twitch_login}");
        }
        let _ = self
            .moderator
            .add_channel_moderator(twitch_user_id, bot_user_id, streamer_access_token)
            .await;

        tokio::time::sleep(self.greeting_initial_pause).await;
        let mut greeted = true;
        let stream_language: Option<String> = sqlx::query_scalar(
            "SELECT language FROM twitch_channel_updates
              WHERE twitch_user_id = $1
                AND NULLIF(BTRIM(language), '') IS NOT NULL
              ORDER BY id DESC
              LIMIT 1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let lang = tb_domain::UiLang::resolve(tb_domain::UiLangHints {
            guild_id: None,
            guild_language: None,
            stream_language: stream_language.as_deref(),
        });
        for (idx, message) in greeting_messages(lang).iter().enumerate() {
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_messages_follow_language() {
        assert!(greeting_messages(tb_domain::UiLang::De)[0].contains("verbunden"));
        assert!(greeting_messages(tb_domain::UiLang::En)[0].contains("connected"));
        assert!(!greeting_messages(tb_domain::UiLang::En)
            .iter()
            .any(|msg| msg.contains('\u{2014}')));
    }

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

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    mod referral_test_support {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test-support/streamer_referrals.rs"
        ));
    }

    async fn testpool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_database::database_url()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            r#"CREATE TABLE twitch_partners (
                id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, twitch_login TEXT,
                require_discord_link INTEGER, last_description TEXT, last_link_ok INTEGER,
                added_by TEXT, last_link_checked_at TEXT, next_link_check_at TEXT,
                manual_partner_opt_out INTEGER, raid_admin_enabled BOOLEAN NOT NULL DEFAULT TRUE, raid_bot_enabled INTEGER, silent_ban INTEGER,
                silent_raid INTEGER, live_ping_role_id BIGINT, live_ping_enabled INTEGER,
                partnered_at TEXT, departnered_at TEXT, status TEXT, admin_archived_at TEXT,
                technical_pause_reason TEXT
            )"#,
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, discord_user_id TEXT,
                discord_display_name TEXT, is_on_discord INTEGER, created_at TEXT, updated_at TEXT
            )"#,
            r#"CREATE TABLE twitch_streamers (
                id BIGSERIAL PRIMARY KEY, twitch_login TEXT UNIQUE NOT NULL,
                twitch_user_id TEXT, created_at TIMESTAMPTZ DEFAULT NOW()
            )"#,
            r#"CREATE TABLE twitch_partner_signup_denylist (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT NOT NULL, reason TEXT NOT NULL,
                public_message TEXT, added_by TEXT NOT NULL, added_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )"#,
            r#"CREATE TABLE twitch_raid_blacklist (
                target_login TEXT PRIMARY KEY, target_id TEXT, reason TEXT, added_at TEXT
            )"#,
            r#"CREATE TABLE twitch_raid_auth (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT)"#,
            r#"CREATE TABLE streamer_plans (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, first_login_at TEXT
            )"#,
            r#"CREATE TABLE twitch_partner_raid_scores (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT
            )"#,
            r#"CREATE TABLE twitch_live_state (
                twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL
            )"#,
            r#"CREATE TABLE twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL,
                twitch_user_id TEXT, tags TEXT
            )"#,
            r#"CREATE TABLE discord_guilds (
                guild_id BIGINT PRIMARY KEY CHECK (guild_id > 0),
                name TEXT NOT NULL DEFAULT '',
                language TEXT NOT NULL DEFAULT 'en',
                announce_channel_id BIGINT,
                alert_channel_id BIGINT,
                streamer_role_id BIGINT,
                ping_role_id BIGINT,
                promo_invite_url TEXT,
                auto_partner_default BOOLEAN NOT NULL DEFAULT TRUE,
                cross_server_raids BOOLEAN NOT NULL DEFAULT TRUE,
                ai_features BOOLEAN NOT NULL DEFAULT FALSE,
                status TEXT NOT NULL DEFAULT 'active',
                installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                removed_at TIMESTAMPTZ,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_by TEXT
            )"#,
            r#"CREATE TABLE streamer_guilds (
                twitch_user_id TEXT NOT NULL,
                guild_id BIGINT NOT NULL REFERENCES discord_guilds(guild_id) ON DELETE CASCADE,
                active BOOLEAN NOT NULL DEFAULT TRUE,
                deactivated_by TEXT,
                deactivated_at TIMESTAMPTZ,
                joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                source TEXT NOT NULL DEFAULT 'onboarding',
                PRIMARY KEY (twitch_user_id, guild_id),
                CHECK (active OR deactivated_at IS NOT NULL)
            )"#,
            r#"CREATE TABLE twitch_exclusions (
                twitch_user_id TEXT PRIMARY KEY, kind TEXT NOT NULL, reactivated_at TIMESTAMPTZ
            )"#,
            r#"INSERT INTO discord_guilds (guild_id, name, language)
                VALUES (1289721245281292288, 'Deutsche Deadlock Community', 'de')"#,
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        referral_test_support::schema(&pool).await;
        Some(pool)
    }

    fn promote_args(login: &str, uid: &str) -> PromotePartnerArgs {
        PromotePartnerArgs {
            twitch_login: login.to_string(),
            twitch_user_id: uid.to_string(),
            discord_user_id: None,
            discord_display_name: None,
            is_on_discord: 0,
            activate_partner_features: true,
            clear_source: true,
            guild_id: Some(COMMUNITY_GUILD_ID),
        }
    }

    #[tokio::test]
    async fn session_tags_nimmt_id_und_login_fallback_ohne_fremde_id() {
        let Some(pool) = testpool("ps_session_tags").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, tags) VALUES
             ('beispiel', '42', 'English'),
             ('Beispiel', NULL, 'Deutsch'),
             ('beispiel', '', 'SoloQ'),
             ('beispiel', '99', 'Fremd')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let tags = session_tags(&pool, "42", "BEISPIEL")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            tags,
            vec![
                "Deutsch".to_string(),
                "English".to_string(),
                "SoloQ".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn promote_loescht_recruitment_blacklist_und_bewahrt_hard_grund() {
        let Some(pool) = testpool("ps_bl_cleanup_ok").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_login, target_id, reason, added_at) VALUES
             ('ismile_e', '58819840', 'confirmed_external_recruitment_limit_grace_expired: count=4 limit=4', '2026-04-24T13:14:33+00:00'),
             ('ismile_e_hard', '58819840', 'partner_raid_bot_banned: kein Zugriff', '2026-05-01T00:00:00+00:00'),
             ('fremd', '99999999', 'confirmed_external_recruitment_limit_grace_expired: count=9 limit=4', '2026-04-01T00:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result =
            promote_streamer_to_partner(&mut tx, &promote_args("ismile_e", "58819840"), Utc::now())
                .await
                .unwrap();
        tx.commit().await.unwrap();
        assert!(result.reactivated, "Promotion muss die Guards passieren");

        let reste: Vec<String> = sqlx::query_scalar(
            "SELECT target_login FROM twitch_raid_blacklist ORDER BY target_login",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            reste,
            vec!["fremd".to_string(), "ismile_e_hard".to_string()],
            "recruitment-Zeile des Partners geloescht, Hard-Grund und fremde ID bleiben"
        );
    }

    #[tokio::test]
    async fn signup_block_noop_loescht_recruitment_blacklist_nicht() {
        let Some(pool) = testpool("ps_bl_cleanup_noop").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_partner_signup_denylist (twitch_user_id, twitch_login, reason, added_by)
             VALUES ('58819840', 'ismile_e', 'owner_decision:test', 'test')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_login, target_id, reason, added_at)
             VALUES ('ismile_e', '58819840', 'confirmed_external_recruitment_limit_grace_expired: count=4 limit=4', '2026-04-24T13:14:33+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result =
            promote_streamer_to_partner(&mut tx, &promote_args("ismile_e", "58819840"), Utc::now())
                .await
                .unwrap();
        tx.commit().await.unwrap();
        assert!(
            result.signup_block.is_some(),
            "Signup-Block muss die No-op-Abweisung ausloesen"
        );
        assert!(!result.reactivated);

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_raid_blacklist \
             WHERE target_id = '58819840' AND reason LIKE 'confirmed_external_recruitment_limit%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "No-op-Abweisung darf nichts loeschen");
    }
}
