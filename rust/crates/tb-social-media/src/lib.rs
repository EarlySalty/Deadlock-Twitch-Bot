//! Social-Media-Integrationsschicht des Twitch-Bots.
//!
//! Aktuell enthält der Crate den periodischen Clip-Fetcher:
//! - `clip::repository` — sqlx-DB-Zugriff (twitch_clips_social_media, clip_fetch_history)
//! - `clip::helix`      — Twitch Helix-API (GET /clips)
//! - `clip::service`    — Orchestrierung eines Fetch-Laufs
//! - `clip::task`       — Always-on-Tokio-Hintergrundtask wie die Python-Referenz
//!
//! Dazu die vollständige serverseitige Social-Media-Pipeline:
//! - `schema`      — idempotente Tabellen-Erstellung (Port von `storage.py`).
//! - `settings`    — allgemeine Social-Media-Einstellungen.
//! - `posting_plan` — Kadenz, Approval-Modus und hartes `release_mode`-Gate.
//! - `credentials` — verschlüsselte Plattform-OAuth-Credentials (Lese-Pfad).
//! - `oauth`       — Multi-Plattform-OAuth-Flow (Authorize/Callback/Refresh).
//! - `refresh_worker` — periodischer Auto-Refresh ablaufender Tokens.
//! - `rendering`   — HTML-Template-Rendering (Dashboard/Terms/Privacy).
//! - `vocab`       — Deadlock-Vokabular-CRUD (`deadlock_vocab`).
//! - `correction`  — Fuzzy-Transkript-Korrektur gegen das Vokabular.
//! - `seed_vocab`  — Initial-Vokabular (Slang + Deadlock-API).
//! - `enrichment`  — Clip-Enrichment-Persistenz (`social_media_clip_enrichment`).
//! - `enrichment_worker` — Background-Loop, der pending Clips anreichert.
//! - `approval`    — Approval-Workflow (State-Maschine + queue-on-approve).
//! - `approval_worker` — Queue-Seite: freigegebene Clips einreihen.
//! - `layout`      — Clip-Compositing-Layout (`social_media_streamer_layout`).
//! - `preparation` — plattformfreie Materialisierung und fingerprintgenaue Vorschau.
//! - `video_processor` — FFmpeg-Wrapper (9:16-Konvertierung + Compositing).
//! - `uploaders`    — Plattform-Uploader (TikTok/YouTube/Instagram).
//! - `upload_worker` — Queue-Verarbeitung (download→convert→upload→status).
//! - `llm`         — LLM-Typen + Prompt-Bau + Output-Parsing.
//! - `llm_dispatch` — zentraler, injizierbarer LLM-Dispatcher.
//! - `enrich_pipeline` — Orchestrator (transcribe→correct→LLM→save).
//! - `retention`   — Publication-Status (published_all ↔ pending).
//! - `retention_worker` — Cleanup-Loop für abgelaufene Clips.
//! - `clip_queue`  — Upload-Queue (`twitch_clips_upload_queue`).
//! - `clip_templates` — Beschreibungs-Templates + Last-Hashtags.
//! - `clip_manager`  — manueller Upload + Dashboard-Clip-Liste.
//! - `clip_analytics` — Analytics-Summary fürs Dashboard.
//! - `analytics`    — Clip-Statistik-Persistenz (`twitch_clips_social_analytics`).
//! - `insights_worker` — Pollt Plattform-Statistiken (24h/7d/30d).
//! - `report_writer` — Report-Aggregation + Markdown (social_media_reports).
//! - `report_dispatcher` — wöchentlicher Admin-Report-Generator (DM=B10 aus).
//!
//! # Live
//! `tb-bot` startet neun überwachte Loops: Clip-Fetch, Preparation, Upload,
//! Retention, Enrichment, Approval-Queue, Credential-Refresh, Insights und
//! Report-Dispatcher. `release_mode = prepare_only` ist der sichere Default:
//! Vorschau und Freigabe funktionieren, aber kein externer Provider wird
//! aufgerufen. Erst der adminseitige Wechsel auf `live` öffnet das Upload-Gate.

pub mod analytics;
pub mod approval;
pub mod approval_worker;
pub mod clip;
pub mod clip_analytics;
pub mod clip_manager;
pub mod clip_queue;
pub mod clip_templates;
pub mod correction;
pub mod credentials;
pub mod downloader_ipc;
pub mod enrich_pipeline;
pub mod enrichment;
pub mod enrichment_worker;
pub mod forms;
pub mod insights_worker;
pub mod layout;
pub mod llm;
pub mod llm_dispatch;
mod media_sandbox;
pub mod oauth;
pub mod partner_access;
pub mod posting_plan;
pub mod preparation;
pub mod refresh_worker;
pub mod rendering;
pub mod report_dispatcher;
pub mod report_writer;
pub mod retention;
pub mod retention_worker;
pub mod scheduler;
pub mod schema;
pub mod seed_vocab;
pub mod settings;
#[cfg(test)]
pub(crate) mod test_support;
pub mod title_gate;
pub mod upload_worker;
pub mod uploaders;
pub mod video_processor;
pub mod vocab;
pub mod vod_archive;

pub use clip::{
    helix::HelixClipSource, repository::ClipRepository, service::ClipFetchService,
    task::ClipFetchTask,
};

use sqlx::PgPool;
use std::sync::Arc;
use tb_transport_twitch::HelixClient;

/// Baut alle Clip-Fetcher-Komponenten und gibt einen fertigen Task zurück.
///
/// Der Task ist nach diesem Aufruf NOCH NICHT gestartet — erst
/// `ClipFetchTask::start()` startet den Hintergrundloop.
pub fn build_clip_fetch_task(pool: PgPool, helix: Arc<HelixClient>) -> ClipFetchTask {
    let repo = ClipRepository::new(pool);
    let helix_src = HelixClipSource::new(helix);
    let service = Arc::new(ClipFetchService::new(repo, helix_src));
    ClipFetchTask::new(service)
}
