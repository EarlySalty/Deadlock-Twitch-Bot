//! Raid-Adapter der Composition-Root: Helix → tb-raid-Ports.
//! (Hexagonal — die Pipeline kennt kein Helix.)

use std::sync::Arc;

use tb_monitoring::sessions::tracker::FollowerCountSource;
use tb_monitoring::SubscriptionManager;
use tb_raid::{
    ArrivalReadiness, FairnessCandidate, FallbackStreamSource, FollowerEnricher, RaidApi,
    RefreshError, TokenOwnerInfo, TokenResponse, TwitchTokenClient, FOLLOWERS_UNKNOWN,
};
use tb_transport_twitch::{HelixClient, HelixStream, UserTokenError};

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
/// Follower werden hier bewusst NICHT geholt (bis zu 50 Streams → 50 Calls) —
/// `followers_total` startet auf [`FOLLOWERS_UNKNOWN`] und wird erst für den
/// **gefilterten** Pool per [`HelixFollowerEnricher`] angereichert (Python
/// `attach_followers_totals` auf dem Pool). Bleibt die Zahl unbekannt, sortiert
/// der Kandidat ans Ende — statt sie mit `0` an die Spitze zu ziehen.
fn to_fairness_candidate(stream: HelixStream) -> FairnessCandidate {
    FairnessCandidate {
        user_id: stream.user_id,
        user_login: stream.user_login.trim().to_lowercase(),
        viewer_count: stream.viewer_count as i32,
        followers_total: FOLLOWERS_UNKNOWN,
        started_at: Some(stream.started_at)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| STARTED_AT_SENTINEL.to_string()),
    }
}

/// Follower-Anreicherung für den gefilterten Fallback-Pool (Python
/// `attach_followers_totals`). Best-effort über den (per `FollowerCountSource`
/// gewählten) Token; ein Kandidat ohne abrufbare Zahl behält den
/// [`FOLLOWERS_UNKNOWN`]-Sentinel.
// P2.49: durch CachedFollowerEnricher ersetzt (Session-Cache vor Helix); als
// Referenz-Implementierung behalten, daher dead-code-erlaubt.
#[allow(dead_code)]
pub struct HelixFollowerEnricher {
    pub followers: Arc<dyn FollowerCountSource>,
}

#[async_trait::async_trait]
impl FollowerEnricher for HelixFollowerEnricher {
    async fn enrich(&self, pool: &mut [FairnessCandidate]) {
        enrich_via_helix(self.followers.as_ref(), pool).await;
    }
}

/// Helix-Anreicherung für alle noch unbekannten Kandidaten — gemeinsam genutzt
/// vom direkten Enricher und dem cache-vorgeschalteten [`CachedFollowerEnricher`].
async fn enrich_via_helix(followers: &dyn FollowerCountSource, pool: &mut [FairnessCandidate]) {
    for candidate in pool.iter_mut() {
        if candidate.followers_total != FOLLOWERS_UNKNOWN {
            continue;
        }
        let user_id = candidate.user_id.trim();
        let uid = if user_id.is_empty() {
            None
        } else {
            Some(user_id)
        };
        if let Some(total) = followers.follower_total(uid, &candidate.user_login).await {
            candidate.followers_total = total;
        }
    }
}

/// P2.49 — Follower-Anreicherung mit vorgeschaltetem DB-Cache-Backfill aus
/// `twitch_stream_sessions`. Zuerst wird die jüngste gespeicherte Follower-Zahl
/// je Login eingesetzt (günstig, offline-resilient, kein Moderator-Token nötig);
/// Helix-Calls fallen nur noch für danach unbekannte Kandidaten an. Port von
/// `attach_followers_totals` + `_load_cached_totals` (followers.py:111-139,247-257).
///
/// Umschließt den bestehenden [`HelixFollowerEnricher`]-Pfad als opt-in-Wrapper,
/// damit die Composition-Root [`HelixFollowerEnricher`] unverändert konstruieren
/// kann (kein erzwungenes Feld an der Live-Struktur).
// WIRING-TODO(P2.49): main.rs den FollowerEnricher der Fallback-Pipeline von
// HelixFollowerEnricher auf CachedFollowerEnricher { followers: <gleiche
// FollowerCountSource>, pool: <DB-Pool> } umstellen, damit der Session-Cache-
// Backfill vor Helix greift.
#[allow(dead_code)]
pub struct CachedFollowerEnricher {
    pub followers: Arc<dyn FollowerCountSource>,
    pub pool: sqlx::PgPool,
}

