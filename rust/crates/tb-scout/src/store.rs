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
}

type Zeile = (
    String,
    Option<String>,
    i32,
    f32,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<String>,
    f32,
    String,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

impl From<Zeile> for KandidatZeile {
    fn from(z: Zeile) -> Self {
        Self {
            login: z.0,
            twitch_user_id: z.1,
            sessions_count: z.2,
            avg_viewers: z.3,
            first_seen: z.4,
            last_seen: z.5,
            language: z.6,
            deadlock_share: z.7,
            status: z.8,
            entscheid_grund: z.9,
            approver: z.10,
            decided_at: z.11,
            dispatched_at: z.12,
            visited_at: z.13,
        }
    }
}

const SPALTEN: &str = "streamer_login, twitch_user_id, sessions_count, avg_viewers, first_seen, \
     last_seen, language, deadlock_share, status, entscheid_grund, approver, decided_at, dispatched_at, \
     visited_at";

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
    let zeilen = sqlx::query_as::<_, Zeile>(&sql)
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
    let zeilen = sqlx::query_as::<_, Zeile>(&sql)
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
    let zeilen = sqlx::query_as::<_, Zeile>(&sql)
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
