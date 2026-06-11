//! Geteilter Idempotenz-Layer der internen API.
//!
//! Exakter Port des Python-Vertrags aus `bot/internal_api/app.py`
//! (`_prepare_idempotency` / `_wait_idempotency_result` /
//! `_release_idempotency_owner`):
//!
//! - Header `Idempotency-Key`, getrimmt; fehlend/leer → Layer übersprungen.
//!   Länger als 128 Zeichen → 400 `invalid idempotency key`.
//! - Scope-Key = `METHOD|PATH|key` (Pfad OHNE Query) — gleicher Key auf
//!   verschiedenen Routen kollidiert nicht.
//! - Fingerprint = `METHOD|path_qs|canonical_json(body)` (Pfad MIT Query,
//!   Body kanonisch: Keys sortiert, kompakte Separatoren; Nicht-Objekt → `{}`).
//!   Gleicher Key + anderer Fingerprint → 409 `idempotency_conflict`.
//! - Cache: 900 s TTL, max. 2000 Einträge, lazy Cleanup (kein Timer).
//!   Replay = gecachter Status + Body + Header `X-Idempotency-Replayed: 1`.
//! - Inflight: Erstanfrage reserviert den Slot synchron (Lock), parallele
//!   Anfragen mit gleichem Key+Fingerprint warten max. 30 s auf das Ergebnis
//!   (danach 503), Inflight älter als TTL → 503 wird in den Cache geschrieben.
//! - Gecacht wird nur `cacheable && status < 500` — Fehler der Erstanfrage
//!   werden NICHT gecacht, ein Retry führt neu aus. Wartende der
//!   fehlgeschlagenen Erstanfrage bekommen den Fehler-Status zurückgespielt.

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Header-Name (Python `IDEMPOTENCY_KEY_HEADER`, `contracts.py:12`).
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";
/// Replay-Marker-Header (Python `app.py:630-634`).
pub const REPLAYED_HEADER: &str = "X-Idempotency-Replayed";

/// TTL (Python `_idempotency_ttl_seconds = 15 * 60`, `app.py:270`).
const TTL_SECS: u64 = 15 * 60;
/// Max. Cache-Einträge (Python `_idempotency_max_entries`, `app.py:271`).
const MAX_ENTRIES: usize = 2000;
/// Waiter-Timeout (Python `asyncio.wait_for(..., timeout=30.0)`, `app.py:666`).
const WAIT_TIMEOUT_SECS: u64 = 30;

// ── interne Typen ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct CacheEntry {
    fingerprint: String,
    status: u16,
    payload: Value,
    created_at: u64,
}

struct InflightEntry {
    fingerprint: String,
    rx: tokio::sync::watch::Receiver<Option<(u16, Value)>>,
    created_at: u64,
}

#[derive(Default)]
struct State {
    cache: HashMap<String, CacheEntry>,
    inflight: HashMap<String, InflightEntry>,
}

/// Shared Zustand des Idempotenz-Layers — einmal pro Router-Instanz, als
/// `Extension` eingehängt (Python: Dicts auf der Server-Instanz).
#[derive(Clone, Default)]
pub struct IdempotencyState(Arc<Mutex<State>>);

// ── Vorbereitung (Python `_prepare_idempotency`) ──────────────────────────────

/// Ergebnis der Vorbereitung — bestimmt, wie der Handler weitermacht.
pub enum Prepared {
    /// Kein (leerer) Key → Layer überspringen, Handler normal ausführen.
    Skip,
    /// Sofort diese Antwort zurückgeben (400 invalid key / 409 conflict /
    /// Replay aus dem Cache / Ergebnis bzw. Timeout einer laufenden
    /// Erstanfrage).
    Immediate(Response),
    /// Diese Anfrage ist der Owner: Handler ausführen, danach
    /// [`IdempotencyState::complete_owner`] aufrufen.
    Owner(OwnerSlot),
}

