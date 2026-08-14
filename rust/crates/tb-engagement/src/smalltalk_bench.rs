//! Backtest des Smalltalks gegen echte eigene Chatzeilen.
//!
//! Die Frage ist nicht "klingt das gut", sondern "haette ein Mensch das an
//! dieser Stelle so getippt". Beides laesst sich nur beantworten, wenn man den
//! Vergleich hat, und den gibt es: `twitch_engagement_reaction_samples` haelt
//! zu jeder eigenen Chatzeile den Stream-Ton und den Chat der Sekunden davor.
//!
//! Der Lauf fuettert der KI nur den Reiz, nie die echte Antwort, und legt beide
//! Zeilen nebeneinander. Ausgewertet wird zweifach:
//!
//! 1. **Stil in Zahlen** ([`crate::smalltalk_v1::style_profile`]) fuer beide
//!    Seiten. Das faengt die groben Abweichungen, die keinem Richter auffallen
//!    muessen: doppelt so lange Zeilen, staendig derselbe Satzanfang.
//! 2. **Blindprobe**: ein Modell bekommt beide Zeilen ohne Kontext und ohne
//!    Reihenfolge-Hinweis und soll die KI heraussuchen. Trefferquote nahe 50
//!    Prozent heisst ununterscheidbar, alles ab 80 Prozent heisst auffaellig.
//!
//! Die Reihenfolge in der Blindprobe haengt an der Sample-ID statt am Zufall.
//! Ein wiederholter Lauf ueber dieselben Samples soll dieselbe Anordnung sehen,
//! sonst misst man beim Vergleich zweier Prompt-Varianten zur Haelfte die
//! Reihenfolge mit.
//!
//! # Anbieter und Datenweg
//!
//! Erzeugen und Richten laufen ueber denselben Client wie der Livebetrieb
//! ([`crate::llm_chat::EngagementLlmClient`], zentral aufgeloest ueber
//! `tb_llm::endpoint_for("engagement")`). Kein eigener Bench-Provider, kein
//! eigenes Bench-Modell und damit auch kein zweiter Datenweg nach draussen.
//! Wer das Modell wechseln will, dreht an der zentralen Auswahl, dann misst der
//! Backtest genau das, was der Bot spaeter wirklich sagt.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::llm_chat::{EngagementLlmClient, GenerateError};

/// Ein Stimulus-Response-Paar aus dem Lernmodus.
#[derive(Debug, Clone)]
pub struct BenchSample {
    pub id: i64,
    pub channel_login: String,
    pub message_ts: DateTime<Utc>,
    pub human_text: String,
    pub stream_context: String,
    pub chat_context: String,
}

/// Das Ergebnis zu einem Sample, so wie es abgelegt wird.
#[derive(Debug, Clone)]
pub struct BenchLine {
    pub sample_id: i64,
    pub channel_login: String,
    pub message_ts: DateTime<Utc>,
    pub human_text: String,
    pub stream_context: String,
    pub chat_context: String,
    /// `None` = Schweigen oder vom Ausgabefilter verworfen.
    pub ai_text: Option<String>,
    pub reject_reason: Option<String>,
    pub latency_ms: Option<i32>,
    pub judge: Option<JudgeVerdict>,
}

/// Urteil der Blindprobe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgePick {
    /// Der Richter hat die KI-Zeile gewaehlt (also richtig getippt).
    Ai,
    /// Der Richter hat die menschliche Zeile fuer die KI gehalten.
    Human,
    /// Antwort nicht auswertbar.
    Unsure,
}

impl JudgePick {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ai => "ai",
            Self::Human => "human",
            Self::Unsure => "unsure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JudgeVerdict {
    pub pick: JudgePick,
    /// `None`, wenn das Urteil nicht auswertbar war.
    pub correct: Option<bool>,
}

