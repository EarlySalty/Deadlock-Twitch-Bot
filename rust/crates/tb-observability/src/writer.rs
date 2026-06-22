//! Deferred Observability-Event-Writer.
//!
//! Parität zu Pythons Hintergrund-Writer in `bot/storage/pg.py`
//! (`insert_observability_event` + `_observability_writer_loop` +
//! `_flush_observability_batch`, Zeilen 1199-1401): Events werden über einen
//! bounded mpsc-Channel an einen Tokio-Task übergeben, der sie gebatcht in
//! `twitch_observability_events` schreibt. Bei vollem Channel werden Events
//! verworfen (drop-Logging), damit der Hot-Path nie blockiert.
//!
//! Spalten-Säuberung (`flow_type<=40`, `flow_id/step/decision/entity_*<=80`),
//! der `decision == "failed"`-Skip und das `details_json`-Encoding entsprechen
//! Pythons `insert_observability_event`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::mpsc;

use crate::event::{ObservabilityEvent, StoragePayload};
use crate::raid_service::EventSink;
use crate::value::safe_observability_text;

const FLOW_TYPE_LIMIT: usize = 40;
const TEXT_LIMIT: usize = 80;

/// Default-Kapazität des Event-Channels (Python `TWITCH_OBSERVABILITY_QUEUE_MAXSIZE`).
pub const DEFAULT_QUEUE_CAPACITY: usize = 5000;
/// Default-Batchgröße (Python `TWITCH_OBSERVABILITY_BATCH_SIZE`).
pub const DEFAULT_BATCH_SIZE: usize = 50;

/// Eine säuberte, persistierbare Zeile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityRow {
    pub flow_type: String,
    pub flow_id: String,
    pub entity_login: Option<String>,
    pub entity_id: Option<String>,
    pub step: String,
    pub decision: String,
    pub details_json: String,
}

/// Wandelt einen Storage-Payload in eine persistierbare Zeile um und wendet die
/// Python-Säuberungsregeln an. Gibt `None` zurück, wenn das Event verworfen
/// werden muss (Pflichtfeld leer **oder** `decision == "failed"`).
pub fn sanitize_payload(payload: &StoragePayload) -> Option<ObservabilityRow> {
    let flow_type = safe_observability_text(&payload.flow_type, FLOW_TYPE_LIMIT)?;
    let flow_id = safe_observability_text(&payload.flow_id, TEXT_LIMIT)?;
    let step = safe_observability_text(&payload.step, TEXT_LIMIT)?;
    let decision = safe_observability_text(&payload.decision, TEXT_LIMIT)?;

    // Fehlerevents sind Rauschen für die DB (hohes Volumen, geringer Wert).
    if decision == "failed" {
        return None;
    }

    let entity_login = safe_observability_text(&payload.entity_login, TEXT_LIMIT);
    let entity_id = safe_observability_text(&payload.entity_id, TEXT_LIMIT);
    let details_json = encode_details(&payload.details);

    Some(ObservabilityRow {
        flow_type,
        flow_id,
        entity_login,
        entity_id,
        step,
        decision,
        details_json,
    })
}