/// Reservierter Inflight-Slot des Owners.
pub struct OwnerSlot {
    scope_key: String,
    fingerprint: String,
    tx: tokio::sync::watch::Sender<Option<(u16, Value)>>,
    state: IdempotencyState,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Kanonisches JSON wie Python `canonical_json` (`app.py:458-469`):
/// Keys sortiert, kompakte Separatoren, Nicht-Objekt → `{}`.
/// serde_json serialisiert `Map` standardmäßig BTreeMap-sortiert und kompakt —
/// das entspricht `json.dumps(..., sort_keys=True, separators=(",", ":"))`.
fn canonical_json(payload: &Value) -> String {
    match payload {
        Value::Object(_) => serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string()),
        _ => "{}".to_string(),
    }
}

fn json_error(status: StatusCode, error: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": error, "message": message })),
    )
        .into_response()
}

fn conflict_response() -> Response {
    // Python app.py:617-628 / 638-649.
    json_error(
        StatusCode::CONFLICT,
        "idempotency_conflict",
        "idempotency key already used with a different request",
    )
}

fn timeout_response() -> Response {
    // Python app.py:667-672 (Waiter-Timeout, OHNE Replayed-Header).
    json_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "upstream_unavailable",
        "idempotent request timed out",
    )
}

fn replayed_response(status: u16, payload: &Value) -> Response {
    let mut resp = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(payload.clone()),
    )
        .into_response();
    resp.headers_mut()
        .insert(REPLAYED_HEADER, HeaderValue::from_static("1"));
    resp
}

