//! `RaidArrivalSink`-Adapter — verbindet die Arrival-Runtime (Plan-Dispatcher)
//! mit den echten Stores + dem Klassifikator + dem ConfirmResolver. Port der
//! Effekte aus `raid_arrival_runtime.py` (`confirm_pending_raid_arrival` u. a.).
//!
//! **Sync/Async-Brücke:** `ArrivalConfirmationService` hat synchrone Lookups,
//! die DB-Status ist aber async. Der Adapter beschafft Partner-/Known-Status
//! **vorab** per async-Query und wrappt sie in `Prefetched*`-Lookups — dann
//! klassifiziert die sync-Engine ohne await.
//!
//! Noch nicht aus `main.rs` aufgerufen (Cutover-Gate).
#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use chrono::Utc;
use sqlx::PgPool;
use tb_raid::arrival_confirmation::{KnownStreamerLookup, PartnerLookup};
use tb_raid::{
    ArrivalConfirmationService, ArrivalSignalContext, ArrivalTrackingStore, PendingRaid,
    PendingRaidStore, RaidArrivalSink, RecordArrivalInput, ScoreTrackingStore,
};

use crate::confirm_resolver::{ConfirmContext, ConfirmResolver};

// ─── Prefetched-Lookups (sync, halten vorab geladene DB-Antworten) ──────────

struct PrefetchedPartner(bool);
impl PartnerLookup for PrefetchedPartner {
    fn lookup_partner(&self, _id: Option<&str>, _login: Option<&str>) -> bool {
        self.0
    }
}

struct PrefetchedKnown(Option<bool>);
impl KnownStreamerLookup for PrefetchedKnown {
    fn lookup_known_streamer(&self, _id: Option<&str>, _login: Option<&str>) -> Option<bool> {
        self.0
    }
}

// ─── Adapter ────────────────────────────────────────────────────────────────

pub struct RaidArrivalSinkImpl {
    pool: PgPool,
    pending: Arc<Mutex<PendingRaidStore>>,
    arrival_store: ArrivalTrackingStore,
    score_tracking: ScoreTrackingStore,
    confirm_resolver: ConfirmResolver,
}

impl RaidArrivalSinkImpl {
    pub fn new(
        pool: PgPool,
        pending: Arc<Mutex<PendingRaidStore>>,
        target_game_lower: &str,
    ) -> Self {
        Self {
            arrival_store: ArrivalTrackingStore::new(pool.clone()),
            score_tracking: ScoreTrackingStore::new(pool.clone()),
            confirm_resolver: ConfirmResolver::new(pool.clone(), target_game_lower),
            pool,
            pending,
        }
    }

    /// Async-Vorabauflösung: ist das Ziel ein aktiver Partner? (`twitch_partners`)
    async fn is_target_partner(&self, to_id: &str, to_login: &str) -> bool {
        let row: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM twitch_partners
              WHERE ((NULLIF($1,'') IS NOT NULL AND twitch_user_id = $1)
                  OR (NULLIF($2,'') IS NOT NULL AND LOWER(twitch_login) = LOWER($2)))
                AND status = 'active' LIMIT 1",
        )
        .bind(to_id)
        .bind(to_login)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);
        row.is_some()
    }

    /// Async-Vorabauflösung des Quell-Streamers (`twitch_streamer_identities`):
    /// `Some(true)` = bekannt mit ID, `Some(false)` = nur Login bekannt,
    /// `None` = unbekannt (Python `resolve_known_streamer_identity`).
    async fn known_source(&self, from_id: Option<&str>, from_login: &str) -> Option<bool> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT twitch_user_id FROM twitch_streamer_identities
              WHERE ((NULLIF($1,'') IS NOT NULL AND twitch_user_id = $1)
                  OR (NULLIF($2,'') IS NOT NULL AND LOWER(twitch_login) = LOWER($2)))
              LIMIT 1",
        )
        .bind(from_id.unwrap_or(""))
        .bind(from_login)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);
        // Bekannt: hat das Signal eine from_broadcaster_id mitgeliefert?
        row.map(|_| from_id.map(|s| !s.trim().is_empty()).unwrap_or(false))
    }
}

