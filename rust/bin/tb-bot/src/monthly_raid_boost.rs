//! Monatlicher Abschluss der Partner-Effort-Season um 00:05 Europe/Berlin.

use chrono::Utc;
use sqlx::PgPool;
use tb_raid::{MonthlyRaidBoostStore, SeasonCloseOutcome};

use crate::score_refresh::ScoreRefreshResolver;

const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub async fn run(pool: PgPool) {
    let store = MonthlyRaidBoostStore::new(pool.clone());
    let mut ticker = tokio::time::interval(RETRY_DELAY);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let now = Utc::now();
        match store.due_season_cutoffs(now).await {
            Ok(cutoffs) => {
                for cutoff in cutoffs {
                    if !close_once(&store, &pool, cutoff, now).await {
                        break;
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Offene Monatswertungen konnten nicht gelesen werden")
            }
        }
    }
}

/// true bedeutet: Season ist geschlossen oder war bereits geschlossen.
/// false bedeutet: am selben 1. später erneut versuchen.
async fn close_once(
    store: &MonthlyRaidBoostStore,
    pool: &PgPool,
    cutoff: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
) -> bool {
    let season_key = match store.close_season_ending_at(cutoff, now).await {
        Ok(SeasonCloseOutcome::Closed {
            season_key,
            partners,
            ..
        }) => {
            tracing::info!(%season_key, partners, "Partner-Effort-Season abgeschlossen");
            season_key
        }
        Ok(SeasonCloseOutcome::AlreadyClosed { season_key }) => season_key,
        Ok(SeasonCloseOutcome::SourceUnavailable { season_key }) => {
            tracing::warn!(%season_key, "Partner-Effort-Eventquelle noch nicht verfügbar; Monatsabschluss wird erneut versucht");
            return false;
        }
        Err(error) => {
            tracing::error!(%error, "Partner-Effort-Monatsabschluss fehlgeschlagen; Retry folgt");
            return false;
        }
    };

    // Auch AlreadyClosed muss den Score schreiben: Ein Prozessabbruch zwischen
    // Grant-Commit und Score-Refresh darf den Sofort-Refresh nicht verschlucken.
    // Den aktuellen Login ausschließlich über die stabile Twitch-ID auflösen.
    let recipient: Option<(String, String)> = match sqlx::query_as(
        "SELECT g.twitch_user_id, COALESCE(p.twitch_login, g.twitch_login)
           FROM twitch_partner_raid_boost_grants g
           LEFT JOIN twitch_partners p ON p.twitch_user_id = g.twitch_user_id
          WHERE g.season_key = $1",
    )
    .bind(&season_key)
    .fetch_optional(pool)
    .await
    {
        Ok(recipient) => recipient,
        Err(error) => {
            tracing::error!(%error, %season_key, "Monatsboost-Score-Refresh konnte nicht vorbereitet werden");
            return false;
        }
    };
    let Some(recipient) = recipient else {
        return true;
    };
    match ScoreRefreshResolver::new(pool.clone())
        .refresh_scores(&[recipient], now)
        .await
    {
        Ok(1) => true,
        Ok(written) => {
            tracing::warn!(%season_key, written, "Monatsboost-Score fehlt; Refresh wird erneut versucht");
            false
        }
        Err(error) => {
            tracing::error!(%error, %season_key, "Sofortiger Raid-Score-Refresh nach Monatsboost fehlgeschlagen; Retry folgt");
            false
        }
    }
}