#[async_trait::async_trait]
impl FollowerEnricher for CachedFollowerEnricher {
    async fn enrich(&self, pool: &mut [FairnessCandidate]) {
        // 1. DB-Cache-Backfill für alle bislang unbekannten Kandidaten.
        let pending_logins: Vec<String> = pool
            .iter()
            .filter(|c| c.followers_total == FOLLOWERS_UNKNOWN)
            .map(|c| c.user_login.trim().to_lowercase())
            .filter(|l| !l.is_empty())
            .collect();
        let cache = load_session_follower_cache(&self.pool, &pending_logins).await;
        for candidate in pool.iter_mut() {
            if candidate.followers_total != FOLLOWERS_UNKNOWN {
                continue;
            }
            let login = candidate.user_login.trim().to_lowercase();
            if let Some(total) = cache.get(&login) {
                candidate.followers_total = *total;
            }
        }

        // 2. Helix nur noch für die danach unbekannten Kandidaten.
        enrich_via_helix(self.followers.as_ref(), pool).await;
    }
}

/// P2.49 — lädt die jüngste gespeicherte Follower-Zahl je Login aus
/// `twitch_stream_sessions`: `COALESCE(followers_end, followers_start)`, neueste
/// Session zuerst (`ORDER BY COALESCE(ended_at, started_at) DESC`), erster
/// Treffer je Login gewinnt. Best-effort: bei DB-Fehler leere Map (Helix-Pfad
/// bleibt). Port von `_load_cached_totals` (followers.py:111-139).
///
/// clean-SQL: `ended_at`/`started_at` sind TIMESTAMPTZ (Prod-Schema, siehe
/// score_refresh.rs SessionRaw) — kein TEXT-Vergleich. `followers_*` INTEGER.
// dead_code bis CachedFollowerEnricher in main.rs verdrahtet ist (WIRING-TODO P2.49).
#[allow(dead_code)]
async fn load_session_follower_cache(
    pool: &sqlx::PgPool,
    logins: &[String],
) -> std::collections::HashMap<String, i32> {
    let mut out = std::collections::HashMap::new();
    if logins.is_empty() {
        return out;
    }
    let rows: Result<Vec<(String, Option<i32>)>, sqlx::Error> = sqlx::query_as(
        "SELECT LOWER(streamer_login) AS login, \
                COALESCE(followers_end, followers_start) AS follower_total \
           FROM twitch_stream_sessions \
          WHERE LOWER(streamer_login) = ANY($1) \
            AND COALESCE(followers_end, followers_start) IS NOT NULL \
          ORDER BY COALESCE(ended_at, started_at) DESC",
    )
    .bind(logins)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => {
            for (login, total) in rows {
                let login = login.trim().to_string();
                if login.is_empty() {
                    continue;
                }
                // Erster Treffer je Login (= neueste Session) gewinnt.
                if let Some(total) = total {
                    out.entry(login).or_insert(total);
                }
            }
        }
        Err(error) => {
            tracing::debug!(%error, "Session-Follower-Cache-Query fehlgeschlagen");
        }
    }
    out
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

/// OAuth-Token-Client-Adapter: Refresh, Code-Exchange (Onboarding) und
/// Token-Owner-Lookup gegen Twitch.
///
/// `redirect_uri` muss exakt der beim Authorize-Link verwendeten URI
/// entsprechen (Twitch validiert sie beim `authorization_code`-Grant);
/// für den reinen Refresh-Pfad ist sie ungenutzt.
pub struct HelixTokenClient {
    pub helix: HelixClient,
    pub redirect_uri: String,
}

