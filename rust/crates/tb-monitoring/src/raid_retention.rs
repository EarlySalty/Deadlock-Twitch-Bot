//! Raid-Retention-Berechnung (#11, P1.24).
//!
//! Port von `bot/analytics/mixin.py:compute_raid_retention`. Ein unabhängiger
//! 1h-Loop liest die Raids der letzten 7 Tage und berechnet je Raid, wie viele
//! der gesendeten Zuschauer 5/15/30 Minuten später noch im Ziel-Chat waren —
//! plus Herkunfts-Splits (`known_from_raider`/`new_to_target`/`new_chatters`).
//! Reines SQL gegen den Pool (kein Token).

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tb_chat::WHITELISTED_BOTS;

/// Ein Raid-Datensatz aus `twitch_raid_history`.
#[derive(Debug, Clone)]
struct RaidRow {
    id: i64,
    from_login: String,
    to_login: String,
    viewer_count: i32,
    executed_at: DateTime<Utc>,
}

/// Aggregierte Stats eines Retention-Laufs (Observability).
#[derive(Debug, Default, Clone)]
pub struct RetentionStats {
    pub raids_scanned: usize,
    pub raids_skipped_existing: usize,
    pub raids_skipped_no_session: usize,
    pub raids_computed: usize,
}

/// Berechnet die Retention für alle Raids der letzten 7 Tage und schreibt neue
/// Zeilen nach `twitch_raid_retention` (idempotent, `ON CONFLICT DO NOTHING`).
pub async fn compute_raid_retention(pool: &PgPool) -> Result<RetentionStats, sqlx::Error> {
    let mut stats = RetentionStats::default();

    let raids = sqlx::query!(
        r#"
        SELECT id, from_broadcaster_login, to_broadcaster_login,
               COALESCE(viewer_count, 0) AS "viewer_count!", executed_at
         FROM twitch_raid_history
         WHERE executed_at >= NOW() - INTERVAL '7 days'
         ORDER BY executed_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    stats.raids_scanned = raids.len();

    for row in raids {
        let raid = RaidRow {
            id: row.id,
            from_login: row.from_broadcaster_login.trim().to_lowercase(),
            to_login: row.to_broadcaster_login.trim().to_lowercase(),
            viewer_count: row.viewer_count,
            executed_at: row.executed_at,
        };
        match compute_one(pool, &raid).await {
            Ok(Outcome::Computed) => stats.raids_computed += 1,
            Ok(Outcome::SkippedExisting) => stats.raids_skipped_existing += 1,
            Ok(Outcome::SkippedNoSession) => stats.raids_skipped_no_session += 1,
            Err(err) => tracing::error!(
                raid_id = raid.id,
                error = %err,
                "raid_retention: Berechnung fehlgeschlagen"
            ),
        }
    }

    Ok(stats)
}

enum Outcome {
    Computed,
    SkippedExisting,
    SkippedNoSession,
}

async fn compute_one(pool: &PgPool, raid: &RaidRow) -> Result<Outcome, sqlx::Error> {
    // 1) Skip wenn bereits berechnet (raid_id, executed_at).
    let exists: bool = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM twitch_raid_retention \
         WHERE raid_id = $1 AND executed_at = $2) AS \"exists!\"",
        raid.id,
        raid.executed_at,
    )
    .fetch_one(pool)
    .await?;
    if exists {
        return Ok(Outcome::SkippedExisting);
    }

    // 2) Ziel-Session auflösen (timestamptz ↔ timestamptz, kein Cast gg. Prod).
    let target_session: Option<i64> = sqlx::query_scalar!(
        "SELECT id FROM twitch_stream_sessions \
         WHERE LOWER(streamer_login) = $1 \
           AND started_at <= $2 \
           AND (ended_at IS NULL OR ended_at >= $2) \
         ORDER BY started_at DESC LIMIT 1",
        &raid.to_login,
        raid.executed_at,
    )
    .fetch_optional(pool)
    .await?;
    let Some(target_session_id) = target_session else {
        return Ok(Outcome::SkippedNoSession);
    };

    // 3) Fenster-Counts 5/15/30 (über session_chatters.last_seen_at).
    let at5 = window_count(pool, target_session_id, raid.executed_at, 5).await?;
    let at15 = window_count(pool, target_session_id, raid.executed_at, 15).await?;
    let at30 = window_count(pool, target_session_id, raid.executed_at, 30).await?;

    // 4) Herkunfts-Splits — Untergrenze executed_at, KEINE Obergrenze
    //    (new_chatters ohne last_seen_at-Bedingung), exakt wie Python.
    let known_from_raider = count_known_from_raider(pool, target_session_id, raid).await?;
    let new_to_target = count_new_to_target(pool, target_session_id, raid).await?;
    let new_chatters = count_new_chatters(pool, target_session_id, raid).await?;

    // 5) Insert (target_session_id::int8-Cast), ON CONFLICT DO NOTHING.
    sqlx::query!(
        "INSERT INTO twitch_raid_retention \
         (raid_id, from_broadcaster_login, to_broadcaster_login, viewer_count_sent, \
          executed_at, target_session_id, chatters_at_plus5m, chatters_at_plus15m, \
          chatters_at_plus30m, known_from_raider, new_to_target, new_chatters) \
         VALUES ($1, $2, $3, $4, $5, $6::int8, $7, $8, $9, $10, $11, $12) \
         ON CONFLICT (raid_id, executed_at) DO NOTHING",
        raid.id,
        &raid.from_login,
        &raid.to_login,
        raid.viewer_count,
        raid.executed_at,
        target_session_id,
        at5,
        at15,
        at30,
        known_from_raider,
        new_to_target,
        new_chatters,
    )
    .execute(pool)
    .await?;

    Ok(Outcome::Computed)
}

