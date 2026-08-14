//! Kurzer Stream-Audio-Transkript-Kontext für den Engagement-Layer (Port von
//! `bot/engagement/stream_transcripts.py`).
//!
//! Voice-to-Text-Segmente werden in `twitch_engagement_stream_transcripts`
//! persistiert; die Pipeline lädt die jüngsten und hängt sie als Kontext an den
//! Prompt. Die Tabelle kommt aus der Migration (Pythons lazy `_ensure_table`
//! entfällt). Capture-/Trim-Jobs nutzen die übrigen Funktionen.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

/// Ein Transkript-Segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamTranscriptSegment {
    pub channel_login: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub text: String,
    pub engine: String,
    pub model: Option<String>,
}

fn env_int(name: &str, default: i64, minimum: i64) -> i64 {
    match std::env::var(name) {
        Ok(raw) if !raw.trim().is_empty() => match raw.trim().parse::<i64>() {
            Ok(v) => v.max(minimum),
            Err(_) => default,
        },
        _ => default,
    }
}

fn env_float(name: &str, default: f64, minimum: f64) -> f64 {
    match std::env::var(name) {
        Ok(raw) if !raw.trim().is_empty() => match raw.trim().parse::<f64>() {
            Ok(v) => v.max(minimum),
            Err(_) => default,
        },
        _ => default,
    }
}

/// Capture-Dauer pro Segment (Sekunden).
pub fn transcript_capture_seconds() -> i64 {
    env_int("ENGAGEMENT_TRANSCRIPT_CAPTURE_SECONDS", 45, 10)
}

/// Poll-Intervall der Capture-Schleife (Sekunden).
pub fn transcript_poll_interval_seconds() -> f64 {
    env_float("ENGAGEMENT_TRANSCRIPT_INTERVAL_SECONDS", 75.0, 15.0)
}

/// Capture-Qualität (`audio_only` o.ä.).
pub fn transcript_quality() -> String {
    std::env::var("ENGAGEMENT_TRANSCRIPT_QUALITY")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "audio_only".to_string())
}

/// Baut das Prompt-Fragment aus den Segmenten (reiner Port von
/// `segments_to_prompt_fragment`): pro Segment „- HH:MM:SS: text", auf das
/// Zeichenbudget gekürzt (vom Ende, erste angeschnittene Zeile weg). "" wenn leer.
pub fn segments_to_prompt_fragment(
    segments: &[StreamTranscriptSegment],
    max_chars: Option<i64>,
) -> String {
    if segments.is_empty() {
        return String::new();
    }
    let budget = max_chars
        .unwrap_or_else(|| env_int("ENGAGEMENT_TRANSCRIPT_PROMPT_MAX_CHARS", 1200, 200))
        .max(0) as usize;

    let mut parts: Vec<String> = Vec::new();
    for segment in segments {
        let text = segment
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            continue;
        }
        let ts = segment.ended_at.format("%H:%M:%S");
        parts.push(format!("- {ts}: {text}"));
    }
    let joined = parts.join("\n");
    let joined = if joined.chars().count() > budget {
        // Letzte `budget` Zeichen, linksstrippen, erste angeschnittene Zeile weg.
        let start = joined.chars().count() - budget;
        let tail: String = joined.chars().skip(start).collect();
        let tail = tail.trim_start();
        tail.split_once('\n')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_else(|| tail.to_string())
    } else {
        joined
    };
    if joined.is_empty() {
        return String::new();
    }
    format!(
        "Aktueller Stream-Audio-Kontext aus Voice-to-Text. \
         Nutze ihn nur, wenn er zur Chat-Nachricht passt; er kann unvollständig sein.\n{joined}"
    )
}

/// Transkript-Provider.
pub struct StreamTranscripts {
    pool: PgPool,
}

