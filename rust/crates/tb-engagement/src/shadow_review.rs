//! Shadow→Discord-Review-Ausgang (Ticket B19-shadow-discord-out).
//!
//! Bei `output_mode='shadow'` erzeugt die [`pipeline`](crate::pipeline) eine
//! KI-Antwort, sendet sie NICHT in den Chat, sondern staged sie ins
//! `twitch_engagement_log` mit `decision='shadowed'` (Text in `response_text`).
//!
//! Dieses Modul baut den Review-Ausgang der **tb-engagement-Seite**: es liest
//! die noch nicht weitergeleiteten Shadow-Zeilen, reicht sie über den Port
//! [`ShadowReviewSink`] zur Review-Auslieferung weiter und markiert sie danach
//! per `shadow_forwarded_at` als erledigt (Idempotenz → kein Doppel-Versand).
//!
//! **Bewusst NICHT hier:** die konkrete Discord-Send-Implementierung des Sinks
//! (via `tb-transport-discord`) und der periodische Scheduler. Beides gehört in
//! `bin/tb-bot`, das den Port implementiert und [`forward_pending_reviews`]
//! ruft. Dieses Modul ist transport- und scheduler-frei.
//!
//! **Kein Effekt im Normalbetrieb:** Es werden ausschließlich `'shadowed'`-Zeilen
//! betrachtet, die nur bei opt-in `output_mode='shadow'` entstehen.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Eine gestagte Shadow-Antwort, die zum Discord-Review ausgeliefert wird.
///
/// Spiegelt eine Zeile aus `twitch_engagement_log` mit `decision='shadowed'`,
/// reduziert auf die fürs Review relevanten Felder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowReviewItem {
    /// Primärschlüssel der Log-Zeile (`twitch_engagement_log.id`). Wird zum
    /// Markieren der Weiterleitung gebraucht.
    pub id: i64,
    /// Channel, dessen Shadow-KI die Antwort erzeugt hat.
    pub channel_login: String,
    /// Die gestagte KI-Antwort (Inhalt der `response_text`-Spalte).
    pub response_text: String,
    /// Twitch-Message-ID, die den Lauf ausgelöst hat (falls vorhanden).
    pub triggered_by_msg_id: Option<String>,
    /// Modell, das die Antwort erzeugt hat (z. B. `MiniMax-M3`).
    pub model: String,
    /// Zeitpunkt der Stagung (`ts`-Spalte).
    pub created_at: DateTime<Utc>,
}

/// Fehlertyp des Review-Ausgangs.
#[derive(Debug, thiserror::Error)]
pub enum ShadowReviewError {
    /// DB-Zugriff fehlgeschlagen (Query/Mark).
    #[error("DB-Fehler beim Shadow-Review: {0}")]
    Db(#[from] sqlx::Error),
    /// Der Sink konnte die Items nicht zum Review ausliefern.
    #[error("Shadow-Review-Sink schlug fehl: {0}")]
    Sink(String),
}

/// Port für die Auslieferung gestagter Shadow-Antworten zum Review.
///
/// Die tb-engagement-Seite stellt nur diesen Trait bereit; die konkrete
/// Implementierung (Discord-Send via `tb-transport-discord`) lebt in
/// `bin/tb-bot`. Liefert der Sink erfolgreich, gelten **alle** übergebenen
/// Items als weitergeleitet und werden anschließend markiert; schlägt er fehl,
/// bleibt nichts markiert (at-least-once, kein stiller Verlust).
#[async_trait]
pub trait ShadowReviewSink: Send + Sync {
    /// Liefert die übergebenen Shadow-Antworten zum Review aus.
    ///
    /// `Ok(())` heißt: alle Items sind sicher angekommen und dürfen als
    /// weitergeleitet markiert werden. Ein `Err` lässt die Markierung aus, der
    /// nächste Lauf reicht dieselben Items erneut ein.
    async fn forward_for_review(&self, items: &[ShadowReviewItem])
        -> Result<(), ShadowReviewError>;
}

/// Liest die noch nicht weitergeleiteten Shadow-Zeilen (älteste zuerst).
///
/// Nur `decision='shadowed' AND shadow_forwarded_at IS NULL` mit nicht-leerem
/// `response_text`. `limit` deckelt die Batch-Größe (`<= 0` → leeres Ergebnis,
/// no-op). Sortierung nach `ts` ASC → faire FIFO-Auslieferung.
pub async fn fetch_pending_reviews(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<ShadowReviewItem>, ShadowReviewError> {
    if limit <= 0 {
        return Ok(Vec::new());
    }
    let rows = sqlx::query!(
        r#"SELECT id AS "id!", channel_login AS "channel_login!",
                  response_text AS "response_text?", triggered_by_msg_id,
                  model AS "model!", ts AS "ts!"
         FROM twitch_engagement_log
         WHERE decision = 'shadowed'
           AND shadow_forwarded_at IS NULL
           AND response_text IS NOT NULL
         ORDER BY ts ASC
         LIMIT $1"#,
        limit
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            // response_text IS NOT NULL ist in der Query erzwungen; der
            // filter_map deckt nur den Option-Typ ab.
            row.response_text.map(|response_text| ShadowReviewItem {
                id: row.id,
                channel_login: row.channel_login,
                response_text,
                triggered_by_msg_id: row.triggered_by_msg_id,
                model: row.model,
                created_at: row.ts,
            })
        })
        .collect())
}