/// COUNT(DISTINCT chatter) im Ziel-Fenster `[executed, executed + offset min]`.
async fn window_count(
    pool: &PgPool,
    target_session_id: i64,
    executed_at: DateTime<Utc>,
    offset_min: i32,
) -> Result<i32, sqlx::Error> {
    // dyn: Bot-Filter erzeugt eine variable NOT-IN-Placeholderliste.
    let count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(DISTINCT COALESCE(NULLIF(chatter_login, ''), chatter_id)) \
         FROM twitch_session_chatters \
         WHERE session_id = $1 \
           AND last_seen_at >= $2 \
           AND last_seen_at <= $2 + INTERVAL '{offset_min} minutes' \
           {bot}",
        offset_min = offset_min,
        bot = bot_not_in_clause("chatter_login", 3),
    ))
    .bind(target_session_id)
    .bind(executed_at)
    .bind_bots()
    .fetch_one(pool)
    .await?;
    Ok(count as i32)
}

/// COUNT(DISTINCT chatter_login) der Ziel-Chatter ab `executed_at` (KEINE
/// Obergrenze, Python-Parität), die bereits vor dem Raid im **Rollup des
/// FROM-Streamers** standen (`first_seen_at < executed_at`).
async fn count_known_from_raider(
    pool: &PgPool,
    target_session_id: i64,
    raid: &RaidRow,
) -> Result<i32, sqlx::Error> {
    // dyn: Bot-Filter erzeugt eine variable NOT-IN-Placeholderliste.
    let count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(DISTINCT sc.chatter_login) \
         FROM twitch_session_chatters sc \
         JOIN twitch_chatter_rollup r \
           ON LOWER(r.streamer_login) = $3 AND r.chatter_login = sc.chatter_login \
          AND r.first_seen_at < $2 \
         WHERE sc.session_id = $1 \
           AND sc.last_seen_at >= $2 \
           {bot}",
        bot = bot_not_in_clause("sc.chatter_login", 4),
    ))
    .bind(target_session_id)
    .bind(raid.executed_at)
    .bind(&raid.from_login)
    .bind_bots()
    .fetch_one(pool)
    .await?;
    Ok(count as i32)
}