#[async_trait::async_trait]
impl RaidArrivalSink for RaidArrivalSinkImpl {
    async fn store_pending_raid(&self, pending: &PendingRaid) {
        if let Ok(mut store) = self.pending.lock() {
            store.store(pending.clone());
        }
    }

    async fn confirm_pending_raid(
        &self,
        signal_type: &str,
        to_broadcaster_id: &str,
        to_broadcaster_login: &str,
        from_broadcaster_login: &str,
        from_broadcaster_id: Option<&str>,
        viewer_count: i32,
    ) {
        // 1. Pending entfernen (pop) — wie Python `pop_pending_raid`.
        let pending = match self.pending.lock() {
            Ok(mut store) => store.pop(to_broadcaster_id, Some(from_broadcaster_login)),
            Err(_) => None,
        };
        let Some(pending) = pending else { return };

        // 2. Partner-/Known-Status vorab async laden, dann sync klassifizieren.
        let is_partner = self
            .is_target_partner(to_broadcaster_id, to_broadcaster_login)
            .await;
        let known = self
            .known_source(from_broadcaster_id, from_broadcaster_login)
            .await;
        let svc = ArrivalConfirmationService::new(
            Box::new(PrefetchedPartner(is_partner)),
            Box::new(PrefetchedKnown(known)),
        );
        let ctx = ArrivalSignalContext {
            from_broadcaster_login,
            from_broadcaster_id,
            to_broadcaster_id,
            to_broadcaster_login: Some(to_broadcaster_login),
        };
        let Some(decision) =
            svc.confirm_pending_raid_arrival(pending, &ctx, signal_type, None, None, None)
        else {
            return;
        };

        // 3. Bei Partner-Ziel: Arrival-Tracking schreiben.
        if decision.target_is_partner {
            let _ = self
                .arrival_store
                .record_arrival(&RecordArrivalInput {
                    from_broadcaster_id: from_broadcaster_id.map(str::to_string),
                    from_broadcaster_login: from_broadcaster_login.to_string(),
                    to_broadcaster_id: to_broadcaster_id.to_string(),
                    to_broadcaster_login: to_broadcaster_login.to_string(),
                    viewer_count,
                    classification: decision.classification.clone().unwrap_or_default(),
                    confirmation_signals: signal_type.to_string(),
                    primary_signal: signal_type.to_string(),
                    correlation_status: "confirmed".to_string(),
                    correlation_detail: decision.suppression_reason.clone(),
                    source_resolution: decision.source_resolution.clone(),
                    raid_history_id: None,
                    raid_history_executed_at: None,
                    unraid_seen: false,
                })
                .await;
        }

        // 4. Bei ours_to_partner: bestätigten Partner-Raid tracken (Score-Effekt).
        if decision.should_track_confirmed_partner_raid {
            let confirm_ctx = ConfirmContext {
                signal_type,
                to_broadcaster_id,
                to_broadcaster_login,
                from_broadcaster_login,
                from_broadcaster_id,
                viewer_count,
            };
            match self
                .confirm_resolver
                .resolve(&confirm_ctx, Utc::now())
                .await
            {
                Ok(input) => {
                    let _ = self.score_tracking.track_confirmed(&input).await;
                }
                Err(error) => {
                    tracing::error!(%error, "Confirm-Resolver fehlgeschlagen");
                }
            }
        }
    }

    async fn record_pending_observation(
        &self,
        pending: &PendingRaid,
        signal_type: &str,
        status: &str,
        reason: Option<&str>,
        detail: Option<&str>,
    ) {
        // Diagnostische Beobachtung auf dem gespeicherten Pending vermerken.
        if let Ok(mut store) = self.pending.lock() {
            if let Some(mut existing) = store.pop(
                &pending.to_broadcaster_id,
                Some(&pending.from_broadcaster_login),
            ) {
                existing.record_signal_observation(
                    signal_type,
                    status,
                    reason.map(str::to_string),
                    detail.map(str::to_string),
                );
                store.store(existing);
            }
        }
    }