fn map_token_error(error: UserTokenError) -> RefreshError {
    match error {
        UserTokenError::InvalidClient => RefreshError::InvalidClient,
        UserTokenError::InvalidGrant => RefreshError::InvalidGrant,
        UserTokenError::Other(message) => RefreshError::Other(message),
    }
}

fn to_token_response(response: tb_transport_twitch::UserTokenResponse) -> TokenResponse {
    TokenResponse {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        expires_in: response.expires_in,
        scopes: response.scope,
    }
}

#[async_trait::async_trait]
impl TwitchTokenClient for HelixTokenClient {
    async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, RefreshError> {
        self.helix
            .refresh_user_token(refresh_token)
            .await
            .map(to_token_response)
            .map_err(map_token_error)
    }

    async fn exchange_code(&self, code: &str) -> Result<TokenResponse, RefreshError> {
        self.helix
            .exchange_user_code(code, &self.redirect_uri)
            .await
            .map(to_token_response)
            .map_err(map_token_error)
    }

    async fn token_owner(&self, access_token: &str) -> Result<TokenOwnerInfo, RefreshError> {
        let owner = self
            .helix
            .fetch_token_owner(access_token)
            .await
            .map_err(map_token_error)?;
        Ok(TokenOwnerInfo {
            twitch_user_id: owner.id,
            twitch_login: owner.login,
        })
    }
}

/// SubscriptionManager-Adapter: stellt vor dem Raid-Start die
/// `channel.raid`-Subscription fürs Ziel sicher (best-effort).
// P2.58: durch ManagerArrivalReadinessWithStatusPoll ersetzt (wartet auf
// Status `enabled`); als Referenz-Implementierung behalten, dead-code-erlaubt.
#[allow(dead_code)]
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

/// P2.58 — Readiness mit Status-Poll: legt die `channel.raid`-Subscription an
/// (best-effort) UND wartet danach, bis Twitch sie auf `enabled` verifiziert
/// hat. Ohne dieses Warten meldet `ensure_ready` schon nach dem create-POST
/// `true`, bevor Twitch den Webhook-Callback verifiziert hat — ein Raid, der in
/// diesem Fenster ankommt, würde nicht korreliert (kein Arrival-Effekt).
///
/// Umschließt [`ManagerArrivalReadiness`] als opt-in-Wrapper, damit die
/// Composition-Root die Live-Struktur unverändert konstruieren kann (kein
/// erzwungenes Feld). Port von `ensure_raid_target_dynamic_ready`
/// (eventsub_mixin.py:3094-3310, Webhook-Pfad).
// WIRING-TODO(P2.58): main.rs den ArrivalReadiness-Port der Raid-Pipeline von
// ManagerArrivalReadiness auf ManagerArrivalReadinessWithStatusPoll umstellen:
//   ManagerArrivalReadinessWithStatusPoll {
//       manager: <SubscriptionManager>,
//       status_poll: RaidSubscriptionStatusPoll {
//           transport: Arc::new(HelixSubscriptionTransport { helix }),  // gleicher Transport wie SubscriptionManager
//           wait_timeout: Duration::from_secs_f64(8.0),
//           poll_interval: Duration::from_millis(500),
//       },
//   }
// damit ensure_ready erst bei status==enabled true liefert.
#[allow(dead_code)]
pub struct ManagerArrivalReadinessWithStatusPoll {
    pub manager: Arc<SubscriptionManager>,
    pub status_poll: RaidSubscriptionStatusPoll,
}

#[async_trait::async_trait]
impl ArrivalReadiness for ManagerArrivalReadinessWithStatusPoll {
    async fn ensure_ready(&self, to_broadcaster_id: &str, to_broadcaster_login: &str) -> bool {
        // 1. Subscription anlegen (best-effort, 409-as-success).
        self.manager
            .ensure_raid_subscription(to_broadcaster_id, to_broadcaster_login)
            .await;
        // 2. Auf `enabled` warten — erst dann erreicht ein ankommender Raid
        //    den Webhook zuverlässig.
        self.status_poll.wait_until_enabled(to_broadcaster_id).await
    }
}

