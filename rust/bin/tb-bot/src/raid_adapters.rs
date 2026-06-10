//! Raid-Adapter der Composition-Root: Helix → tb-raid-Ports.
//! (Hexagonal — die Pipeline kennt kein Helix.)
//!
//! Noch nicht aus `main.rs` aufgerufen (Cutover-Gate).
#![allow(dead_code)]

use std::sync::Arc;

use tb_monitoring::SubscriptionManager;
use tb_raid::{ArrivalReadiness, FairnessCandidate, FallbackStreamSource, RaidApi};
use tb_transport_twitch::{HelixClient, HelixStream};

/// Startzeit-Sentinel wie in `target_resolution` (sortiert ans Ende).
const STARTED_AT_SENTINEL: &str = "9999-99-99";

/// Helix-Adapter für den Raid-Start (`POST /raids` mit User-Token).
pub struct HelixRaidApi {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl RaidApi for HelixRaidApi {
    async fn start_raid(
        &self,
        from_broadcaster_id: &str,
        to_broadcaster_id: &str,
        user_token: &str,
    ) -> Result<(), String> {
        match self
            .helix
            .start_raid(from_broadcaster_id, to_broadcaster_id, user_token)
            .await
        {
            // API erreicht: Ok(()) oder Twitch-Fehlertext (auf den matcht
            // `is_retryable_raid_error`).
            Ok(result) => result,
            // Netz-/Transportfehler: nicht-wiederholbar formatieren.
            Err(error) => Err(format!("Raid API request failed: {error}")),
        }
    }
}

/// Wandelt einen Helix-Stream in einen Fairness-Kandidaten des DE-Fallbacks.
/// Follower werden hier bewusst nicht geholt (bis zu 50 Streams → 50 Calls);
/// der Fairness-Tie-Break fällt dann auf `started_at` zurück.
fn to_fairness_candidate(stream: HelixStream) -> FairnessCandidate {
    FairnessCandidate {
        user_id: stream.user_id,
        user_login: stream.user_login.trim().to_lowercase(),
        viewer_count: stream.viewer_count as i32,
        followers_total: 0,
        started_at: Some(stream.started_at)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| STARTED_AT_SENTINEL.to_string()),
    }
}

/// Helix-Adapter für die Fallback-Streams der Ziel-Kategorie.
pub struct HelixFallbackStreams {
    pub helix: HelixClient,
}

#[async_trait::async_trait]
impl FallbackStreamSource for HelixFallbackStreams {
    async fn category_streams(
        &self,
        category_id: &str,
        language: &str,
        limit: usize,
    ) -> Result<Vec<FairnessCandidate>, String> {
        let streams = self
            .helix
            .get_streams_by_category(category_id, Some(language), limit)
            .await
            .map_err(|error| error.to_string())?;
        Ok(streams.into_iter().map(to_fairness_candidate).collect())
    }
}

/// SubscriptionManager-Adapter: stellt vor dem Raid-Start die
/// `channel.raid`-Subscription fürs Ziel sicher (best-effort).
pub struct ManagerArrivalReadiness {
    pub manager: Arc<SubscriptionManager>,
}

#[async_trait::async_trait]
impl ArrivalReadiness for ManagerArrivalReadiness {
    async fn ensure_ready(&self, to_broadcaster_id: &str, to_broadcaster_login: &str) -> bool {
        self.manager
            .ensure_raid_subscription(to_broadcaster_id, to_broadcaster_login)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(user_id: &str, login: &str, viewers: i64, started_at: &str) -> HelixStream {
        HelixStream {
            user_id: user_id.to_string(),
            user_login: login.to_string(),
            viewer_count: viewers,
            started_at: started_at.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn fairness_kandidat_normalisiert_login_und_sentinel() {
        let c = to_fairness_candidate(stream("1", "  MixedCase ", 7, ""));
        assert_eq!(c.user_login, "mixedcase");
        assert_eq!(c.viewer_count, 7);
        assert_eq!(
            c.started_at, STARTED_AT_SENTINEL,
            "leere Startzeit → Sentinel"
        );

        let c2 = to_fairness_candidate(stream("2", "x", 1, "2026-06-10T16:00:00Z"));
        assert_eq!(c2.started_at, "2026-06-10T16:00:00Z");
    }
}
