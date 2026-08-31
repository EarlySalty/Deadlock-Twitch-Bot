//! Kandidaten-Erkennung "klein + first_seen" über `twitch_stats_category`.
//!
//! Ein Kandidat muss im Lookback-Fenster liegen (erste Sichtung nicht älter
//! als [`LOOKBACK_DAYS`]), dort höchstens [`MAX_SESSIONS`] Sessions
//! (LAG/30-Minuten-Gap wie `admin_research.rs`) und im Schnitt höchstens
//! [`MAX_AVG_VIEWERS`] Zuschauer haben und darf nicht Partner sein. Die
//! harten Filter (Black-/Denylists, aktive Recruitment-Suppression, aktiver
//! Outreach-Cooldown, schon entschieden) sitzen als NOT-EXISTS direkt in der
//! Query; der globale Ban wird pro Kandidat im Code über den bestehenden
//! `RaidBlacklistStore::is_hard_banned` geprüft — bei Query-Fehler
//! fail-closed: der Kandidat fällt raus.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tb_raid::RaidBlacklistStore;

use crate::store;

/// Lookback-Fenster in Tagen; zugleich Obergrenze für first_seen
/// (Kanäle, deren ältester Tick weiter zurückliegt, fallen raus).
pub const LOOKBACK_DAYS: i64 = 60;
/// Höchstzahl Sessions im Fenster (REQ-01a).
pub const MAX_SESSIONS: i64 = 5;
/// Höchstzahl mittlere Zuschauer (REQ-01c).
pub const MAX_AVG_VIEWERS: f64 = 10.0;
/// Kappung je Lauf, damit ein Scan die Datenbank nicht wälzt.
pub const MAX_ERGEBNIS: i64 = 25;

/// Ein erkannter Kandidat (Zeile der Aggregat-Query).
#[derive(Debug, Clone, PartialEq)]
pub struct KandidatFund {
    pub login: String,
    pub twitch_user_id: Option<String>,
    pub sessions_count: i64,
    pub avg_viewers: f64,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub language: Option<String>,
    pub deadlock_share: f64,
}

type FundZeile = (
    String,
    i64,
    f64,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<String>,
    f64,
    Option<String>,
);

const FINDE_SQL: &str = r#"WITH ticks AS (
       SELECT LOWER(s.streamer) AS login, s.ts_utc, s.viewer_count, s.language, s.game_name,
              LAG(s.ts_utc) OVER (PARTITION BY LOWER(s.streamer) ORDER BY s.ts_utc) AS previous_ts
       FROM twitch_stats_category s
       WHERE s.ts_utc >= $1
         AND s.is_partner = FALSE
         AND NOT EXISTS (SELECT 1 FROM twitch_stats_category alt
                         WHERE LOWER(alt.streamer) = LOWER(s.streamer) AND alt.ts_utc < $1)
         AND NOT EXISTS (SELECT 1 FROM twitch_partners p
                         WHERE LOWER(p.twitch_login) = LOWER(s.streamer))
         AND NOT EXISTS (SELECT 1 FROM twitch_raid_blacklist b
                         WHERE LOWER(b.target_login) = LOWER(s.streamer))
         AND NOT EXISTS (SELECT 1 FROM twitch_partner_signup_denylist d
                         WHERE LOWER(d.twitch_login) = LOWER(s.streamer))
         AND NOT EXISTS (SELECT 1 FROM twitch_scout_pitch_blacklist pb
                         WHERE LOWER(pb.streamer_login) = LOWER(s.streamer))
         AND NOT EXISTS (SELECT 1 FROM twitch_outbound_chat_suppressions sup
                         WHERE LOWER(sup.target_login) = LOWER(s.streamer)
                           AND sup.source = 'recruitment'
                           AND sup.suppressed_until > NOW())
         AND NOT EXISTS (SELECT 1 FROM twitch_partner_outreach o
                         WHERE LOWER(o.streamer_login) = LOWER(s.streamer)
                           AND o.cooldown_until > NOW())
         AND NOT EXISTS (SELECT 1 FROM twitch_scout_candidates c
                         WHERE c.streamer_login = LOWER(s.streamer)
                           AND c.status <> 'vorgeschlagen')
   )
   SELECT login,
          COUNT(*) FILTER (
              WHERE previous_ts IS NULL OR ts_utc - previous_ts > INTERVAL '30 minutes'
          )::bigint AS sessions_count,
          AVG(viewer_count)::float8 AS avg_viewers,
          MIN(ts_utc) AS first_seen,
          MAX(ts_utc) AS last_seen,
          MODE() WITHIN GROUP (ORDER BY NULLIF(LOWER(TRIM(language)), '')) AS language,
          AVG(CASE WHEN LOWER(TRIM(COALESCE(game_name, ''))) = 'deadlock'
                   THEN 1.0 ELSE 0.0 END)::float8 AS deadlock_share,
          (SELECT ss.twitch_user_id FROM twitch_stream_sessions ss
            WHERE LOWER(ss.streamer_login) = login
              AND ss.twitch_user_id IS NOT NULL AND ss.twitch_user_id <> ''
            ORDER BY ss.started_at DESC LIMIT 1) AS twitch_user_id
   FROM ticks
   GROUP BY login
   HAVING COUNT(*) FILTER (
              WHERE previous_ts IS NULL OR ts_utc - previous_ts > INTERVAL '30 minutes'
          ) <= $2
      AND AVG(viewer_count)::float8 <= $3
   ORDER BY first_seen ASC, login ASC
   LIMIT $4"#;