/// Status-Poll-Konfiguration + Transport für das P2.58-Readiness-Warten.
// dead_code bis zur Verdrahtung (siehe WIRING-TODO an
// ManagerArrivalReadinessWithStatusPoll); voll test-abgedeckt.
#[allow(dead_code)]
pub struct RaidSubscriptionStatusPoll {
    /// Transport, über den die registrierten Subscriptions samt Status gelesen
    /// werden (`SubscriptionTransport::list`).
    pub transport: Arc<dyn tb_monitoring::SubscriptionTransport>,
    /// Gesamt-Deadline (Python `wait_timeout_seconds = 8.0`).
    pub wait_timeout: std::time::Duration,
    /// Poll-Intervall (Python `poll_interval_seconds = 0.5`).
    pub poll_interval: std::time::Duration,
}

#[allow(dead_code)] // bis zur Verdrahtung; voll test-abgedeckt (status_poll_*).
impl RaidSubscriptionStatusPoll {
    /// Liest den Status der `channel.raid`-Subscription für ein Ziel:
    /// `Some("enabled")`, `Some("webhook_callback_verification_pending")`, … ;
    /// `None` = (noch) keine passende Subscription registriert.
    async fn raid_status(&self, to_broadcaster_id: &str) -> Option<String> {
        match self.transport.list().await {
            Ok(subs) => subs
                .into_iter()
                .find(|s| {
                    s.sub_type == "channel.raid"
                        && s.broadcaster_user_id.as_deref() == Some(to_broadcaster_id)
                })
                .map(|s| s.status),
            Err(error) => {
                tracing::debug!(%error, to_broadcaster_id, "Raid-Subscription-Status-Liste fehlgeschlagen");
                None
            }
        }
    }

