//! Store für `twitch_scout_candidates`: Vormerken (Upsert), Entscheidung
//! setzen, freigegebene-ohne-Dispatch lesen, Dispatch stempeln.
//!
//! Idempotenz und REQ-05: der Upsert überschreibt nur Zeilen mit Status
//! `vorgeschlagen` — Freigaben und Überspringungen des Nutzers bleiben
//! stehen, pausierte/übersprungene Kandidaten tauchen nicht erneut auf.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::detector::KandidatFund;
use crate::{
    normalisiere_login, normalize_entscheidung, STATUS_APPROVED, STATUS_PAUSIERT,
    STATUS_PERSOENLICH, STATUS_VORGESCHLAGEN,
};

/// Eine Zeile aus `twitch_scout_candidates`.
#[derive(Debug, Clone, PartialEq)]
pub struct KandidatZeile {
    pub login: String,
    pub twitch_user_id: Option<String>,
    pub sessions_count: i32,
    pub avg_viewers: f32,
    pub first_seen: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub language: Option<String>,
    pub deadlock_share: f32,
    pub status: String,
    pub entscheid_grund: Option<String>,
    pub approver: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub dispatched_at: Option<DateTime<Utc>>,
    /// Erster erkannter Owner-Besuch im Kanal (Besuch-Erkennung, tb-bot-Tick).
    pub visited_at: Option<DateTime<Utc>>,
    /// `auto` (Scout-Erkennung) oder `community` (Vorschlag aus dem Discord).
    pub source: String,
    /// Erster Vorschlagender (Discord-ID), nur bei Community-Vorschlägen.
    pub suggested_by_discord_id: Option<String>,
    pub suggestion_reason: Option<String>,
    pub suggested_at: Option<DateTime<Utc>>,
    /// Zahl verschiedener Vorschlagender aus der Community.
    pub suggestion_count: i32,
}

#[derive(sqlx::FromRow)]
struct Zeile {
    streamer_login: String,
    twitch_user_id: Option<String>,
    sessions_count: i32,
    avg_viewers: f32,
    first_seen: Option<DateTime<Utc>>,
    last_seen: Option<DateTime<Utc>>,
    language: Option<String>,
    deadlock_share: f32,
    status: String,
    entscheid_grund: Option<String>,
    approver: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    dispatched_at: Option<DateTime<Utc>>,
    visited_at: Option<DateTime<Utc>>,
    source: String,
    suggested_by_discord_id: Option<String>,
    suggestion_reason: Option<String>,
    suggested_at: Option<DateTime<Utc>>,
    suggestion_count: i32,
}

impl From<Zeile> for KandidatZeile {
    fn from(z: Zeile) -> Self {
        Self {
            login: z.streamer_login,
            twitch_user_id: z.twitch_user_id,
            sessions_count: z.sessions_count,
            avg_viewers: z.avg_viewers,
            first_seen: z.first_seen,
            last_seen: z.last_seen,
            language: z.language,
            deadlock_share: z.deadlock_share,
            status: z.status,
            entscheid_grund: z.entscheid_grund,
            approver: z.approver,
            decided_at: z.decided_at,
            dispatched_at: z.dispatched_at,
            visited_at: z.visited_at,
            source: z.source,
            suggested_by_discord_id: z.suggested_by_discord_id,
            suggestion_reason: z.suggestion_reason,
            suggested_at: z.suggested_at,
            suggestion_count: z.suggestion_count,
        }
    }
}

const SPALTEN: &str = "streamer_login, twitch_user_id, sessions_count, avg_viewers, first_seen, \
     last_seen, language, deadlock_share, status, entscheid_grund, approver, decided_at, dispatched_at, \
     visited_at, source, suggested_by_discord_id, suggestion_reason, suggested_at, suggestion_count";

/// Merkt einen Kandidaten vor. `true`, wenn geschrieben wurde (neu angelegt
/// oder Kennzahlen einer `vorgeschlagen`-Zeile aktualisiert). Zeilen mit
/// bereits getroffener Entscheidung bleiben unangetastet (`false`).
pub async fn vermerke_kandidat(pool: &PgPool, fund: &KandidatFund) -> Result<bool, sqlx::Error> {
    let ergebnis = sqlx::query(
        "INSERT INTO twitch_scout_candidates \
             (streamer_login, twitch_user_id, sessions_count, avg_viewers, first_seen, last_seen, \
              language, deadlock_share, status) \
           VALUES (LOWER($1), $2, $3, $4, $5, $6, $7, $8, $9) \
           ON CONFLICT (streamer_login) DO UPDATE SET \
             twitch_user_id = COALESCE(EXCLUDED.twitch_user_id, twitch_scout_candidates.twitch_user_id), \
             sessions_count = EXCLUDED.sessions_count, \
             avg_viewers = EXCLUDED.avg_viewers, \
             first_seen = EXCLUDED.first_seen, \
             last_seen = EXCLUDED.last_seen, \
             language = EXCLUDED.language, \
             deadlock_share = EXCLUDED.deadlock_share \
           WHERE twitch_scout_candidates.status = 'vorgeschlagen'",
    )
    .bind(&fund.login)
    .bind(fund.twitch_user_id.as_deref().filter(|id| !id.is_empty()))
    .bind(i32::try_from(fund.sessions_count).unwrap_or(i32::MAX))
    .bind(fund.avg_viewers as f32)
    .bind(fund.first_seen)
    .bind(fund.last_seen)
    .bind(fund.language.as_deref().filter(|l| !l.is_empty()))
    .bind(fund.deadlock_share as f32)
    .bind(STATUS_VORGESCHLAGEN)
    .execute(pool)
    .await?;
    Ok(ergebnis.rows_affected() > 0)
}