impl IdempotencyState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Python `_prepare_idempotency` (`app.py:587-658`).
    ///
    /// `raw_key` = roher Header-Wert, `method`/`path` ohne Query für den
    /// Scope-Key, `path_qs` mit Query für den Fingerprint, `payload` = der
    /// geparste JSON-Body (ungültiges JSON muss VOR diesem Aufruf 400 geben,
    /// wie Pythons `_json_body`).
    pub async fn prepare(
        &self,
        raw_key: Option<&str>,
        method: &str,
        path: &str,
        path_qs: &str,
        payload: &Value,
    ) -> Prepared {
        let key = raw_key.unwrap_or("").trim().to_string();
        if key.is_empty() {
            return Prepared::Skip;
        }
        if key.len() > 128 {
            return Prepared::Immediate(json_error(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "invalid idempotency key",
            ));
        }

        let method_up = method.to_uppercase();
        let scope_key = format!("{}|{}|{}", method_up.trim(), path.trim(), key);
        let fingerprint = format!(
            "{}|{}|{}",
            method_up.trim(),
            path_qs.trim(),
            canonical_json(payload)
        );

        // Ein Lock-Scope für Cleanup + Cache-Check + Inflight-Check +
        // Reservierung — das synchrone Belegen ist der Lock gegen parallele
        // Doppel-Anfragen (Python: kein await zwischen Check und Insert).
        let rx = {
            let mut st = self.0.lock().unwrap_or_else(|e| e.into_inner());
            Self::cleanup(&mut st);

            if let Some(entry) = st.cache.get(&scope_key) {
                if entry.fingerprint != fingerprint {
                    return Prepared::Immediate(conflict_response());
                }
                return Prepared::Immediate(replayed_response(entry.status, &entry.payload));
            }

            if let Some(inflight) = st.inflight.get(&scope_key) {
                if inflight.fingerprint != fingerprint {
                    return Prepared::Immediate(conflict_response());
                }
                inflight.rx.clone()
            } else {
                let (tx, rx) = tokio::sync::watch::channel(None);
                st.inflight.insert(
                    scope_key.clone(),
                    InflightEntry {
                        fingerprint: fingerprint.clone(),
                        rx,
                        created_at: now_secs(),
                    },
                );
                return Prepared::Owner(OwnerSlot {
                    scope_key,
                    fingerprint,
                    tx,
                    state: self.clone(),
                });
            }
        };

        // Waiter-Pfad (Python `_wait_idempotency_result`, `app.py:660-681`).
        Prepared::Immediate(Self::wait_for_owner(rx).await)
    }

    async fn wait_for_owner(
        mut rx: tokio::sync::watch::Receiver<Option<(u16, Value)>>,
    ) -> Response {
        let deadline = tokio::time::Duration::from_secs(WAIT_TIMEOUT_SECS);
        let result = tokio::time::timeout(deadline, async {
            loop {
                if let Some((status, payload)) = rx.borrow().clone() {
                    return (status, payload);
                }
                if rx.changed().await.is_err() {
                    // Owner-Sender weg ohne Ergebnis → wie Python-Exception-Pfad
                    // (app.py:673-678): 500 internal_error.
                    return (
                        500,
                        serde_json::json!({
                            "error": "internal_error",
                            "message": "failed to resolve idempotent request"
                        }),
                    );
                }
            }
        })
        .await;

        match result {
            // Auch Fehler-Status des Owners wird zurückgespielt — MIT
            // Replayed-Header (Python app.py:680).
            Ok((status, payload)) => replayed_response(status, &payload),
            Err(_) => timeout_response(),
        }
    }

    /// Python `_cleanup_idempotency_cache` + `_cleanup_idempotency_inflight`
    /// (`app.py:496-541`), lazy bei jedem `prepare`/`complete_owner`.
    fn cleanup(st: &mut State) {
        let now = now_secs();
        st.cache
            .retain(|_, v| now.saturating_sub(v.created_at) <= TTL_SECS);
        if st.cache.len() > MAX_ENTRIES {
            let mut pairs: Vec<(String, u64)> = st
                .cache
                .iter()
                .map(|(k, v)| (k.clone(), v.created_at))
                .collect();
            pairs.sort_by_key(|(_, ts)| *ts);
            let overflow = st.cache.len() - MAX_ENTRIES;
            for (k, _) in pairs.into_iter().take(overflow) {
                st.cache.remove(&k);
            }
        }

        // Hängende Inflights: 503 in den Cache schreiben + Waiter auflösen
        // (Python app.py:522-539 — bewusst am `status >= 500`-Block vorbei).
        let stale: Vec<String> = st
            .inflight
            .iter()
            .filter(|(_, v)| now.saturating_sub(v.created_at) > TTL_SECS)
            .map(|(k, _)| k.clone())
            .collect();
        for key in stale {
            if let Some(entry) = st.inflight.remove(&key) {
                let payload = serde_json::json!({
                    "error": "upstream_unavailable",
                    "message": "idempotent request timed out"
                });
                st.cache.insert(
                    key,
                    CacheEntry {
                        fingerprint: entry.fingerprint,
                        status: 503,
                        payload,
                        created_at: now,
                    },
                );
                // rx im Entry hält den Channel offen; nach dem remove löst der
                // Drop des letzten Senders die Waiter über `changed()` → Err.
            }
        }
    }
}

impl OwnerSlot {
    /// Python `_release_idempotency_owner` → `_complete_idempotency_owner`
    /// (`app.py:714-762`): Ergebnis cachen (nur `cacheable && status < 500`),
    /// Waiter IMMER auflösen (auch 4xx/5xx), Slot freigeben.
    pub fn complete(self, status: u16, payload: &Value, cacheable: bool) {
        {
            let mut st = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
            if cacheable && status < 500 {
                st.cache.insert(
                    self.scope_key.clone(),
                    CacheEntry {
                        fingerprint: self.fingerprint.clone(),
                        status,
                        payload: payload.clone(),
                        created_at: now_secs(),
                    },
                );
                Self::trim_cache(&mut st);
            }
            st.inflight.remove(&self.scope_key);
        }
        // Außerhalb des Locks: Waiter auflösen.
        let _ = self.tx.send(Some((status, payload.clone())));
    }