/// Laedt Samples fuer den Lauf, juengste zuerst.
///
/// `only_with_stream` ist der Normalfall: ohne Stream-Ton fehlt genau der Reiz,
/// auf den der Mensch reagiert hat, und die KI wuerde fuer eine Lage bewertet,
/// die sie nie gesehen hat.
pub async fn load_samples(
    pool: &PgPool,
    limit: i64,
    channel: Option<&str>,
    only_with_stream: bool,
) -> Result<Vec<BenchSample>, sqlx::Error> {
    sqlx::query_as::<_, (i64, String, DateTime<Utc>, String, String, String)>(
        "SELECT id, channel_login, message_ts, my_message, stream_context, chat_context
           FROM twitch_engagement_reaction_samples
          WHERE ($1::TEXT IS NULL OR channel_login = $1)
            AND (NOT $2::BOOLEAN OR has_stream_context)
            AND coalesce(verdict, '') <> 'bad'
          ORDER BY message_ts DESC
          LIMIT $3",
    )
    .bind(channel)
    .bind(only_with_stream)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map(|rows| {
        rows.into_iter()
            .map(
                |(id, channel_login, message_ts, human_text, stream_context, chat_context)| {
                    BenchSample {
                        id,
                        channel_login,
                        message_ts,
                        human_text,
                        stream_context,
                        chat_context,
                    }
                },
            )
            .collect()
    })
}

/// Ablage der Laeufe.
#[derive(Clone)]
pub struct BenchStore {
    pool: PgPool,
}