impl StreamTranscripts {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Hängt ein Segment an (Text whitespace-normalisiert; leer → kein Insert).
    pub async fn append_segment(
        &self,
        segment: &StreamTranscriptSegment,
    ) -> Result<(), sqlx::Error> {
        let text = segment
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            return Ok(());
        }
        sqlx::query!(
            "INSERT INTO twitch_engagement_stream_transcripts \
             (channel_login, started_at, ended_at, text, engine, model) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &segment.channel_login,
            segment.started_at,
            segment.ended_at,
            &text,
            &segment.engine,
            segment.model.as_deref()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lädt die jüngsten Segmente eines Channels (chronologisch, leere gefiltert).
    /// `None`-Parameter ziehen aus Env bzw. Defaults (15min / 8).
    pub async fn load_recent_segments(
        &self,
        channel_login: &str,
        max_age_minutes: Option<i64>,
        limit: Option<i64>,
    ) -> Vec<StreamTranscriptSegment> {
        let max_age = max_age_minutes
            .unwrap_or_else(|| env_int("ENGAGEMENT_TRANSCRIPT_CONTEXT_MINUTES", 15, 1));
        let limit = limit.unwrap_or_else(|| env_int("ENGAGEMENT_TRANSCRIPT_CONTEXT_LIMIT", 8, 1));
        let cutoff = Utc::now() - Duration::minutes(max_age);

        let rows = sqlx::query!(
            r#"SELECT channel_login AS "channel_login!", started_at AS "started_at!",
                    ended_at AS "ended_at!", text AS "text!", engine AS "engine!", model
             FROM twitch_engagement_stream_transcripts
             WHERE channel_login = $1 AND ended_at >= $2
             ORDER BY ended_at DESC LIMIT $3"#,
            channel_login,
            cutoff,
            limit
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .rev()
            .map(|r| StreamTranscriptSegment {
                channel_login: r.channel_login,
                started_at: r.started_at,
                ended_at: r.ended_at,
                text: r.text,
                engine: r.engine,
                model: r.model.filter(|m| !m.is_empty()),
            })
            .filter(|s| !s.text.trim().is_empty())
            .collect()
    }

    /// Räumt alte/überzählige Segmente auf (created_at < cutoff ODER Rang
    /// > keep_per_channel). Liefert die Anzahl gelöschter Zeilen.
    pub async fn trim_segments(
        &self,
        max_age_minutes: Option<i64>,
        keep_per_channel: Option<i64>,
    ) -> u64 {
        let max_age = max_age_minutes
            .unwrap_or_else(|| env_int("ENGAGEMENT_TRANSCRIPT_RETENTION_MINUTES", 60, 1));
        let keep = keep_per_channel
            .unwrap_or_else(|| env_int("ENGAGEMENT_TRANSCRIPT_KEEP_PER_CHANNEL", 40, 1));
        let cutoff = Utc::now() - Duration::minutes(max_age);

        match sqlx::query!(
            "DELETE FROM twitch_engagement_stream_transcripts \
             WHERE created_at < $1 OR id IN (\
               SELECT id FROM (\
                 SELECT id, ROW_NUMBER() OVER (\
                   PARTITION BY channel_login ORDER BY ended_at DESC) AS rn \
                 FROM twitch_engagement_stream_transcripts) ranked \
               WHERE rn > $2)",
            cutoff,
            keep
        )
        .execute(&self.pool)
        .await
        {
            Ok(result) => result.rows_affected(),
            Err(error) => {
                tracing::warn!(
                    %error,
                    max_age_minutes = max_age,
                    keep_per_channel = keep,
                    "stream-transcripts: Trim fehlgeschlagen"
                );
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn seg(text: &str, ts: i64) -> StreamTranscriptSegment {
        let t = DateTime::from_timestamp(ts, 0).unwrap();
        StreamTranscriptSegment {
            channel_login: "nani".to_string(),
            started_at: t - Duration::seconds(40),
            ended_at: t,
            text: text.to_string(),
            engine: "whisper".to_string(),
            model: None,
        }
    }

    #[test]
    fn fragment_format_und_budget() {
        assert_eq!(segments_to_prompt_fragment(&[], None), "");
        let segs = vec![
            seg("erster satz", 1_700_000_000),
            seg("zweiter satz", 1_700_000_060),
        ];
        let frag = segments_to_prompt_fragment(&segs, None);
        assert!(frag.contains("Stream-Audio-Kontext"));
        assert!(frag.contains(": erster satz"));
        assert!(frag.contains(": zweiter satz"));

        // Budget kürzt von vorne (erste angeschnittene Zeile fällt weg).
        let many: Vec<_> = (0..20)
            .map(|i| {
                seg(
                    &format!("zeile nummer {i} mit text"),
                    1_700_000_000 + i * 60,
                )
            })
            .collect();
        let cut = segments_to_prompt_fragment(&many, Some(60));
        assert!(cut.chars().count() < 400); // deutlich gekürzt
        assert!(cut.contains("Stream-Audio-Kontext"));
    }

    #[test]
    fn config_getter_defaults() {
        // Nur valide, wenn die Env nicht gesetzt ist.
        if std::env::var("ENGAGEMENT_TRANSCRIPT_CAPTURE_SECONDS").is_err() {
            assert_eq!(transcript_capture_seconds(), 45);
        }
        if std::env::var("ENGAGEMENT_TRANSCRIPT_QUALITY").is_err() {
            assert_eq!(transcript_quality(), "audio_only");
        }
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
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
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_stream_transcripts (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, \
             started_at TIMESTAMPTZ NOT NULL, ended_at TIMESTAMPTZ NOT NULL, \
             text TEXT NOT NULL, engine TEXT NOT NULL, model TEXT, \
             created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn append_und_load_chronologisch() {
        let Some(pool) = make_pool("t_eng_transcript").await else {
            return;
        };
        let st = StreamTranscripts::new(pool.clone());
        // Zwei frische Segmente + ein leeres (wird nicht inserted).
        let now = Utc::now();
        let mut a = seg("älteres segment", 0);
        a.started_at = now - Duration::minutes(5);
        a.ended_at = now - Duration::minutes(5);
        let mut b = seg("neueres   segment", 0); // doppelte Spaces → normalisiert
        b.started_at = now - Duration::minutes(2);
        b.ended_at = now - Duration::minutes(2);
        st.append_segment(&a).await.unwrap();
        st.append_segment(&b).await.unwrap();
        st.append_segment(&seg("   ", 0)).await.unwrap(); // leer → kein Insert

        let segs = st.load_recent_segments("nani", Some(15), Some(8)).await;
        assert_eq!(segs.len(), 2);
        // chronologisch (älteste zuerst).
        assert_eq!(segs[0].text, "älteres segment");
        assert_eq!(segs[1].text, "neueres segment"); // normalisiert
    }
}