/// Markiert die gegebenen Log-Zeilen als weitergeleitet (`shadow_forwarded_at =
/// NOW()`), nur falls noch unmarkiert. Idempotent — ein erneuter Aufruf mit
/// denselben IDs überschreibt einen bereits gesetzten Zeitstempel nicht.
///
/// Gibt die Anzahl tatsächlich markierter Zeilen zurück. Leere `ids` → no-op.
pub async fn mark_forwarded(pool: &PgPool, ids: &[i64]) -> Result<u64, ShadowReviewError> {
    if ids.is_empty() {
        return Ok(0);
    }
    let res = sqlx::query!(
        "UPDATE twitch_engagement_log \
         SET shadow_forwarded_at = NOW() \
         WHERE id = ANY($1) AND shadow_forwarded_at IS NULL",
        ids
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Voll-Durchlauf des Review-Ausgangs: lädt bis zu `limit` offene Shadow-Zeilen,
/// reicht sie an den `sink` und markiert sie bei Erfolg.
///
/// Reihenfolge ist bewusst forward-then-mark: Markiert wird erst nach
/// erfolgreicher Auslieferung. Scheitert der Sink, bleibt nichts markiert und
/// derselbe Batch wird beim nächsten Lauf erneut versucht (at-least-once).
///
/// Gibt die Anzahl markierter Zeilen zurück (`0`, wenn nichts offen war).
pub async fn forward_pending_reviews(
    pool: &PgPool,
    sink: &dyn ShadowReviewSink,
    limit: i64,
) -> Result<u64, ShadowReviewError> {
    let items = fetch_pending_reviews(pool, limit).await?;
    if items.is_empty() {
        return Ok(0);
    }
    sink.forward_for_review(&items).await?;
    let ids: Vec<i64> = items.iter().map(|i| i.id).collect();
    mark_forwarded(pool, &ids).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::Mutex;

    /// Test-Fake des Sinks: merkt sich die empfangenen Items und kann auf
    /// Fehler geschaltet werden, um den Nicht-Markier-Pfad zu prüfen.
    #[derive(Default)]
    struct FakeSink {
        received: Mutex<Vec<Vec<ShadowReviewItem>>>,
        fail: bool,
    }

    #[async_trait]
    impl ShadowReviewSink for FakeSink {
        async fn forward_for_review(
            &self,
            items: &[ShadowReviewItem],
        ) -> Result<(), ShadowReviewError> {
            if self.fail {
                return Err(ShadowReviewError::Sink("erzwungener Testfehler".into()));
            }
            self.received.lock().unwrap().push(items.to_vec());
            Ok(())
        }
    }

    impl FakeSink {
        fn batches(&self) -> Vec<Vec<ShadowReviewItem>> {
            self.received.lock().unwrap().clone()
        }
    }

    /// Baut eine Temp-DB mit dem PRODUKTIVEN `twitch_engagement_log`-Schema
    /// (`ts`-Spalte + additiver `shadow_forwarded_at`-Marker aus der Migration).
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
            "CREATE TABLE twitch_engagement_log (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, triggered_by_msg_id TEXT, \
             decision TEXT NOT NULL, response_text TEXT, referenced_thread_ids BIGINT[], \
             model TEXT NOT NULL, prompt_tokens INT, completion_tokens INT, \
             cost_usd_estimate DOUBLE PRECISION, latency_ms INT, \
             ts TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
             shadow_forwarded_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    /// Hilfsfunktion: schreibt eine Log-Zeile direkt (mit kontrollierbarem `ts`).
    async fn insert_log(
        pool: &PgPool,
        decision: &str,
        text: Option<&str>,
        ts_offset_secs: i64,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO twitch_engagement_log \
             (channel_login, triggered_by_msg_id, decision, response_text, model, ts) \
             VALUES ('nani', 'm1', $1, $2, 'MiniMax-M3', NOW() + ($3 || ' seconds')::interval) \
             RETURNING id",
        )
        .bind(decision)
        .bind(text)
        .bind(ts_offset_secs.to_string())
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn fetch_nur_offene_shadowed() {
        let Some(pool) = make_pool("t_eng_shadowrev_fetch").await else {
            return;
        };
        // Älteste Shadow-Zeile zuerst (ts -10), dann eine jüngere (ts 0).
        let id_old = insert_log(&pool, "shadowed", Some("alt"), -10).await;
        let id_new = insert_log(&pool, "shadowed", Some("neu"), 0).await;
        // Rauschen: nicht-shadowed, schon weitergeleitet, leerer Text.
        insert_log(&pool, "spoke", Some("gesendet"), -5).await;
        insert_log(&pool, "shadowed", None, -5).await; // kein Text → ausgeschlossen
        let forwarded = insert_log(&pool, "shadowed", Some("schon raus"), -20).await;
        mark_forwarded(&pool, &[forwarded]).await.unwrap();

        let items = fetch_pending_reviews(&pool, 50).await.unwrap();
        assert_eq!(items.len(), 2, "nur offene shadowed mit Text");
        // FIFO: ältere Stagung zuerst.
        assert_eq!(items[0].id, id_old);
        assert_eq!(items[0].response_text, "alt");
        assert_eq!(items[0].channel_login, "nani");
        assert_eq!(items[0].model, "MiniMax-M3");
        assert_eq!(items[0].triggered_by_msg_id.as_deref(), Some("m1"));
        assert_eq!(items[1].id, id_new);
        assert!(items[0].created_at <= items[1].created_at);

        // limit deckelt.
        assert_eq!(fetch_pending_reviews(&pool, 1).await.unwrap().len(), 1);
        // limit <= 0 → leer (no-op).
        assert!(fetch_pending_reviews(&pool, 0).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn forward_markiert_und_ist_idempotent() {
        let Some(pool) = make_pool("t_eng_shadowrev_forward").await else {
            return;
        };
        insert_log(&pool, "shadowed", Some("eins"), -2).await;
        insert_log(&pool, "shadowed", Some("zwei"), -1).await;

        let sink = FakeSink::default();
        let marked = forward_pending_reviews(&pool, &sink, 50).await.unwrap();
        assert_eq!(marked, 2, "beide Zeilen markiert");

        let batches = sink.batches();
        assert_eq!(batches.len(), 1, "ein Batch ausgeliefert");
        assert_eq!(batches[0].len(), 2);

        // Idempotenz: zweiter Lauf findet nichts mehr → kein Sink-Call, 0 markiert.
        let again = forward_pending_reviews(&pool, &sink, 50).await.unwrap();
        assert_eq!(again, 0);
        assert_eq!(sink.batches().len(), 1, "kein erneuter Sink-Call");

        // Alle Zeilen tragen jetzt einen Marker.
        let still_open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_engagement_log \
             WHERE decision='shadowed' AND shadow_forwarded_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(still_open, 0);
    }

    #[tokio::test]
    async fn sink_fehler_markiert_nichts() {
        let Some(pool) = make_pool("t_eng_shadowrev_failsink").await else {
            return;
        };
        insert_log(&pool, "shadowed", Some("eins"), -1).await;

        let sink = FakeSink {
            fail: true,
            ..Default::default()
        };
        let err = forward_pending_reviews(&pool, &sink, 50).await.unwrap_err();
        assert!(matches!(err, ShadowReviewError::Sink(_)));

        // Nichts markiert → bleibt offen, nächster Lauf reicht erneut ein.
        let open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_engagement_log \
             WHERE decision='shadowed' AND shadow_forwarded_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(open, 1);
    }

    #[tokio::test]
    async fn forward_ohne_offene_ist_noop() {
        let Some(pool) = make_pool("t_eng_shadowrev_empty").await else {
            return;
        };
        insert_log(&pool, "spoke", Some("nur live"), 0).await;
        let sink = FakeSink::default();
        let marked = forward_pending_reviews(&pool, &sink, 50).await.unwrap();
        assert_eq!(marked, 0);
        assert!(
            sink.batches().is_empty(),
            "kein Sink-Call ohne offene Zeilen"
        );
    }

    #[tokio::test]
    async fn mark_forwarded_leere_ids_noop() {
        let Some(pool) = make_pool("t_eng_shadowrev_markempty").await else {
            return;
        };
        assert_eq!(mark_forwarded(&pool, &[]).await.unwrap(), 0);
    }
}
