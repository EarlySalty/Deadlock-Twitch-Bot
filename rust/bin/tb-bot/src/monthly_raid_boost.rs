//! Monatlicher Abschluss der Partner-Effort-Season um 00:05 Europe/Berlin.

use chrono::{Datelike, Timelike, Utc};
use chrono_tz::Europe::Berlin;
use sqlx::PgPool;
use tb_raid::{next_monthly_close_after, MonthlyRaidBoostStore, SeasonCloseOutcome};

use crate::score_refresh::ScoreRefreshResolver;

const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub async fn run(pool: PgPool) {
    let store = MonthlyRaidBoostStore::new(pool.clone());

    // Neustart am 1. nach 00:05: denselben Abschluss idempotent nachholen.
    // Ein Mid-Month-Deploy schließt dagegen bewusst keinen alten Monat nach.
    if is_close_day_after_cutoff(Utc::now()) {
        retry_close_on_first(&store, &pool).await;
    }

    loop {
        let now = Utc::now();
        let next = next_monthly_close_after(now);
        let wait = (next - now)
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(1));
        tokio::time::sleep(wait).await;
        retry_close_on_first(&store, &pool).await;
    }
}

async fn retry_close_on_first(store: &MonthlyRaidBoostStore, pool: &PgPool) {
    loop {
        let now = Utc::now();
        if close_once(store, pool, now).await {
            return;
        }

        // Nur am 1. retryen. So kann eine kurz verspätete Effort-Migration den
        // Abschluss noch übernehmen, ohne später im Monat rückwirkend Grants
        // für Altmonate zu erzeugen.
        if now.with_timezone(&Berlin).day() != 1 {
            return;
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

fn is_close_day_after_cutoff(now: chrono::DateTime<Utc>) -> bool {
    let local = now.with_timezone(&Berlin);
    local.day() == 1 && (local.hour() > 0 || local.minute() >= 5)
}

/// true bedeutet: Season ist geschlossen oder war bereits geschlossen.
/// false bedeutet: am selben 1. später erneut versuchen.
async fn close_once(
    store: &MonthlyRaidBoostStore,
    pool: &PgPool,
    now: chrono::DateTime<Utc>,
) -> bool {
    match store.close_previous_season(now).await {
        Ok(SeasonCloseOutcome::Closed {
            season_key,
            partners,
            winner,
        }) => {
            tracing::info!(
                %season_key,
                partners,
                winner_id = winner.as_ref().map(|value| value.twitch_user_id.as_str()),
                "Partner-Effort-Season abgeschlossen"
            );

            if let Some(winner) = winner {
                let resolver = ScoreRefreshResolver::new(pool.clone());
                match resolver
                    .refresh_scores(
                        &[(winner.twitch_user_id.clone(), winner.twitch_login.clone())],
                        now,
                    )
                    .await
                {
                    Ok(written) => tracing::info!(
                        %season_key,
                        twitch_user_id = %winner.twitch_user_id,
                        written,
                        "Raid-Score nach Monatsboost sofort neu berechnet"
                    ),
                    Err(error) => tracing::error!(
                        %error,
                        %season_key,
                        twitch_user_id = %winner.twitch_user_id,
                        "Sofortiger Raid-Score-Refresh nach Monatsboost fehlgeschlagen"
                    ),
                }
            }
            true
        }
        Ok(SeasonCloseOutcome::AlreadyClosed { season_key }) => {
            tracing::debug!(%season_key, "Partner-Effort-Season bereits abgeschlossen");
            true
        }
        Ok(SeasonCloseOutcome::SourceUnavailable { season_key }) => {
            tracing::warn!(
                %season_key,
                "Partner-Effort-Eventquelle noch nicht verfügbar; Monatsabschluss wird erneut versucht"
            );
            false
        }
        Err(error) => {
            tracing::error!(%error, "Partner-Effort-Monatsabschluss fehlgeschlagen; Retry folgt");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn restart_nach_0005_am_ersten_ist_faellig() {
        let now = Utc
            .with_ymd_and_hms(2026, 9, 30, 22, 6, 0)
            .single()
            .unwrap();
        assert!(is_close_day_after_cutoff(now));
    }

    #[test]
    fn vor_0005_und_mitten_im_monat_kein_catchup() {
        let before = Utc
            .with_ymd_and_hms(2026, 9, 30, 22, 4, 0)
            .single()
            .unwrap();
        let middle = Utc
            .with_ymd_and_hms(2026, 9, 26, 20, 0, 0)
            .single()
            .unwrap();
        assert!(!is_close_day_after_cutoff(before));
        assert!(!is_close_day_after_cutoff(middle));
    }
}