impl BenchStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Legt den Lauf an und gibt seine ID zurueck.
    pub async fn start_run(
        &self,
        variant: &str,
        model: &str,
        judge_model: Option<&str>,
        note: Option<&str>,
    ) -> Result<Uuid, sqlx::Error> {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO twitch_smalltalk_bench_runs (id, variant, model, judge_model, note)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(variant)
        .bind(model)
        .bind(judge_model)
        .bind(note)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// Schreibt eine Zeile. Ein zweiter Versuch zum selben Sample ueberschreibt,
    /// damit ein abgebrochener Lauf fortgesetzt werden kann, ohne dass die
    /// Auswertung ein Sample doppelt zaehlt.
    pub async fn record_line(&self, run_id: Uuid, line: &BenchLine) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_smalltalk_bench_lines (
                 run_id, sample_id, channel_login, message_ts, human_text,
                 stream_context, chat_context, ai_text, reject_reason, latency_ms,
                 judge_pick, judge_correct)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (run_id, sample_id) DO UPDATE SET
                 ai_text = EXCLUDED.ai_text,
                 reject_reason = EXCLUDED.reject_reason,
                 latency_ms = EXCLUDED.latency_ms,
                 judge_pick = EXCLUDED.judge_pick,
                 judge_correct = EXCLUDED.judge_correct",
        )
        .bind(run_id)
        .bind(line.sample_id)
        .bind(&line.channel_login)
        .bind(line.message_ts)
        .bind(&line.human_text)
        .bind(&line.stream_context)
        .bind(&line.chat_context)
        .bind(line.ai_text.as_deref())
        .bind(line.reject_reason.as_deref())
        .bind(line.latency_ms)
        .bind(line.judge.map(|verdict| verdict.pick.as_str()))
        .bind(line.judge.and_then(|verdict| verdict.correct))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Schliesst den Lauf ab.
    pub async fn finish_run(&self, run_id: Uuid, sample_count: i32) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE twitch_smalltalk_bench_runs
                SET finished_at = NOW(), sample_count = $2
              WHERE id = $1",
        )
        .bind(run_id)
        .bind(sample_count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

const JUDGE_SYSTEM: &str = "Du pruefst Twitch-Chatzeilen. Du antwortest mit genau einem \
Buchstaben, A oder B, ohne Begruendung und ohne Satzzeichen.";

/// Baut die Blindprobe. `ai_first` entscheidet, ob die KI-Zeile als A oder als
/// B steht.
pub fn build_judge_prompt(human_text: &str, ai_text: &str, ai_first: bool) -> String {
    let (a, b) = if ai_first {
        (ai_text, human_text)
    } else {
        (human_text, ai_text)
    };
    format!(
        "Zwei Zeilen aus demselben Twitch-Chat, beide in derselben Lage geschrieben. \
Eine stammt von einem echten Zuschauer, die andere von einer KI.\n\n\
A: {a}\n\
B: {b}\n\n\
Welche stammt von der KI? Antworte nur mit A oder B."
    )
}

/// Die KI-Zeile steht bei geraden Sample-IDs vorne. Feste Regel statt Zufall,
/// damit zwei Laeufe ueber dieselben Samples vergleichbar bleiben.
pub fn ai_first_for(sample_id: i64) -> bool {
    sample_id % 2 == 0
}

/// Wertet die Modellantwort aus. Alles ausser einem klaren A oder B gilt als
/// unauswertbar; ein geratener Treffer wuerde die Quote schoenen.
pub fn parse_judge_answer(raw: &str, ai_first: bool) -> JudgeVerdict {
    let cleaned = crate::llm_chat::strip_think(raw);
    let letter = cleaned
        .chars()
        .find(|c| matches!(c, 'A' | 'B' | 'a' | 'b'))
        .map(|c| c.to_ascii_uppercase());
    let picked_first = match letter {
        Some('A') => true,
        Some('B') => false,
        _ => {
            return JudgeVerdict {
                pick: JudgePick::Unsure,
                correct: None,
            }
        }
    };
    let picked_ai = picked_first == ai_first;
    JudgeVerdict {
        pick: if picked_ai {
            JudgePick::Ai
        } else {
            JudgePick::Human
        },
        correct: Some(picked_ai),
    }
}

/// Fuehrt die Blindprobe durch.
pub async fn judge(
    client: &EngagementLlmClient,
    sample_id: i64,
    human_text: &str,
    ai_text: &str,
) -> Result<JudgeVerdict, GenerateError> {
    let ai_first = ai_first_for(sample_id);
    let prompt = build_judge_prompt(human_text, ai_text, ai_first);
    // Temperatur 0: das Urteil soll am Text haengen, nicht an der Wuerfelung.
    let raw = client
        .raw_completion(JUDGE_SYSTEM, &prompt, 200, 0.0)
        .await?;
    Ok(parse_judge_answer(&raw, ai_first))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reihenfolge_haengt_an_der_sample_id() {
        assert!(ai_first_for(2));
        assert!(!ai_first_for(3));
        // Gleiche ID, gleiche Anordnung, auch im zweiten Lauf.
        assert_eq!(ai_first_for(17), ai_first_for(17));
    }

    #[test]
    fn judge_prompt_dreht_die_seiten() {
        let vorne = build_judge_prompt("mensch", "ki", true);
        assert!(vorne.contains("A: ki"));
        assert!(vorne.contains("B: mensch"));
        let hinten = build_judge_prompt("mensch", "ki", false);
        assert!(hinten.contains("A: mensch"));
        assert!(hinten.contains("B: ki"));
    }

    #[test]
    fn treffer_wird_als_richtig_gewertet() {
        let verdict = parse_judge_answer("A", true);
        assert_eq!(verdict.pick, JudgePick::Ai);
        assert_eq!(verdict.correct, Some(true));

        let verdict = parse_judge_answer("B", true);
        assert_eq!(verdict.pick, JudgePick::Human);
        assert_eq!(verdict.correct, Some(false));

        let verdict = parse_judge_answer("b", false);
        assert_eq!(verdict.pick, JudgePick::Ai);
        assert_eq!(verdict.correct, Some(true));
    }

    #[test]
    fn think_block_verdeckt_das_urteil_nicht() {
        let verdict = parse_judge_answer("<think>B klingt glatt</think>\nB", true);
        assert_eq!(verdict.correct, Some(false));
    }

    #[test]
    fn unbrauchbare_antwort_gilt_als_unsicher() {
        let verdict = parse_judge_answer("weiss ich nicht", true);
        assert_eq!(verdict.pick, JudgePick::Unsure);
        assert_eq!(verdict.correct, None);
    }
}