/// Setzt eine Admin-Entscheidung (`approved` | `uebersprungen` | `pausiert` |
/// `persoenlich` | `bekannter_kontakt`) samt Grund und Entscheider. `false`,
/// wenn der Login unbekannt oder der Status ungültig ist — dann wird nichts
/// geschrieben.
pub async fn setze_entscheidung(
    pool: &PgPool,
    login: &str,
    entscheidung: &str,
    grund: Option<&str>,
    approver: &str,
) -> Result<bool, sqlx::Error> {
    let Some(login) = normalisiere_login(login) else {
        return Ok(false);
    };
    let Some(status) = normalize_entscheidung(entscheidung) else {
        return Ok(false);
    };
    let grund = grund.map(str::trim).filter(|g| !g.is_empty());
    let ergebnis = sqlx::query(
        "UPDATE twitch_scout_candidates \
           SET status = $2, entscheid_grund = $3, approver = $4, decided_at = NOW() \
         WHERE streamer_login = $1",
    )
    .bind(&login)
    .bind(status)
    .bind(grund)
    .bind(Some(approver.trim()).filter(|a| !a.is_empty()))
    .execute(pool)
    .await?;
    Ok(ergebnis.rows_affected() > 0)
}

/// Offene Kandidaten für die Freigabeliste: `vorgeschlagen` + `pausiert`,
/// älteste first_seen zuerst.
pub async fn liste_offen(pool: &PgPool) -> Result<Vec<KandidatZeile>, sqlx::Error> {
    let sql = format!(
        "SELECT {SPALTEN} FROM twitch_scout_candidates \
         WHERE status IN ('{v}', '{p}') \
         ORDER BY first_seen ASC NULLS LAST, streamer_login ASC",
        v = STATUS_VORGESCHLAGEN,
        p = STATUS_PAUSIERT
    );
    let zeilen = sqlx::query_as::<_, Zeile>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(KandidatZeile::from)
        .collect();
    Ok(zeilen)
}

/// Persönliche Besuchsliste: Status `persoenlich`, nach Potenzial sortiert
/// (wiederkehrende Kanäle zuerst, dann Ø Zuschauer, dann älteste first_seen).
pub async fn liste_persoenlich(pool: &PgPool) -> Result<Vec<KandidatZeile>, sqlx::Error> {
    let sql = format!(
        "SELECT {SPALTEN} FROM twitch_scout_candidates \
         WHERE status = '{p}' \
         ORDER BY sessions_count DESC, avg_viewers DESC, first_seen ASC",
        p = STATUS_PERSOENLICH
    );
    let zeilen = sqlx::query_as::<_, Zeile>(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(KandidatZeile::from)
        .collect();
    Ok(zeilen)
}

/// Freigegebene Kandidaten ohne Dispatch-Stempel, mit bekannter
/// `twitch_user_id` (der bestehende Outreach-Weg braucht die ID), älteste
/// Entscheidung zuerst.
pub async fn approved_ohne_dispatch(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<KandidatZeile>, sqlx::Error> {
    let sql = format!(
        "SELECT {SPALTEN} FROM twitch_scout_candidates \
         WHERE status = '{a}' AND dispatched_at IS NULL \
           AND twitch_user_id IS NOT NULL AND twitch_user_id <> '' \
         ORDER BY decided_at ASC NULLS LAST, streamer_login ASC LIMIT $1",
        a = STATUS_APPROVED
    );
    let zeilen = sqlx::query_as::<_, Zeile>(sqlx::AssertSqlSafe(sql))
        .bind(limit.max(0))
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(KandidatZeile::from)
        .collect();
    Ok(zeilen)
}

/// Stempelt den Dispatch. Läuft nur bei `approved` und nur einmal:
/// ein zweiter Aufruf für dieselbe Zeile liefert `false` (INV-06).
pub async fn vermerke_dispatch(pool: &PgPool, login: &str) -> Result<bool, sqlx::Error> {
    let Some(login) = normalisiere_login(login) else {
        return Ok(false);
    };
    let ergebnis = sqlx::query(
        "UPDATE twitch_scout_candidates \
           SET dispatched_at = NOW() \
         WHERE streamer_login = $1 AND status = $2 AND dispatched_at IS NULL",
    )
    .bind(&login)
    .bind(STATUS_APPROVED)
    .execute(pool)
    .await?;
    Ok(ergebnis.rows_affected() > 0)
}