    fn trim_cache(st: &mut State) {
        if st.cache.len() > MAX_ENTRIES {
            let mut pairs: Vec<(String, u64)> = st
                .cache
                .iter()
                .map(|(k, v)| (k.clone(), v.created_at))
                .collect();
            pairs.sort_by_key(|(_, ts)| *ts);
            let overflow = st.cache.len() - MAX_ENTRIES;
            for (k, _) in pairs.into_iter().take(overflow) {
                st.cache.remove(&k);
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn prepare_simple(state: &IdempotencyState, key: &str, body: Value) -> Prepared {
        state
            .prepare(Some(key), "POST", "/x/test", "/x/test", &body)
            .await
    }

    async fn body_json(resp: Response) -> (StatusCode, Value, bool) {
        let status = resp.status();
        let replayed = resp.headers().contains_key(REPLAYED_HEADER);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap(), replayed)
    }

    #[tokio::test]
    async fn ohne_key_wird_uebersprungen() {
        let state = IdempotencyState::new();
        assert!(matches!(
            state.prepare(None, "POST", "/x", "/x", &json!({})).await,
            Prepared::Skip
        ));
        assert!(matches!(
            state.prepare(Some("  "), "POST", "/x", "/x", &json!({})).await,
            Prepared::Skip
        ));
    }

    #[tokio::test]
    async fn key_ueber_128_zeichen_gibt_400() {
        let state = IdempotencyState::new();
        let long_key = "k".repeat(129);
        let Prepared::Immediate(resp) = prepare_simple(&state, &long_key, json!({})).await else {
            panic!("erwartet Immediate");
        };
        let (status, body, _) = body_json(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "bad_request");
        assert_eq!(body["message"], "invalid idempotency key");
    }

    #[tokio::test]
    async fn replay_liefert_gecachten_status_body_und_header() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = prepare_simple(&state, "key1", json!({"a": 1})).await else {
            panic!("Erstanfrage muss Owner sein");
        };
        slot.complete(201, &json!({"ok": true, "id": 7}), true);

        let Prepared::Immediate(resp) = prepare_simple(&state, "key1", json!({"a": 1})).await
        else {
            panic!("Replay erwartet");
        };
        let (status, body, replayed) = body_json(resp).await;
        assert_eq!(status.as_u16(), 201);
        assert_eq!(body["id"], 7);
        assert!(replayed, "X-Idempotency-Replayed muss gesetzt sein");
    }

    #[tokio::test]
    async fn gleicher_key_anderer_body_gibt_409() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = prepare_simple(&state, "key2", json!({"a": 1})).await else {
            panic!()
        };
        slot.complete(200, &json!({"ok": true}), true);

        let Prepared::Immediate(resp) = prepare_simple(&state, "key2", json!({"a": 2})).await
        else {
            panic!("Konflikt erwartet");
        };
        let (status, body, _) = body_json(resp).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"], "idempotency_conflict");
    }

    // Scope-Key enthält Methode+Pfad — gleicher Key auf anderer Route ist
    // KEIN Konflikt (Python app.py:486-494).
    #[tokio::test]
    async fn gleicher_key_andere_route_kein_konflikt() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = state
            .prepare(Some("key3"), "POST", "/a", "/a", &json!({}))
            .await
        else {
            panic!()
        };
        slot.complete(200, &json!({"ok": true}), true);

