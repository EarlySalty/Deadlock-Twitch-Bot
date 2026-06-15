//! Clip-Enrichment-Orchestrator (Port von `enrichment.py` `ClipEnrichmentPipeline`).
//!
//! Verkettet Whisper-Transkription → Vokabular-Korrektur → LLM-Anreicherung →
//! Persistenz, mit Status-Maschine (pending→transcribing→correcting→llm→done /
//! failed / skipped_no_key). Transcriber + LLM sind injizierbar (Traits), damit
//! der Orchestrator ohne echte Modelle/Netzwerk testbar ist.

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;

use crate::correction::{correct_transcript, CorrectionResult};
use crate::enrichment::{
    ensure_enrichment_row, save_corrected, save_llm_output, save_transcript,
    update_enrichment_status, STATUS_CORRECTING, STATUS_DONE, STATUS_FAILED, STATUS_LLM,
    STATUS_SKIPPED_NO_KEY, STATUS_TRANSCRIBING,
};
use crate::llm::{LlmError, LlmRequest, LlmResponse, StreamerProfile};
use crate::llm_dispatch::LlmDispatcher;
use crate::vocab::load_all_vocab;

/// Whisper-Ergebnis (Text + Segmente + Sprache).
#[derive(Debug, Clone, Default)]
pub struct TranscriptionOutput {
    pub text: String,
    pub segments: Vec<Value>,
    pub language: Option<String>,
}

/// Transkriptions-Fehler. `Unavailable`/`NotFound` → Transkription überspringen
/// (skipped); `Failed` → harte Fehlschlag.
#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    #[error("transcriber unavailable: {0}")]
    Unavailable(String),
    #[error("clip file missing: {0}")]
    NotFound(String),
    #[error("transcription failed: {0}")]
    Failed(String),
}

/// Injizierbarer Transcriber (echte Impl = OpenAI-Whisper, im Worker-Cutover).
#[async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe_clip(&self, video_path: &Path) -> Result<TranscriptionOutput, TranscribeError>;
}

/// Injizierbare LLM-Anreicherung (impl für [`LlmDispatcher`]).
#[async_trait]
pub trait EnrichmentLlm: Send + Sync {
    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError>;
}

#[async_trait]
impl EnrichmentLlm for LlmDispatcher {
    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        LlmDispatcher::generate(self, request).await
    }
}

/// Clip-Kontext (Python `EnrichmentClipContext`).
#[derive(Debug, Clone)]
pub struct ClipContext {
    pub clip_db_id: i32,
    pub clip_id: String,
    pub streamer_login: String,
    pub title: Option<String>,
    pub duration_seconds: Option<f64>,
    pub game_name: Option<String>,
    pub upload_local_path: Option<String>,
    pub local_file_path: Option<String>,
}