fn encode_details(details: &std::collections::BTreeMap<String, Value>) -> String {
    let value = Value::Object(details.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

/// Handle zum Einreihen von Events. Cheap-clonebar; teilt den Channel.
#[derive(Clone)]
pub struct ObservabilityWriter {
    tx: mpsc::Sender<ObservabilityRow>,
    dropped: Arc<AtomicU64>,
}

impl ObservabilityWriter {
    /// Startet den Hintergrund-Writer-Task und liefert ein Handle.
    ///
    /// `capacity` begrenzt den Channel; `batch_size` die DB-Inserts pro Flush.
    /// Der Task läuft, bis alle Handles gedroppt sind (Channel geschlossen).
    pub fn spawn(pool: PgPool, capacity: usize, batch_size: usize) -> Self {
        let (tx, rx) = mpsc::channel::<ObservabilityRow>(capacity.max(1));
        let dropped = Arc::new(AtomicU64::new(0));
        let batch = batch_size.max(1);
        tokio::spawn(writer_loop(pool, rx, batch));
        Self { tx, dropped }
    }

    /// Reiht ein bereits gebautes Event ein. Volle Queue → Event wird verworfen
    /// (kein Blockieren des Hot-Paths). `failed`-Events werden gefiltert.
    pub fn enqueue_event(&self, event: &ObservabilityEvent) {
        self.enqueue_payload(&event.as_storage_payload());
    }

    /// Reiht einen Storage-Payload ein.
    pub fn enqueue_payload(&self, payload: &StoragePayload) {
        let Some(row) = sanitize_payload(payload) else {
            return;
        };
        if self.tx.try_send(row).is_err() {
            let count = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::debug!(
                dropped_total = count,
                "observability event dropped (queue full or writer stopped)"
            );
        }
    }

    /// Anzahl bisher verworfener Events (Diagnose/Tests).
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl EventSink for ObservabilityWriter {
    fn emit(&self, event: &ObservabilityEvent) {
        self.enqueue_event(event);
    }
}

async fn writer_loop(pool: PgPool, mut rx: mpsc::Receiver<ObservabilityRow>, batch_size: usize) {
    let mut batch: Vec<ObservabilityRow> = Vec::with_capacity(batch_size);
    loop {
        let received = rx.recv().await;
        match received {
            Some(row) => {
                batch.push(row);
                // Opportunistisch weitere bereitstehende Events einsammeln.
                while batch.len() < batch_size {
                    match rx.try_recv() {
                        Ok(next) => batch.push(next),
                        Err(_) => break,
                    }
                }
                flush_batch(&pool, &mut batch).await;
            }
            None => {
                // Channel geschlossen: Rest flushen und beenden.
                if !batch.is_empty() {
                    flush_batch(&pool, &mut batch).await;
                }
                break;
            }
        }
    }
}

async fn flush_batch(pool: &PgPool, batch: &mut Vec<ObservabilityRow>) {
    if batch.is_empty() {
        return;
    }
    if let Err(err) = insert_batch(pool, batch).await {
        tracing::debug!(
            error = %err,
            batch = batch.len(),
            "could not persist observability batch"
        );
    }
    batch.clear();
}

async fn insert_batch(pool: &PgPool, batch: &[ObservabilityRow]) -> Result<(), sqlx::Error> {
    let mut builder = sqlx::QueryBuilder::new(
        "INSERT INTO twitch_observability_events \
         (flow_type, flow_id, entity_login, entity_id, step, decision, details_json) ",
    );
    builder.push_values(batch, |mut b, row| {
        b.push_bind(&row.flow_type)
            .push_bind(&row.flow_id)
            .push_bind(&row.entity_login)
            .push_bind(&row.entity_id)
            .push_bind(&row.step)
            .push_bind(&row.decision)
            .push_bind(&row.details_json);
    });
    builder.build().execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn payload(decision: &str) -> StoragePayload {
        let mut details = BTreeMap::new();
        details.insert("reason".to_string(), json!("ok"));
        StoragePayload {
            flow_type: "analytics".into(),
            flow_id: "analytics-1".into(),
            entity_login: "login".into(),
            entity_id: "55".into(),
            step: "terminal_decision".into(),
            decision: decision.into(),
            details,
        }
    }

    #[test]
    fn sanitize_drops_failed_decisions() {
        assert!(sanitize_payload(&payload("failed")).is_none());
        assert!(sanitize_payload(&payload("success")).is_some());
    }

    #[test]
    fn sanitize_drops_when_required_field_empty() {
        let mut p = payload("success");
        p.flow_id = "   ".into();
        assert!(sanitize_payload(&p).is_none());
    }

    #[test]
    fn sanitize_clamps_flow_type_to_40_chars() {
        let mut p = payload("success");
        p.flow_type = "x".repeat(60);
        let row = sanitize_payload(&p).unwrap();
        assert_eq!(row.flow_type.len(), 40);
    }

    #[test]
    fn sanitize_nullifies_empty_entity_fields() {
        let mut p = payload("success");
        p.entity_login = "".into();
        p.entity_id = "   ".into();
        let row = sanitize_payload(&p).unwrap();
        assert_eq!(row.entity_login, None);
        assert_eq!(row.entity_id, None);
    }

    #[test]
    fn sanitize_encodes_details_json() {
        let row = sanitize_payload(&payload("success")).unwrap();
        assert_eq!(row.details_json, "{\"reason\":\"ok\"}");
    }
}