    async fn record_secondary_signal(
        &self,
        _signal_type: &str,
        _from_broadcaster_login: &str,
        _from_broadcaster_id: Option<&str>,
        _to_broadcaster_login: &str,
        _to_broadcaster_id: &str,
        _viewer_count: i32,
        _unraid_seen: bool,
    ) {
        // Sekundär-Signal (z. B. doppelte Arrival-Notification) — aktualisiert
        // ein bestehendes Arrival-Tracking. Folgeeffekt, hier best-effort no-op
        // bis das Arrival-Update an confirmation_signals angebunden ist.
    }

    async fn store_orphan_chat_notification(
        &self,
        _to_broadcaster_id: &str,
        _to_broadcaster_login: &str,
        _from_broadcaster_id: Option<&str>,
        _from_broadcaster_login: &str,
        _viewer_count: i32,
        _message_id: Option<&str>,
        _event_timestamp: Option<&str>,
    ) {
        // Verwaiste Chat-Notification (Raid-Signal ohne Pending) — Python legt
        // einen Orphan-Eintrag an für spätere Korrelation. Eigener Store noch
        // nicht portiert; hier dokumentierter no-op (kein bestätigter Raid).
    }

    async fn mark_manual_raid_started(&self, _source_key: &str, _ttl_seconds: f64) {
        // Manual-Raid-TTL-Lock — verhindert Doppel-Auto-Raid kurz nach manuellem
        // Raid. Eigener Lock-Store; hier no-op bis verdrahtet.
    }

    async fn record_independent_raid_arrival(
        &self,
        _signal_type: &str,
        _from_broadcaster_login: &str,
        _from_broadcaster_id: Option<&str>,
        _to_broadcaster_login: &str,
        _to_broadcaster_id: &str,
        _viewer_count: i32,
    ) {
        // Unabhängiger Raid-Arrival ohne Pending-Kontext (manueller Raid ohne
        // vorherige Registrierung) — best-effort no-op bis Arrival-Record-Pfad
        // ohne Pending angebunden ist.
    }
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tb_raid::PendingRaid;

