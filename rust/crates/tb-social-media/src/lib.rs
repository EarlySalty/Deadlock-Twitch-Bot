//! Social-Media-Integrationsschicht des Twitch-Bots.
//!
//! Aktuell enthält der Crate den periodischen Clip-Fetcher:
//! - `clip::repository` — sqlx-DB-Zugriff (twitch_clips_social_media, clip_fetch_history)
//! - `clip::helix`      — Twitch Helix-API (GET /clips)
//! - `clip::service`    — Orchestrierung eines Fetch-Laufs
//! - `clip::task`       — Tokio-Hintergrundtask (Gate: TB_CLIP_FETCHER_ENABLED=1)
//!
//! Sowie die Anfänge der vollen Posting-Pipeline (Port von `bot/social_media/`):
//! - `schema`      — idempotente Tabellen-Erstellung (Port von `storage.py`).
//! - `settings`    — Key/Value-Settings (`social_media_settings`, Consent +
//!   Auto-Approve je Plattform).
//! - `credentials` — verschlüsselte Plattform-OAuth-Credentials (Lese-Pfad).
//! - `oauth`       — Multi-Plattform-OAuth-Flow (Authorize/Callback/Refresh).
//! - `refresh_worker` — periodischer Auto-Refresh ablaufender Tokens.
//! - `rendering`   — HTML-Template-Rendering (Dashboard/Terms/Privacy).
//! - `vocab`       — Deadlock-Vokabular-CRUD (`deadlock_vocab`).
//! - `correction`  — Fuzzy-Transkript-Korrektur gegen das Vokabular.
//! - `seed_vocab`  — Initial-Vokabular (Slang + Deadlock-API).
//! - `enrichment`  — Clip-Enrichment-Persistenz (`social_media_clip_enrichment`).
//!
//! # Deaktiviert
//! Der Clip-Fetcher-Task ist **standardmäßig deaktiviert** und wird nicht in
//! `tb-bot` gestartet bis die Social-Media-Pipeline bereit ist. Aktivierung:
//! Env-Var setzen + `ClipFetchTask::start_if_enabled()` aufrufen.

pub mod clip;
pub mod correction;
pub mod credentials;
pub mod enrichment;
pub mod oauth;
pub mod refresh_worker;
pub mod rendering;
pub mod schema;
pub mod seed_vocab;
pub mod settings;
pub mod vocab;

pub use clip::{
    repository::ClipRepository,
    helix::HelixClipSource,
    service::ClipFetchService,
    task::ClipFetchTask,
};

use sqlx::PgPool;
use std::sync::Arc;
use tb_transport_twitch::HelixClient;

/// Baut alle Clip-Fetcher-Komponenten und gibt einen fertigen Task zurück.
///
/// Der Task ist nach diesem Aufruf NOCH NICHT gestartet — erst
/// `ClipFetchTask::start_if_enabled()` startet den Hintergrundloop.
pub fn build_clip_fetch_task(pool: PgPool, helix: Arc<HelixClient>) -> ClipFetchTask {
    let repo = ClipRepository::new(pool);
    let helix_src = HelixClipSource::new(helix);
    let service = Arc::new(ClipFetchService::new(repo, helix_src));
    ClipFetchTask::new(service)
}