/// Ergebnis eines Enrichment-Laufs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichmentOutcome {
    pub clip_db_id: i32,
    pub status: String,
    pub error_message: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("clip_db_id {0} not found")]
    ClipNotFound(i32),
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Lädt den Clip-Kontext aus `twitch_clips_social_media`.
async fn load_clip_context(pool: &PgPool, clip_db_id: i32) -> Option<ClipContext> {
    let row: Option<(i32, Option<String>, Option<String>, Option<String>, Option<f64>, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT id, clip_id, streamer_login, clip_title, duration_seconds, game_name, \
                    upload_local_path, local_file_path \
             FROM twitch_clips_social_media WHERE id = $1 LIMIT 1",
        )
        .bind(clip_db_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    row.map(|r| ClipContext {
        clip_db_id: r.0,
        clip_id: r.1.unwrap_or_default(),
        streamer_login: r.2.unwrap_or_default(),
        title: r.3,
        duration_seconds: r.4,
        game_name: r.5,
        upload_local_path: r.6,
        local_file_path: r.7,
    })
}

/// Setzt den Clip in `social_media_clip_approval` auf `awaiting_approval`
/// (best-effort; volle approval-Logik = eigener Slice).
async fn mark_clip_awaiting_approval(pool: &PgPool, clip_db_id: i32) {
    let _ = sqlx::query(
        "INSERT INTO social_media_clip_approval (clip_db_id, state) VALUES ($1, 'awaiting_approval') \
         ON CONFLICT (clip_db_id) DO NOTHING",
    )
    .bind(clip_db_id)
    .execute(pool)
    .await;
}

/// Orchestriert die Clip-Enrichment.
pub struct ClipEnrichmentPipeline {
    pool: PgPool,
}

impl ClipEnrichmentPipeline {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn done_outcome(clip_db_id: i32, provider: Option<String>, model: Option<String>) -> EnrichmentOutcome {
        EnrichmentOutcome { clip_db_id, status: STATUS_DONE.to_string(), error_message: None, provider, model }
    }

    async fn fail(&self, clip_db_id: i32, status: &str, message: String) -> EnrichmentOutcome {
        let _ = update_enrichment_status(&self.pool, clip_db_id, status, Some(Some(message.clone())), None, Some(Some(now_iso()))).await;
        EnrichmentOutcome { clip_db_id, status: status.to_string(), error_message: Some(message), provider: None, model: None }
    }

    /// Führt die Pipeline aus (Python `run`).
    pub async fn run(
        &self,
        clip_db_id: i32,
        transcriber: Option<&dyn Transcriber>,
        llm: &dyn EnrichmentLlm,
        force: bool,
    ) -> Result<EnrichmentOutcome, PipelineError> {
        let ctx = load_clip_context(&self.pool, clip_db_id).await.ok_or(PipelineError::ClipNotFound(clip_db_id))?;
        let existing = ensure_enrichment_row(&self.pool, clip_db_id).await;
        if existing.status == STATUS_DONE && !force {
            return Ok(Self::done_outcome(clip_db_id, existing.llm_provider, existing.llm_model));
        }

        // ---- Transcribe ----
        let _ = update_enrichment_status(&self.pool, clip_db_id, STATUS_TRANSCRIBING, Some(None), Some(Some(now_iso())), Some(None)).await;
        let mut transcript = TranscriptionOutput::default();
        let mut skipped = false;
        let video_path = ctx.upload_local_path.clone().or_else(|| ctx.local_file_path.clone());
        match (video_path.as_deref(), transcriber) {
            (Some(path), Some(t)) => match t.transcribe_clip(Path::new(path)).await {
                Ok(out) => transcript = out,
                Err(TranscribeError::Unavailable(_) | TranscribeError::NotFound(_)) => skipped = true,
                Err(TranscribeError::Failed(e)) => return Ok(self.fail(clip_db_id, STATUS_FAILED, format!("transcription: {e}")).await),
            },
            _ => skipped = true, // kein Pfad oder kein Transcriber
        }
        let _ = save_transcript(
            &self.pool,
            clip_db_id,
            Some(transcript.text.as_str()).filter(|s| !s.is_empty()),
            &transcript.segments,
            transcript.language.as_deref(),
        )
        .await;

        // ---- Correct ----
        let _ = update_enrichment_status(&self.pool, clip_db_id, STATUS_CORRECTING, None, None, None).await;
        let vocab = load_all_vocab(&self.pool).await;
        let correction: CorrectionResult = if transcript.text.is_empty() {
            CorrectionResult { corrected: String::new(), detected_terms: Vec::new(), replacements: Vec::new() }
        } else {
            correct_transcript(&transcript.text, &vocab)
        };
        let _ = save_corrected(
            &self.pool,
            clip_db_id,
            Some(correction.corrected.as_str()).filter(|s| !s.is_empty()),
            &correction.detected_terms,
        )
        .await;

        // ---- LLM ----
        let _ = update_enrichment_status(&self.pool, clip_db_id, STATUS_LLM, None, None, None).await;
        let request = LlmRequest {
            transcript: correction.corrected.clone(),
            detected_terms: correction.detected_terms.clone(),
            streamer: Some(StreamerProfile {
                streamer_login: ctx.streamer_login.clone(),
                language: transcript.language.clone(),
                ..Default::default()
            }),
            clip_title: ctx.title.clone(),
            game_name: Some(ctx.game_name.clone().filter(|g| !g.is_empty()).unwrap_or_else(|| "Deadlock".to_string())),
            duration_seconds: ctx.duration_seconds,
        };
        let response = match llm.generate(&request).await {
            Ok(r) => r,
            Err(e) => {
                let status = if skipped { STATUS_SKIPPED_NO_KEY } else { STATUS_FAILED };
                return Ok(self.fail(clip_db_id, status, format!("llm: {e}")).await);
            }
        };

        let _ = save_llm_output(
            &self.pool,
            clip_db_id,
            &response.youtube,
            &response.tiktok,
            &response.instagram,
            &response.provider,
            Some(response.model.as_str()),
            response.cost_usd_estimate,
        )
        .await;
        let _ = update_enrichment_status(&self.pool, clip_db_id, STATUS_DONE, Some(None), None, Some(Some(now_iso()))).await;
        mark_clip_awaiting_approval(&self.pool, clip_db_id).await;

        Ok(Self::done_outcome(clip_db_id, Some(response.provider), Some(response.model)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrichment::{get_enrichment, PlatformEnrichment};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    struct MockTranscriber {
        out: Option<TranscriptionOutput>,
    }
    #[async_trait]
    impl Transcriber for MockTranscriber {
        async fn transcribe_clip(&self, _: &Path) -> Result<TranscriptionOutput, TranscribeError> {
            self.out.clone().ok_or(TranscribeError::Unavailable("no model".into()))
        }
    }

    struct MockLlm {
        response: Option<LlmResponse>,
    }
    #[async_trait]
    impl EnrichmentLlm for MockLlm {
        async fn generate(&self, _: &LlmRequest) -> Result<LlmResponse, LlmError> {
            self.response.clone().ok_or(LlmError::ProviderError("all failed".into()))
        }
    }

    fn llm_response() -> LlmResponse {
        let pe = |t: &str| PlatformEnrichment { title: Some(t.into()), description: Some("d".into()), hashtags: vec!["#Deadlock".into()] };
        LlmResponse { youtube: pe("YT"), tiktok: pe("TK"), instagram: pe("IG"), provider: "ollama".into(), model: "llama3".into(), cost_usd_estimate: Some(0.0) }
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT, streamer_login TEXT, clip_title TEXT, duration_seconds DOUBLE PRECISION, game_name TEXT, upload_local_path TEXT, local_file_path TEXT)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT NOT NULL DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval')",
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', aliases JSONB NOT NULL DEFAULT '[]'::jsonb, weight INTEGER NOT NULL DEFAULT 1, updated_at TIMESTAMPTZ DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn seed_clip(pool: &PgPool) -> i32 {
        sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login, clip_title, duration_seconds, upload_local_path) VALUES ('c1', 'nani', 'Insane', 28.0, '/x/clip.mp4') RETURNING id")
            .fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn voller_lauf_done() {
        let Some(pool) = make_pool("t_sm_pipe_done").await else { return };
        let id = seed_clip(&pool).await;
        sqlx::query("INSERT INTO deadlock_vocab (term, canonical, category) VALUES ('haze', 'Haze', 'hero')").execute(&pool).await.unwrap();
        let pipe = ClipEnrichmentPipeline::new(pool.clone());
        let transcriber = MockTranscriber { out: Some(TranscriptionOutput { text: "haze ist stark".into(), segments: vec![serde_json::json!({"text":"haze"})], language: Some("de".into()) }) };
        let llm = MockLlm { response: Some(llm_response()) };

        let outcome = pipe.run(id, Some(&transcriber), &llm, false).await.unwrap();
        assert_eq!(outcome.status, "done");
        assert_eq!(outcome.provider.as_deref(), Some("ollama"));

        let rec = get_enrichment(&pool, id).await.unwrap();
        assert_eq!(rec.status, "done");
        assert_eq!(rec.transcript_raw.as_deref(), Some("haze ist stark"));
        assert_eq!(rec.transcript_corrected.as_deref(), Some("Haze ist stark")); // korrigiert
        assert_eq!(rec.detected_terms, vec!["Haze".to_string()]);
        assert_eq!(rec.title_youtube.as_deref(), Some("YT"));
        // Approval-Zeile angelegt.
        let state: String = sqlx::query_scalar("SELECT state FROM social_media_clip_approval WHERE clip_db_id = $1").bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!(state, "awaiting_approval");

        // Zweiter Lauf ohne force → bleibt done, kein Re-Run.
        let again = pipe.run(id, Some(&transcriber), &llm, false).await.unwrap();
        assert_eq!(again.status, "done");
    }

    #[tokio::test]
    async fn skipped_no_key_wenn_llm_fehlt_und_transkription_uebersprungen() {
        let Some(pool) = make_pool("t_sm_pipe_skip").await else { return };
        let id = seed_clip(&pool).await;
        let pipe = ClipEnrichmentPipeline::new(pool.clone());
        // Kein Transcriber → skipped; LLM ohne Response → Fehler.
        let llm = MockLlm { response: None };
        let outcome = pipe.run(id, None, &llm, false).await.unwrap();
        assert_eq!(outcome.status, "skipped_no_key");
        let rec = get_enrichment(&pool, id).await.unwrap();
        assert_eq!(rec.status, "skipped_no_key");
        assert!(rec.error_message.unwrap().contains("llm"));
    }

    #[tokio::test]
    async fn clip_nicht_gefunden() {
        let Some(pool) = make_pool("t_sm_pipe_404").await else { return };
        let pipe = ClipEnrichmentPipeline::new(pool);
        let llm = MockLlm { response: None };
        assert!(matches!(pipe.run(999, None, &llm, false).await, Err(PipelineError::ClipNotFound(999))));
    }
}