    /// Pollt bis `enabled` oder Deadline. `true` nur bei `enabled`.
    /// `webhook_callback_verification_pending`/fehlend → weiter warten;
    /// jeder andere Status → sofort `false` (Python `status:{...}`-Pfad).
    async fn wait_until_enabled(&self, to_broadcaster_id: &str) -> bool {
        let deadline = std::time::Instant::now() + self.wait_timeout;
        loop {
            match self.raid_status(to_broadcaster_id).await.as_deref() {
                Some("enabled") => return true,
                // Verifikation läuft noch ODER Sub noch nicht gelistet → warten.
                Some("webhook_callback_verification_pending") | None => {}
                // Terminaler Fehlerstatus (failed, revoked, …) → kein Warten.
                Some(_other) => return false,
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

/// Adapter: interner API-Endpoint `POST /raid/manual` → Raid-Handler.
pub struct ManualRaidAdapter {
    pub handler: Arc<crate::auto_raid::OfflineRaidHandler>,
}

#[async_trait::async_trait]
impl tb_internal_api::ManualRaidPort for ManualRaidAdapter {
    async fn start_manual_raid(
        &self,
        broadcaster_id: &str,
        broadcaster_login: &str,
    ) -> serde_json::Value {
        let response = self
            .handler
            .start_manual_raid(broadcaster_id, broadcaster_login)
            .await;
        serde_json::to_value(&response).unwrap_or_else(|error| {
            tracing::error!(%error, "Manual-Raid-Antwort nicht serialisierbar");
            serde_json::json!({"status": "raid_failed", "error": "serialization"})
        })
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

    // ─── P2.49: Session-Cache-Backfill ──────────────────────────────────────

    #[cfg(feature = "integration")]
    use std::str::FromStr;
    #[cfg(feature = "integration")]
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Zählender Follower-Source-Stub: protokolliert Helix-Aufrufe und liefert
    /// für jeden Aufruf eine feste Zahl.
    #[cfg(feature = "integration")]
    struct CountingFollowerSource {
        calls: AtomicUsize,
        total: Option<i32>,
    }

    #[cfg(feature = "integration")]
    #[async_trait::async_trait]
    impl FollowerCountSource for CountingFollowerSource {
        async fn follower_total(&self, _twitch_user_id: Option<&str>, _login: &str) -> Option<i32> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.total
        }
    }

    #[cfg(feature = "integration")]
    async fn setup_sessions_db(schema: &str) -> sqlx::PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        let admin = sqlx::PgPool::connect(&url).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_stream_sessions (
                id               BIGSERIAL PRIMARY KEY,
                streamer_login   TEXT NOT NULL,
                started_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                ended_at         TIMESTAMPTZ,
                duration_seconds INTEGER,
                followers_start  INTEGER,
                followers_end    INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[cfg(feature = "integration")]
    fn candidate(login: &str) -> FairnessCandidate {
        FairnessCandidate {
            user_id: format!("id_{login}"),
            user_login: login.to_string(),
            viewer_count: 1,
            followers_total: FOLLOWERS_UNKNOWN,
            started_at: "2026-06-10T16:00:00Z".to_string(),
        }
    }

    /// Kandidat mit gespeicherter Session-Follower-Zahl wird aus dem Cache
    /// bedient — KEIN Helix-Call.
    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn cache_backfill_ohne_helix_call() {
        let pool = setup_sessions_db("p249_cache").await;
        // Zwei Sessions für 'alice': neuere mit followers_end=500 gewinnt.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, followers_start, followers_end)
             VALUES ('alice', NOW() - INTERVAL '5 days', NOW() - INTERVAL '5 days', 100, 200),
                    ('alice', NOW() - INTERVAL '1 day',  NOW() - INTERVAL '1 day',  400, 500)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let followers = Arc::new(CountingFollowerSource {
            calls: AtomicUsize::new(0),
            total: Some(999),
        });
        let enricher = CachedFollowerEnricher {
            followers: followers.clone(),
            pool: pool.clone(),
        };
        let mut cands = vec![candidate("alice")];
        enricher.enrich(&mut cands).await;

        assert_eq!(cands[0].followers_total, 500, "neueste Session-Zahl aus Cache");
        assert_eq!(followers.calls.load(Ordering::SeqCst), 0, "kein Helix-Call bei Cache-Treffer");
    }

    /// followers_end NULL → COALESCE fällt auf followers_start zurück.
    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn cache_coalesce_auf_followers_start() {
        let pool = setup_sessions_db("p249_coalesce").await;
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at, followers_start, followers_end)
             VALUES ('bob', NOW() - INTERVAL '1 day', NOW(), 333, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let followers = Arc::new(CountingFollowerSource {
            calls: AtomicUsize::new(0),
            total: Some(999),
        });
        let enricher = CachedFollowerEnricher {
            followers: followers.clone(),
            pool: pool.clone(),
        };
        let mut cands = vec![candidate("bob")];
        enricher.enrich(&mut cands).await;
        assert_eq!(cands[0].followers_total, 333);
        assert_eq!(followers.calls.load(Ordering::SeqCst), 0);
    }

    /// Kandidat OHNE Session-Cache fällt auf Helix zurück.
    #[cfg(feature = "integration")]
    #[tokio::test]
    async fn ohne_cache_faellt_auf_helix_zurueck() {
        let pool = setup_sessions_db("p249_nohit").await;
        let followers = Arc::new(CountingFollowerSource {
            calls: AtomicUsize::new(0),
            total: Some(777),
        });
        let enricher = CachedFollowerEnricher {
            followers: followers.clone(),
            pool: pool.clone(),
        };
        let mut cands = vec![candidate("ghost")];
        enricher.enrich(&mut cands).await;
        assert_eq!(cands[0].followers_total, 777, "Helix-Fallback ohne Cache");
        assert_eq!(followers.calls.load(Ordering::SeqCst), 1, "genau ein Helix-Call");
    }

    // ─── P2.58: Raid-Subscription-Status-Poll ───────────────────────────────

    use std::sync::Mutex;
    use tb_monitoring::poller::source::SourceError;
    use tb_monitoring::{RemoteSubscription, SubscriptionTransport};

    /// Stub-Transport: gibt bei jedem `list()`-Aufruf den nächsten
    /// vorbereiteten Status für eine `channel.raid`-Sub zurück (simuliert die
    /// Verifikations-Sequenz pending → enabled). `None` = keine Sub gelistet.
    struct SequenceTransport {
        broadcaster_id: String,
        statuses: Mutex<std::collections::VecDeque<Option<&'static str>>>,
        last: Mutex<Option<&'static str>>,
    }

    #[async_trait::async_trait]
    impl SubscriptionTransport for SequenceTransport {
        async fn create(
            &self,
            _sub_type: &str,
            _version: &str,
            _condition: &serde_json::Value,
            _callback: &str,
            _secret: &str,
            _bearer_override: Option<&str>,
        ) -> Result<bool, SourceError> {
            Ok(false)
        }
        async fn list(&self) -> Result<Vec<RemoteSubscription>, SourceError> {
            let next = {
                let mut q = self.statuses.lock().unwrap();
                let v = q.pop_front().flatten();
                // Letzten Status für weitere Aufrufe „einrasten".
                if let Some(v) = v {
                    *self.last.lock().unwrap() = Some(v);
                }
                v.or_else(|| *self.last.lock().unwrap())
            };
            Ok(match next {
                Some(status) => vec![RemoteSubscription {
                    id: "sub-1".into(),
                    sub_type: "channel.raid".into(),
                    status: status.into(),
                    callback: None,
                    broadcaster_user_id: Some(self.broadcaster_id.clone()),
                }],
                None => vec![],
            })
        }
        async fn delete(&self, _id: &str) -> Result<(), SourceError> {
            Ok(())
        }
    }

    fn poll_with(transport: Arc<SequenceTransport>) -> RaidSubscriptionStatusPoll {
        RaidSubscriptionStatusPoll {
            transport,
            wait_timeout: std::time::Duration::from_secs(2),
            poll_interval: std::time::Duration::from_millis(1),
        }
    }

    /// Status springt pending → enabled: wait_until_enabled gibt true zurück.
    #[tokio::test]
    async fn status_poll_wartet_bis_enabled() {
        let transport = Arc::new(SequenceTransport {
            broadcaster_id: "200".into(),
            statuses: Mutex::new(
                [Some("webhook_callback_verification_pending"), Some("enabled")].into(),
            ),
            last: Mutex::new(None),
        });
        let poll = poll_with(transport);
        assert!(poll.wait_until_enabled("200").await);
    }

    /// Bleibt pending bis Deadline → false (kein enabled in der Zeit).
    #[tokio::test]
    async fn status_poll_pending_bis_deadline_ist_false() {
        let transport = Arc::new(SequenceTransport {
            broadcaster_id: "200".into(),
            statuses: Mutex::new([Some("webhook_callback_verification_pending")].into()),
            last: Mutex::new(None),
        });
        let mut poll = poll_with(transport);
        poll.wait_timeout = std::time::Duration::from_millis(20);
        assert!(!poll.wait_until_enabled("200").await, "pending bis Deadline → nicht ready");
    }

    /// Terminaler Fehlerstatus → sofort false.
    #[tokio::test]
    async fn status_poll_terminaler_fehler_ist_false() {
        let transport = Arc::new(SequenceTransport {
            broadcaster_id: "200".into(),
            statuses: Mutex::new([Some("notification_failures_exceeded")].into()),
            last: Mutex::new(None),
        });
        let poll = poll_with(transport);
        assert!(!poll.wait_until_enabled("200").await);
    }

    /// Schon enabled beim ersten Poll → sofort true.
    #[tokio::test]
    async fn status_poll_sofort_enabled() {
        let transport = Arc::new(SequenceTransport {
            broadcaster_id: "200".into(),
            statuses: Mutex::new([Some("enabled")].into()),
            last: Mutex::new(None),
        });
        let poll = poll_with(transport);
        assert!(poll.wait_until_enabled("200").await);
    }
}