/// COUNT(DISTINCT chatter) ab `executed_at` (KEINE Obergrenze, Python-Parität),
/// die NICHT bereits vor dem Raid im Rollup des TO-Streamers waren
/// (`first_seen_at < executed_at`).
async fn count_new_to_target(
    pool: &PgPool,
    target_session_id: i64,
    raid: &RaidRow,
) -> Result<i32, sqlx::Error> {
    // dyn: Bot-Filter erzeugt eine variable NOT-IN-Placeholderliste.
    let count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) \
         FROM twitch_session_chatters sc \
         WHERE sc.session_id = $1 \
           AND sc.last_seen_at >= $2 \
           AND NOT EXISTS ( \
             SELECT 1 FROM twitch_chatter_rollup r \
             WHERE LOWER(r.streamer_login) = $3 \
               AND r.chatter_login = sc.chatter_login \
               AND r.first_seen_at < $2 \
           ) \
           {bot}",
        bot = bot_not_in_clause("sc.chatter_login", 4),
    ))
    .bind(target_session_id)
    .bind(raid.executed_at)
    .bind(&raid.to_login)
    .bind_bots()
    .fetch_one(pool)
    .await?;
    Ok(count as i32)
}

/// Echte Erst-Schreiber (`first_message_at >= executed_at AND messages > 0`),
/// die NICHT bereits vor dem Raid im Rollup des TO-Streamers waren. Anders als
/// die übrigen Metriken hat new_chatters GAR KEINE `last_seen_at`-Bedingung
/// (Python-Parität) — Lurker zählen über `messages > 0` ohnehin nicht.
async fn count_new_chatters(
    pool: &PgPool,
    target_session_id: i64,
    raid: &RaidRow,
) -> Result<i32, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) \
         FROM twitch_session_chatters sc \
         WHERE sc.session_id = $1 \
           AND sc.first_message_at >= $2 \
           AND sc.messages > 0 \
           AND NOT EXISTS ( \
             SELECT 1 FROM twitch_chatter_rollup r \
             WHERE LOWER(r.streamer_login) = $3 \
               AND r.chatter_login = sc.chatter_login \
               AND r.first_seen_at < $2 \
           ) \
           {bot}",
        bot = bot_not_in_clause("sc.chatter_login", 4),
    ))
    .bind(target_session_id)
    .bind(raid.executed_at)
    .bind(&raid.to_login)
    .bind_bots()
    .fetch_one(pool)
    .await?;
    Ok(count as i32)
}

/// Baut das `AND LOWER(<col>) NOT IN ($start, …)`-Fragment für die Bot-Exklusion
/// (Port `build_known_chat_bot_not_in_clause`). NULL/''-Logins bleiben (LOWER auf
/// NULL = NULL ⇒ `NOT IN` ist NULL ⇒ Zeile fällt NICHT raus). `start` = erster
/// Param-Index der Bot-Bindings.
fn bot_not_in_clause(column: &str, start: usize) -> String {
    let placeholders: Vec<String> = (0..WHITELISTED_BOTS.len())
        .map(|i| format!("${}", start + i))
        .collect();
    format!("AND LOWER({column}) NOT IN ({})", placeholders.join(", "))
}

/// Bindet die `WHITELISTED_BOTS`-Logins an die Query (in der Reihenfolge, die
/// [`bot_not_in_clause`] erwartet). Erweiterungs-Trait, damit der Aufruf wie
/// `.bind_bots()` lesbar an den Bind-Ketten hängt.
trait BindBots<'q> {
    fn bind_bots(self) -> Self;
}

impl<'q> BindBots<'q> for sqlx::query::QueryScalar<'q, sqlx::Postgres, i64, sqlx::postgres::PgArguments> {
    fn bind_bots(mut self) -> Self {
        for bot in WHITELISTED_BOTS {
            self = self.bind(*bot);
        }
        self
    }
}