    async fn setup(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
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
        for ddl in [
            "CREATE TABLE twitch_partners (twitch_user_id TEXT, twitch_login TEXT, status TEXT)",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, last_started_at TEXT, last_game TEXT, active_session_id BIGINT)",
            "CREATE TABLE twitch_raid_history (id BIGSERIAL PRIMARY KEY, from_broadcaster_id TEXT, from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT, executed_at TIMESTAMPTZ, success BOOLEAN)",
            "CREATE TABLE twitch_partner_raid_scores (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT DEFAULT '', avg_duration_sec INTEGER DEFAULT 0, time_pattern_score_base DOUBLE PRECISION DEFAULT 0.5, received_successful_raids_total INTEGER DEFAULT 0, is_new_partner_preferred INTEGER DEFAULT 0, new_partner_multiplier DOUBLE PRECISION DEFAULT 1.0, raid_boost_multiplier DOUBLE PRECISION DEFAULT 1.0, is_live INTEGER DEFAULT 0, current_started_at TEXT, current_uptime_sec INTEGER DEFAULT 0, duration_score DOUBLE PRECISION DEFAULT 0.5, time_pattern_score DOUBLE PRECISION DEFAULT 0.5, readiness_score DOUBLE PRECISION DEFAULT 0.5, fairness_score DOUBLE PRECISION DEFAULT 0.5, base_score DOUBLE PRECISION DEFAULT 0.5, final_score DOUBLE PRECISION DEFAULT 0.5, internal_sent_raids_30d INTEGER DEFAULT 0, internal_received_raids_30d INTEGER DEFAULT 0, internal_received_raids_7d INTEGER DEFAULT 0, today_received_raids INTEGER DEFAULT 0, last_computed_at TEXT DEFAULT '')",
            "CREATE TABLE twitch_raid_arrival_tracking (id SERIAL PRIMARY KEY, detected_at TIMESTAMPTZ DEFAULT NOW(), last_signal_at TIMESTAMPTZ, from_broadcaster_id TEXT, from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT, viewer_count INTEGER, classification TEXT, confirmation_signals TEXT, primary_signal TEXT, correlation_status TEXT, correlation_detail TEXT, source_resolution TEXT, raid_history_id BIGINT, raid_history_executed_at TIMESTAMPTZ, unraid_seen BOOLEAN, last_unraid_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_partner_raid_score_tracking (id SERIAL PRIMARY KEY, raid_history_id BIGINT, from_broadcaster_id TEXT, from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT, viewer_count INTEGER, confirmed_at TEXT, target_session_id INTEGER, target_stream_started_at TEXT, score_last_computed_at TEXT, final_score DOUBLE PRECISION, base_score DOUBLE PRECISION, duration_score DOUBLE PRECISION, time_pattern_score DOUBLE PRECISION, new_partner_multiplier DOUBLE PRECISION, raid_boost_multiplier DOUBLE PRECISION, today_received_raids INTEGER, was_deadlock_at_raid INTEGER, deadlock_continued_until TEXT, deadlock_continued_sec INTEGER, resolved_at TEXT, resolution_reason TEXT, raid_history_executed_at TIMESTAMPTZ, readiness_score DOUBLE PRECISION, fairness_score DOUBLE PRECISION)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn confirm_partner_quelle_bekannt_schreibt_arrival_und_score_tracking() {
        let pool = setup("t6e_arrival_sink").await;
        // Ziel 200 ist aktiver Partner; Quelle 100 ist bekannter Streamer mit ID.
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('200','dst','active')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ('100','src')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, last_started_at, last_game, active_session_id) VALUES ('200','2026-06-10T16:00:00+00:00','Deadlock',5)").execute(&pool).await.unwrap();

        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        // Pending-Raid 100(src) -> 200 ablegen.
        pending_store
            .lock()
            .unwrap()
            .store(PendingRaid::new("src", "200"));

        let sink = RaidArrivalSinkImpl::new(pool.clone(), pending_store.clone(), "deadlock");
        sink.confirm_pending_raid("channel.raid", "200", "dst", "src", Some("100"), 42)
            .await;

        // Pending wurde gepoppt.
        assert_eq!(pending_store.lock().unwrap().len(), 0, "Pending gepoppt");
        // Arrival-Tracking geschrieben mit ours_to_partner.
        let (cls, cnt): (String, i64) = sqlx::query_as(
            "SELECT classification, COUNT(*) OVER () FROM twitch_raid_arrival_tracking WHERE to_broadcaster_id='200'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(cnt, 1);
        assert_eq!(cls, "ours_to_partner");
        // Score-Tracking geschrieben (should_track bei ours_to_partner).
        let track_cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_partner_raid_score_tracking WHERE to_broadcaster_id='200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(track_cnt, 1, "bestaetigter Partner-Raid getrackt");
        let deadlock: i32 = sqlx::query_scalar("SELECT was_deadlock_at_raid FROM twitch_partner_raid_score_tracking WHERE to_broadcaster_id='200'").fetch_one(&pool).await.unwrap();
        assert_eq!(deadlock, 1, "live_state.last_game=Deadlock -> was_deadlock");
    }

    #[tokio::test]
    async fn confirm_nicht_partner_ziel_kein_score_tracking() {
        let pool = setup("t6e_arrival_sink_nonpartner").await;
        // Ziel 200 NICHT Partner; kein twitch_partners-Eintrag.
        let pending_store = Arc::new(Mutex::new(PendingRaidStore::new()));
        pending_store
            .lock()
            .unwrap()
            .store(PendingRaid::new("src", "200"));
        let sink = RaidArrivalSinkImpl::new(pool.clone(), pending_store.clone(), "deadlock");
        sink.confirm_pending_raid("channel.raid", "200", "dst", "src", Some("100"), 42)
            .await;

        let track_cnt: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_raid_score_tracking")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(track_cnt, 0, "kein Partner-Ziel -> kein Score-Tracking");
    }
}