        assert!(matches!(
            state.prepare(Some("key3"), "POST", "/b", "/b", &json!({})).await,
            Prepared::Owner(_)
        ));
    }

    // Fingerprint nutzt path_qs (MIT Query): gleicher Key + gleicher Pfad +
    // andere Query → 409, nicht zwei getrennte Einträge.
    #[tokio::test]
    async fn gleiche_route_andere_query_gibt_409() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = state
            .prepare(Some("key4"), "POST", "/a", "/a?x=1", &json!({}))
            .await
        else {
            panic!()
        };
        slot.complete(200, &json!({"ok": true}), true);

        let Prepared::Immediate(resp) = state
            .prepare(Some("key4"), "POST", "/a", "/a?x=2", &json!({}))
            .await
        else {
            panic!("Konflikt erwartet");
        };
        let (status, _, _) = body_json(resp).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    // Fehler (status >= 500 oder cacheable=false) werden nicht gecacht —
    // ein Retry mit demselben Key führt neu aus.
    #[tokio::test]
    async fn fehler_wird_nicht_gecacht_retry_fuehrt_neu_aus() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = prepare_simple(&state, "key5", json!({})).await else {
            panic!()
        };
        slot.complete(500, &json!({"error": "internal_error"}), false);

        assert!(matches!(
            prepare_simple(&state, "key5", json!({})).await,
            Prepared::Owner(_)
        ));
    }

    // 4xx mit cacheable=true WIRD gecacht (Python: nur status >= 500 blockt).
    #[tokio::test]
    async fn vierxx_cacheable_wird_gecacht() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = prepare_simple(&state, "key6", json!({})).await else {
            panic!()
        };
        slot.complete(404, &json!({"error": "not_found"}), true);

        let Prepared::Immediate(resp) = prepare_simple(&state, "key6", json!({})).await else {
            panic!("Replay erwartet");
        };
        let (status, _, replayed) = body_json(resp).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(replayed);
    }

    // Parallele Zweitanfrage mit gleichem Key+Body wartet auf den Owner und
    // bekommt dessen Ergebnis mit Replayed-Header.
    #[tokio::test]
    async fn paralleler_waiter_bekommt_owner_ergebnis() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = prepare_simple(&state, "key7", json!({"x": 1})).await else {
            panic!()
        };

        let state2 = state.clone();
        let waiter = tokio::spawn(async move {
            let Prepared::Immediate(resp) =
                prepare_simple(&state2, "key7", json!({"x": 1})).await
            else {
                panic!("Waiter erwartet Immediate");
            };
            body_json(resp).await
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        slot.complete(200, &json!({"ok": true, "n": 42}), true);

        let (status, body, replayed) = waiter.await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["n"], 42);
        assert!(replayed);
    }

    // Parallele Anfrage mit gleichem Key aber anderem Body → sofort 409,
    // ohne auf den Owner zu warten.
    #[tokio::test]
    async fn paralleler_konflikt_gibt_sofort_409() {
        let state = IdempotencyState::new();
        let Prepared::Owner(_slot) = prepare_simple(&state, "key8", json!({"x": 1})).await else {
            panic!()
        };

        let Prepared::Immediate(resp) = prepare_simple(&state, "key8", json!({"x": 2})).await
        else {
            panic!("Konflikt erwartet");
        };
        let (status, _, _) = body_json(resp).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    // Nicht-Objekt-Payloads werden als {} gefingerprintet (Python app.py:482):
    // null und [] sind damit derselbe Fingerprint.
    #[tokio::test]
    async fn nicht_objekt_payload_fingerprintet_als_leeres_objekt() {
        let state = IdempotencyState::new();
        let Prepared::Owner(slot) = prepare_simple(&state, "key9", json!(null)).await else {
            panic!()
        };
        slot.complete(200, &json!({"ok": true}), true);

        let Prepared::Immediate(resp) = prepare_simple(&state, "key9", json!([1, 2])).await
        else {
            panic!("Replay erwartet (gleicher Fingerprint)");
        };
        let (status, _, replayed) = body_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert!(replayed);
    }

    // canonical_json: Schlüssel-Reihenfolge im Body darf den Fingerprint
    // nicht ändern (Python: sort_keys=True).
    #[tokio::test]
    async fn key_reihenfolge_im_body_aendert_fingerprint_nicht() {
        let state = IdempotencyState::new();
        let body_a: Value = serde_json::from_str(r#"{"b": 2, "a": 1}"#).unwrap();
        let body_b: Value = serde_json::from_str(r#"{"a": 1, "b": 2}"#).unwrap();

        let Prepared::Owner(slot) = prepare_simple(&state, "key10", body_a).await else {
            panic!()
        };
        slot.complete(200, &json!({"ok": true}), true);

        let Prepared::Immediate(resp) = prepare_simple(&state, "key10", body_b).await else {
            panic!("Replay erwartet");
        };
        let (status, _, replayed) = body_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        assert!(replayed);
    }
}