/// Liefert die aktuellen Kandidaten. Datenbankfehler werden an den Aufrufer
/// gereicht, damit ein defekter Scan nicht wie „keine Kandidaten“ aussieht.
pub async fn finde_kandidaten(pool: &PgPool) -> Result<Vec<KandidatFund>, sqlx::Error> {
    let seit = Utc::now() - chrono::Duration::days(LOOKBACK_DAYS);
    let zeilen = sqlx::query_as::<_, FundZeile>(FINDE_SQL)
        .bind(seit)
        .bind(MAX_SESSIONS)
        .bind(MAX_AVG_VIEWERS)
        .bind(MAX_ERGEBNIS)
        .fetch_all(pool)
        .await;
    let zeilen = zeilen?;
    // Globaler Ban als Code-Probe. Ein Treffer verwirft den Kandidaten; ein
    // Queryfehler bricht den Scan sichtbar ab.
    let ban_store = RaidBlacklistStore::new(pool.clone());
    let mut funds = Vec::with_capacity(zeilen.len());
    for zeile in zeilen {
        let (
            login,
            sessions_count,
            avg_viewers,
            first_seen,
            last_seen,
            language,
            deadlock_share,
            twitch_user_id,
        ) = zeile;
        match ban_store
            .is_hard_banned(twitch_user_id.as_deref(), &login)
            .await
        {
            Ok(false) => {}
            Ok(true) => {
                tracing::info!(login, "tb-scout: Kandidat global gebannt, verworfen");
                continue;
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    login,
                    "tb-scout: Global-Ban-Prüfung fehlgeschlagen; Scan wird abgebrochen"
                );
                return Err(error);
            }
        }
        funds.push(KandidatFund {
            login,
            twitch_user_id,
            sessions_count,
            avg_viewers,
            first_seen,
            last_seen,
            language,
            deadlock_share,
        });
    }
    Ok(funds)
}

/// Erkennen und vormerken: legt jeden Fund als `vorgeschlagen` in
/// `twitch_scout_candidates` (bestehende Zeilen mit anderer Entscheidung
/// bleiben unangetastet, siehe [`store::vermerke_kandidat`]). Liefert die
/// Anzahl angefasster Zeilen.
pub async fn laufe_scout_scan(pool: &PgPool) -> Result<usize, sqlx::Error> {
    let mut angefasst = 0usize;
    for fund in finde_kandidaten(pool).await? {
        if store::vermerke_kandidat(pool, &fund).await? {
            angefasst += 1;
        }
    }
    if angefasst > 0 {
        tracing::info!(angefasst, "tb-scout: Scan-Kandidaten vorgemerkt");
    }
    Ok(angefasst)
}
