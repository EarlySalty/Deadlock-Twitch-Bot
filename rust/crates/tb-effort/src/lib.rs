pub mod calendar;
mod evidence;
mod goal;
mod projection;
mod quests;
mod sources;
mod store;
pub mod types;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, ConnectOptions, Connection, PgConnection, PgPool};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tb_config::challenges::Challenges;
use tb_observability::WarningBudget;
use tb_transport_twitch::HelixClient;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database unavailable")]
    Database(#[from] sqlx::Error),
    #[error("invalid evidence: {0}")]
    Invalid(&'static str),
    #[error("source unavailable: {0}")]
    Source(&'static str),
    #[error("configuration unavailable")]
    Config,
    #[error("active partner not found")]
    NotFound,
}

#[derive(Clone)]
pub struct Engine {
    pub(crate) pool: PgPool,
    pub(crate) central: Option<PgPool>,
    pub(crate) cfg: Arc<Challenges>,
    pub(crate) rules_hash: String,
    pub(crate) helix: Option<HelixClient>,
    pub(crate) shared_chat_continuity_dirty: Arc<AtomicBool>,
}

pub(crate) struct SharedChatContinuityGuard {
    dirty: Arc<AtomicBool>,
    confirmed: bool,
}

impl SharedChatContinuityGuard {
    fn new(dirty: Arc<AtomicBool>) -> Self {
        Self {
            dirty,
            confirmed: false,
        }
    }

    pub(crate) fn confirm(&mut self) {
        self.dirty.store(false, Ordering::Release);
        self.confirmed = true;
    }

    pub(crate) fn leave_unchanged(&mut self) {
        self.confirmed = true;
    }
}

impl Drop for SharedChatContinuityGuard {
    fn drop(&mut self) {
        if !self.confirmed {
            self.dirty.store(true, Ordering::Release);
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Partner {
    pub twitch_user_id: String,
    pub login: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    QualifiedInvite,
    StreamerReferral,
    CoStream,
    PartyPlay,
    ClipSubmitted,
    ClipTop3,
    QuestDone,
}

impl EventKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::QualifiedInvite => "qualified_invite",
            Self::StreamerReferral => "streamer_referral",
            Self::CoStream => "co_stream",
            Self::PartyPlay => "party_play",
            Self::ClipSubmitted => "clip_submitted",
            Self::ClipTop3 => "clip_top3",
            Self::QuestDone => "quest_done",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Event {
    pub partner_twitch_user_id: String,
    pub kind: EventKind,
    pub source_id: String,
    pub occurred_at: DateTime<Utc>,
    pub viewer_twitch_user_id: Option<String>,
    pub metadata: Value,
}

impl Engine {
    pub fn new(
        pool: PgPool,
        cfg: Challenges,
        central: Option<PgPool>,
        helix: Option<HelixClient>,
    ) -> Result<Self> {
        cfg.validate().map_err(|_| Error::Config)?;
        let rules = serde_json::to_vec(&cfg).map_err(|_| Error::Config)?;
        Ok(Self {
            pool,
            central,
            cfg: Arc::new(cfg),
            rules_hash: hex::encode(Sha256::digest(rules)),
            helix,
            shared_chat_continuity_dirty: Arc::new(AtomicBool::new(true)),
        })
    }

    pub fn readonly_central_from_pool(pool: &PgPool, database: &str) -> Result<Option<PgPool>> {
        if database.trim().is_empty() {
            return Err(Error::Config);
        }
        let options = pool
            .connect_options()
            .as_ref()
            .clone()
            .database(database)
            .application_name("twitch-partner-effort-readonly")
            .options([
                ("default_transaction_read_only", "on"),
                ("statement_timeout", "5000"),
            ])
            .disable_statement_logging();
        Ok(Some(
            PgPoolOptions::new()
                .max_connections(3)
                .acquire_timeout(Duration::from_secs(3))
                .connect_lazy_with(options),
        ))
    }

    pub async fn run(self) {
        let mut ticker = tokio::time::interval(Duration::from_secs(self.cfg.poll_seconds));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut warning_budget = WarningBudget::default();
        loop {
            ticker.tick().await;
            if let Err(error) = self.tick(Utc::now()).await {
                if let Some(suppressed_repeats) = warning_budget.allow(Instant::now()) {
                    tracing::warn!(
                        %error,
                        error_group = "effort_tick",
                        suppressed_repeats,
                        "Partner-Challenges konnten nicht vollständig aktualisiert werden"
                    );
                }
            }
        }
    }

    pub async fn tick(&self, now: DateTime<Utc>) -> Result<()> {
        if !self.cfg.enabled {
            return Ok(());
        }
        let pending_shared_chat_guard =
            SharedChatContinuityGuard::new(self.shared_chat_continuity_dirty.clone());
        // Der Tick benötigt den Pool für seine Quelltransaktionen. Der globale
        // Abschluss-Lock darf deshalb auch bei pool_max=1 keinen Slot belegen.
        let mut lock_connection = PgConnection::connect_with(&self.pool.connect_options()).await?;
        let mut lock = lock_connection.begin().await?;
        // Declare the active guard after the lock transaction so its Drop
        // records interruptions before the transaction releases the lock.
        let mut shared_chat_guard = pending_shared_chat_guard;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(713219, 27)")
            .fetch_one(&mut *lock)
            .await?;
        if !acquired {
            shared_chat_guard.leave_unchanged();
            return Ok(());
        }
        let result = self.collect(now, &mut shared_chat_guard).await;
        let settlement = self.settle(now).await;
        let result = result.and(settlement);
        self.source_state("engine", now, &result).await?;
        drop(shared_chat_guard);
        lock.commit().await?;
        result
    }

    pub async fn ensure_ready(&self, now: DateTime<Utc>) -> Result<()> {
        if !self.cfg.enabled {
            return Err(Error::Source("disabled"));
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_source_state WHERE source=ANY($1) AND healthy AND successful_at >= $2")
            .bind(vec!["invites","referrals","clips","shared_chat","steam_party","engine","category_collection"])
            .bind(now-chrono::Duration::seconds(self.cfg.poll_seconds as i64*3+self.cfg.source_timeout_seconds as i64*5))
            .fetch_one(&self.pool).await?;
        if count != 7 {
            return Err(Error::Source("not_current"));
        }
        Ok(())
    }

    pub(crate) fn central(&self) -> Result<&PgPool> {
        self.central
            .as_ref()
            .ok_or(Error::Source("central_database_not_configured"))
    }
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 20
        && id.bytes().all(|c| c.is_ascii_digit())
        && id.bytes().any(|c| c != b'0')
}
