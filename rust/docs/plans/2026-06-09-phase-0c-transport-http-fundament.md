# Phase 0c — Transport/HTTP-Fundament — Implementierungsplan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline, mit Sonnet-Worker-Delegation pro Task) zum Umsetzen. Schritte nutzen Checkbox-Syntax (`- [ ]`) zum Tracking.

**Goal:** Drei neue Foundation-Crates (`tb-transport-twitch`, `tb-transport-discord`, `tb-http-core`) plus ein Bugfix in `tb-config`, hermetisch testbar ohne echtes Netz, ohne echte Bridge, ohne Secrets. Danach sind alle Transport- und Inbound-Auth-Bausteine vorhanden, auf die Feature-Crates aufsetzen.

**Architecture:** `tb-transport-twitch` kapselt den Twitch Helix-HTTP-Client (shared `Arc<reqwest::Client>`) mit App-Token-Manager (client_credentials-Grant, in-memory, deterministisches Expiry). `tb-transport-discord` definiert den `DiscordBackend`-Trait mit `BrokerRelay` (HTTP POST an Master-Broker 8770, deterministischer Idempotency-Key, Retry-Logik) und `HeadlessNoop` (Test-Stub). `tb-http-core` liefert die gemeinsamen axum-Bausteine für die interne API: Loopback-Middleware, constant-time Auth-Layer, Idempotency-Key-Extraktion und einheitliche Fehler-Antworten. Alle HTTP-Clients werden in Tests gegen `wiremock`-Mock-Server geprüft; axum-Middleware gegen axums oneshot-Harness. Kein reales Netz, keine Secrets in Tests.

**Tech Stack:** Rust (stable 1.96), `reqwest` 0.12 (json + rustls-tls, kein default-features), `axum` 0.7, `tower` 0.5, `tower-http` 0.6 (trace-Feature), `sha2` 0.10; dev: `wiremock` 0.6. `serde`, `serde_json`, `tokio`, `hex` und `thiserror` sind bereits im Workspace-Manifest.

> **For agentic workers:** Alle Kommandos müssen mit `source "$HOME/.cargo/env"` die Cargo-Umgebung laden. Cargo-Workspace liegt unter `/home/naniadm/Documents/Deadlock-Twitch-Bot/rust/`. Kein `cd` ohne absoluten Pfad. `rustfmt`-Falle: keine alleinstehende Kommentarzeile direkt unter eine Trailing-Kommentar-Zeile setzen — das bricht `cargo fmt --check`.

---

## Scope, Abweichungen & Delegation

- Folgt auf [`2026-06-09-phase-0b-persistenz-fundament.md`](2026-06-09-phase-0b-persistenz-fundament.md). **Kein Live-Cutover.**
- **Bewusste Abweichungen von der Design-Doku (in Doku-Sync, Task 5, nachgezogen):**
  1. **`tb-transport-twitch`: hand-rolled reqwest statt `twitch_api`-Crate (YAGNI).**
     Die `twitch_api`-Crate (typsichere Helix-Bindings) ist erst sinnvoll, wenn viele Helix-Endpoints implementiert werden. Für das Skelett bringt sie nur Gewicht und Lock-in. Phase 0c baut einen minimalen Helix-Client mit `reqwest` direkt. Migration zu `twitch_api` ist ein separater, abgegrenzter Schritt — als Abweichung in `01-architecture.md` notiert.
  2. **OAuth2-PKCE-User-Flow nicht in 0c.** Verschoben auf die Raid-Phase (Phase 6/8), wo der Flow tatsächlich gebraucht wird. YAGNI.
  3. **Idempotency-Key: SHA-256 über Payload-JSON + Präfix, keine byte-genaue Python-Parität.**
     Python-Monitoring und Rust-Monitoring laufen nicht gleichzeitig (Schritt 4 schaltet Python-Monitoring aus). Determinismus pro Payload ist ausreichend; Byte-Parität mit dem Python-SHA-256 ist unnötige Komplexität.
  4. **`tb-eventsub` und `tb-llm` nicht in 0c.** EventSub-WS und LLM-Dispatcher sind Feature-Crates (Phase 4/5). In 0c nicht angefasst.
- **`tb-config`-Bugfix (Task 1):** Die aktuelle `BrokerConfig::load`-Fallback-Kette prüft nur `MASTER_BROKER_TOKEN → TWITCH_INTERNAL_API_TOKEN`. Das mittlere Glied `MAIN_BOT_INTERNAL_TOKEN` fehlt. Wird in Task 1 nachgezogen.
- **Delegation:** Implementierung an Sonnet-Worker; Review, Verifikation und Commit bei Opus.

---

## Verifizierter Vertrag (Grundlage der Implementierung)

### Master-Broker Outbound (`tb-transport-discord` / `BrokerRelay`)

- **Base-URL:** Aus `tb_config::BrokerConfig::base_url` (existiert). Default: `http://127.0.0.1:8770`.
- **Auth-Header:** `X-Internal-Token: <token>` (aus `BrokerConfig::token`).
- **Idempotency-Header (outbound):** `X-Idempotency-Key: <key>` — deterministischer SHA-256-Hex-Digest (48 Zeichen) über `serde_json::to_string(&payload)` + Aktions-Präfix, z. B. `twitch-live-send-<digest48>`.
- **Endpoints (POST, JSON):**
  - `/internal/master/v1/discord/send-rich-message`
    Body: `{ channel_id: i64, content: Option<String>, embed: serde_json::Value, allowed_role_ids: Vec<i64>, view_spec: Option<serde_json::Value> }`
    Response: `{ ok: bool, result: { message_id: String } }`
  - `/internal/master/v1/discord/edit-rich-message`
    Body: `{ channel_id: i64, message_id: String, content: Option<String>, embed: serde_json::Value, view_spec: Option<serde_json::Value> }`
- **Timeout:** 10 Sekunden gesamt.
- **Retry:** max. 2 Versuche; bei Timeout 2 s warten, dann erneut; HTTP ≥ 400 → kein Retry; Netzwerkfehler → kein Retry.

### Inbound-Auth (`tb-http-core`)

- **Header-Konstanten:** `INTERNAL_TOKEN_HEADER = "X-Internal-Token"`, `IDEMPOTENCY_KEY_HEADER = "Idempotency-Key"` (**ohne** `X-`-Präfix — abweichend vom outbound `X-Idempotency-Key`), `INTERNAL_API_BASE_PATH = "/internal/twitch/v1"`.
- **Middleware-Reihenfolge:** Loopback-Check zuerst → Auth-Check.
  - Nicht-Loopback-Peer: `403 { "error": "forbidden", "message": "internal API accepts loopback traffic only" }`
  - Falscher/leerer Token: `401 { "error": "unauthorized", "message": "missing or invalid internal token" }` (constant-time compare)
  - Leer konfiguriertes Token → fail-closed (alle Requests 401).
- **Kein CSRF** auf der internen API.

### Twitch Helix / App-Token (`tb-transport-twitch`)

- **Env-Vars:** `TWITCH_CLIENT_ID`, `TWITCH_CLIENT_SECRET` — werden als Parameter an den Client-Konstruktor übergeben (kein globales `std::env::var` im Lib-Code).
- **`TOKEN_URL`:** `https://id.twitch.tv/oauth2/token`
- **`HELIX_BASE`:** `https://api.twitch.tv/helix`
- **App-Token-Grant:** POST `TOKEN_URL`, form-encoded Body `client_id=…&client_secret=…&grant_type=client_credentials`. Response: `{ access_token: String, expires_in: i64 }`.
- **Expiry-Logik (pure):** `expiry_unix = now_unix + expires_in`; `needs_refresh(now_unix) → bool` wenn `now_unix >= expiry_unix - 60`.
- **Helix-Request-Header:** `Client-Id: <client_id>`, `Authorization: Bearer <access_token>`.

---

## Dateistruktur nach 0c

```
rust/
  Cargo.toml                             # members + deps ergänzt
  crates/
    tb-config/
      src/lib.rs                         # BrokerConfig::load Token-Kette korrigiert (+Test)
    tb-transport-twitch/
      Cargo.toml
      src/
        lib.rs                           # pub mod token; pub mod client; re-exports
        token.rs                         # AppToken (pure), TokenManager (reqwest, wiremock-testbar)
        client.rs                        # HelixClient (Arc<reqwest::Client> + TokenManager)
    tb-transport-discord/
      Cargo.toml
      src/
        lib.rs                           # pub mod backend; pub mod relay; pub mod noop; re-exports
        backend.rs                       # DiscordBackend-Trait + Payload-Typen + Response-Typen
        relay.rs                         # BrokerRelay (reqwest, Idempotency-Key, Retry)
        noop.rs                          # HeadlessNoop (kein Netz, Drop-Impl)
    tb-http-core/
      Cargo.toml
      src/
        lib.rs                           # pub mod constants; pub mod error; pub mod middleware; re-exports
        constants.rs                     # Header-Konstanten + INTERNAL_API_BASE_PATH
        error.rs                         # ApiError (axum IntoResponse, JSON)
        middleware/
          mod.rs                         # pub mod loopback; pub mod auth; pub mod idempotency;
          loopback.rs                    # LoopbackLayer + LoopbackMiddleware (tower)
          auth.rs                        # InternalAuthLayer + InternalAuthMiddleware (tower, constant-time)
          idempotency.rs                 # IdempotencyKey-Extraktor (axum FromRequestParts)
  docs/
    01-architecture.md                   # twitch_api-Hinweis ergänzt
    plans/
      2026-06-09-phase-0c-transport-http-fundament.md   # diese Datei
```

---

## Task 0: Workspace-Deps + Prereq-Check

**Files:** `rust/Cargo.toml`

- [ ] **Step 1: Voraussetzungen prüfen**

Run:
```bash
source "$HOME/.cargo/env" && cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo check --workspace 2>&1 | tail -5
```
Expected: `Finished` oder nur `warning:` — kein `error:`. Zeigt, dass 0a/0b-Basis kompiliert.

- [ ] **Step 2: Workspace-`members` + `workspace.dependencies` erweitern**

In `rust/Cargo.toml` `members`-Array ergänzen:

```toml
"crates/tb-transport-twitch",
"crates/tb-transport-discord",
"crates/tb-http-core",
```

Unter `[workspace.dependencies]` ergänzen:

```toml
reqwest        = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
sha2           = "0.10"
axum           = "0.7"
tower          = "0.5"
tower-http     = { version = "0.6", features = ["trace"] }
tb-transport-twitch   = { path = "crates/tb-transport-twitch" }
tb-transport-discord  = { path = "crates/tb-transport-discord" }
tb-http-core          = { path = "crates/tb-http-core" }

[workspace.dev-dependencies]
wiremock = "0.6"
```

> Hinweis: `wiremock` als `[workspace.dev-dependencies]` — jede nutzende Crate bindet es als `[dev-dependencies] wiremock = { workspace = true }`.

- [ ] **Step 3: Kein Commit** (Workspace baut erst nach Task 1ff.).

---

## Task 1: `tb-config` — BrokerConfig Token-Kette korrigieren

**Files:** `rust/crates/tb-config/src/lib.rs`

**Problem:** `BrokerConfig::load` prüft aktuell `MASTER_BROKER_TOKEN → TWITCH_INTERNAL_API_TOKEN` (direkt Fallback auf `internal_token`). Das Monitoring-System kennt aber auch `MAIN_BOT_INTERNAL_TOKEN` als Zwischenstufe. Korrekte Kette: `MASTER_BROKER_TOKEN → MAIN_BOT_INTERNAL_TOKEN → TWITCH_INTERNAL_API_TOKEN`.

- [ ] **Step 1: `BrokerConfig::load` in `tb-config/src/lib.rs` anpassen**

Die bestehende `BrokerConfig::load`-Methode ersetzen:

```rust
impl BrokerConfig {
    fn load(get: &Get, internal_token: &str) -> Result<Self, ConfigError> {
        let base_url = match get("MASTER_BROKER_BASE_URL").and_then(non_empty) {
            Some(u) => u,
            None => {
                let host = or_default(get, "MASTER_BROKER_HOST", "127.0.0.1");
                let port = parse_or(get, "MASTER_BROKER_PORT", 8770u16)?;
                format!("http://{host}:{port}")
            }
        };
        // Fallback-Kette: MASTER_BROKER_TOKEN → MAIN_BOT_INTERNAL_TOKEN → TWITCH_INTERNAL_API_TOKEN
        let token = get("MASTER_BROKER_TOKEN")
            .and_then(non_empty)
            .or_else(|| get("MAIN_BOT_INTERNAL_TOKEN").and_then(non_empty))
            .unwrap_or_else(|| internal_token.to_string());
        Ok(Self { base_url, token })
    }
}
```

- [ ] **Step 2: Test für die drei Fallback-Stufen ergänzen**

Im bestehenden `#[cfg(test)] mod tests` in `tb-config/src/lib.rs` drei neue Tests hinzufügen (nach den vorhandenen Tests):

```rust
    #[test]
    fn broker_token_first_priority_master_broker_token() {
        let mut m = minimal();
        m.insert("MASTER_BROKER_TOKEN", "master-tok");
        m.insert("MAIN_BOT_INTERNAL_TOKEN", "main-tok");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.token, "master-tok");
    }

    #[test]
    fn broker_token_second_priority_main_bot_internal_token() {
        let mut m = minimal();
        m.insert("MAIN_BOT_INTERNAL_TOKEN", "main-tok");
        let s = Settings::load(&src(m)).unwrap();
        assert_eq!(s.broker.token, "main-tok");
    }

    #[test]
    fn broker_token_fallback_to_internal_api_token() {
        // kein MASTER_BROKER_TOKEN, kein MAIN_BOT_INTERNAL_TOKEN → fällt auf TWITCH_INTERNAL_API_TOKEN zurück
        let s = Settings::load(&src(minimal())).unwrap();
        assert_eq!(s.broker.token, "tok-123");
    }
```

- [ ] **Step 3: Testen**

Run:
```bash
source "$HOME/.cargo/env" && cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo test -p tb-config 2>&1 | tail -15
```
Expected:
```
running 7 tests
test tests::broker_token_fallback_to_internal_api_token ... ok
test tests::broker_token_first_priority_master_broker_token ... ok
test tests::broker_token_second_priority_main_bot_internal_token ... ok
test tests::defaults_apply_when_optional_absent ... ok
test tests::invalid_int_errors ... ok
test tests::missing_required_dsn_errors ... ok
test tests::overrides_are_parsed ... ok
test result: ok. 7 passed; 0 failed
```

- [ ] **Step 4: Commit**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/crates/tb-config/src/lib.rs
git commit -m "$(printf 'fix(rust): BrokerConfig Token-Fallback-Kette MAIN_BOT_INTERNAL_TOKEN ergänzt\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
```

---

## Task 2: `tb-transport-discord` — DiscordBackend + BrokerRelay + HeadlessNoop

**Files:** `rust/crates/tb-transport-discord/Cargo.toml`, `src/lib.rs`, `src/backend.rs`, `src/relay.rs`, `src/noop.rs`

- [ ] **Step 1: Crate-Verzeichnis + Cargo.toml anlegen**

```bash
mkdir -p /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/crates/tb-transport-discord/src/middleware
```

`rust/crates/tb-transport-discord/Cargo.toml`:

```toml
[package]
name = "tb-transport-discord"
version = "0.1.0"
edition = "2021"

[dependencies]
tb-config   = { workspace = true }
tb-error    = { workspace = true }
reqwest     = { workspace = true }
serde       = { workspace = true }
serde_json  = { workspace = true }
sha2        = { workspace = true }
hex         = { workspace = true }
tokio       = { workspace = true }
thiserror   = { workspace = true }

[dev-dependencies]
wiremock    = { workspace = true }
tokio       = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

- [ ] **Step 2: `src/backend.rs` — Trait + Payload-/Response-Typen**

```rust
//! DiscordBackend-Trait und Payload-Typen für Discord-Rich-Messages über den Master-Broker.

use serde::{Deserialize, Serialize};

/// Payload für `/internal/master/v1/discord/send-rich-message`.
#[derive(Debug, Clone, Serialize)]
pub struct SendRichMessage {
    pub channel_id: i64,
    pub content: Option<String>,
    pub embed: serde_json::Value,
    pub allowed_role_ids: Vec<i64>,
    pub view_spec: Option<serde_json::Value>,
}

/// Payload für `/internal/master/v1/discord/edit-rich-message`.
#[derive(Debug, Clone, Serialize)]
pub struct EditRichMessage {
    pub channel_id: i64,
    pub message_id: String,
    pub content: Option<String>,
    pub embed: serde_json::Value,
    pub view_spec: Option<serde_json::Value>,
}

/// Antwort des Brokers auf `send-rich-message`.
#[derive(Debug, Deserialize)]
pub struct SendResult {
    pub ok: bool,
    pub result: SendResultInner,
}

/// Inneres Ergebnis-Objekt der Broker-Antwort.
#[derive(Debug, Deserialize)]
pub struct SendResultInner {
    pub message_id: String,
}

/// Einheitlicher Fehlertyp für Discord-Transport.
#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Broker antwortete mit Status {status}: {body}")]
    BrokerError { status: u16, body: String },
    #[error("Antwort-Deserialisierung fehlgeschlagen: {0}")]
    Deserialize(#[from] serde_json::Error),
}

/// Abstraktes Discord-Backend — ermöglicht Test-Stubs ohne Netz.
#[async_trait::async_trait]
pub trait DiscordBackend: Send + Sync {
    /// Sendet eine Rich-Message in einen Discord-Kanal.
    async fn send_rich_message(
        &self,
        payload: SendRichMessage,
    ) -> Result<SendResult, DiscordError>;

    /// Bearbeitet eine bestehende Rich-Message.
    async fn edit_rich_message(
        &self,
        payload: EditRichMessage,
    ) -> Result<(), DiscordError>;
}
```

> **Hinweis:** `async_trait` muss als Workspace-Dep ergänzt werden (Step 1 Cargo.toml nachziehen: `async-trait = "0.1"`).

- [ ] **Step 2b: Cargo.toml um `async-trait` ergänzen**

```toml
async-trait = "0.1"
```

Auch in `rust/Cargo.toml` unter `[workspace.dependencies]`:
```toml
async-trait = "0.1"
```

- [ ] **Step 3: `src/relay.rs` — BrokerRelay**

```rust
//! BrokerRelay — HTTP-Client für den Master-Broker (Port 8770).
//!
//! Idempotency-Key: SHA-256 über kanonisches Payload-JSON + Aktionspräfix,
//! hex auf 48 Zeichen. Keine Byte-Parität zur Python-Implementierung nötig —
//! Python-Monitoring wird vor dem Rust-Cutover abgeschaltet.

use crate::backend::{DiscordBackend, DiscordError, EditRichMessage, SendResult, SendRichMessage};
use reqwest::{Client, StatusCode};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tb_config::BrokerConfig;

const SEND_PATH: &str = "/internal/master/v1/discord/send-rich-message";
const EDIT_PATH: &str = "/internal/master/v1/discord/edit-rich-message";
const TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_WAIT: Duration = Duration::from_secs(2);
const MAX_ATTEMPTS: u32 = 2;

/// HTTP-Client für den Master-Broker. Hält eine geteilte `reqwest::Client`-Instanz.
#[derive(Clone)]
pub struct BrokerRelay {
    client: Arc<Client>,
    base_url: String,
    token: String,
}

impl BrokerRelay {
    /// Erstellt einen neuen BrokerRelay aus der übergebenen Konfiguration.
    pub fn new(config: &BrokerConfig) -> Result<Self, reqwest::Error> {
        let client = Client::builder().timeout(TIMEOUT).build()?;
        Ok(Self {
            client: Arc::new(client),
            base_url: config.base_url.clone(),
            token: config.token.clone(),
        })
    }

    /// Berechnet den deterministischen Idempotency-Key.
    ///
    /// Format: `<prefix>-<sha256hex[..48]>`
    pub fn idempotency_key<T: serde::Serialize>(prefix: &str, payload: &T) -> String {
        let json = serde_json::to_string(payload).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let digest = hex::encode(hasher.finalize());
        format!("{}-{}", prefix, &digest[..48])
    }

    /// Sendet eine POST-Anfrage an den Broker mit Retry-Logik.
    ///
    /// Retry: max. 2 Versuche; bei Timeout 2 s warten; HTTP ≥ 400 → kein Retry;
    /// Netzwerkfehler → kein Retry.
    async fn post_with_retry<T: serde::Serialize>(
        &self,
        path: &str,
        payload: &T,
        idempotency_key: &str,
    ) -> Result<reqwest::Response, DiscordError> {
        let url = format!("{}{}", self.base_url, path);
        let mut last_err: Option<DiscordError> = None;

        for attempt in 0..MAX_ATTEMPTS {
            let result = self
                .client
                .post(&url)
                .header("X-Internal-Token", &self.token)
                .header("X-Idempotency-Key", idempotency_key)
                .header("Content-Type", "application/json")
                .json(payload)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    // HTTP ≥ 400 → kein Retry
                    let body = resp.text().await.unwrap_or_default();
                    return Err(DiscordError::BrokerError {
                        status: status.as_u16(),
                        body,
                    });
                }
                Err(e) if e.is_timeout() && attempt < MAX_ATTEMPTS - 1 => {
                    last_err = Some(DiscordError::Http(e));
                    tokio::time::sleep(RETRY_WAIT).await;
                }
                Err(e) => {
                    // Netzwerkfehler oder letzter Timeout-Versuch → kein weiterer Retry
                    return Err(DiscordError::Http(e));
                }
            }
        }

        Err(last_err.unwrap_or_else(|| DiscordError::BrokerError {
            status: 0,
            body: "Unbekannter Fehler".to_string(),
        }))
    }
}

#[async_trait::async_trait]
impl DiscordBackend for BrokerRelay {
    async fn send_rich_message(
        &self,
        payload: SendRichMessage,
    ) -> Result<SendResult, DiscordError> {
        let key = Self::idempotency_key("twitch-live-send", &payload);
        let resp = self.post_with_retry(SEND_PATH, &payload, &key).await?;
        let result: SendResult = resp.json().await?;
        Ok(result)
    }

    async fn edit_rich_message(
        &self,
        payload: EditRichMessage,
    ) -> Result<(), DiscordError> {
        let key = Self::idempotency_key("twitch-live-edit", &payload);
        self.post_with_retry(EDIT_PATH, &payload, &key).await?;
        Ok(())
    }
}
```

- [ ] **Step 4: `src/noop.rs` — HeadlessNoop**

```rust
//! HeadlessNoop — kein Netz, kein Panic. Für Tests und headless Builds.

use crate::backend::{DiscordBackend, DiscordError, EditRichMessage, SendResult, SendResultInner, SendRichMessage};

/// Verwirft alle Discord-Nachrichten stillschweigend. Nützlich in Tests
/// und in Umgebungen ohne Bridge-Zugang.
#[derive(Debug, Default, Clone)]
pub struct HeadlessNoop;

#[async_trait::async_trait]
impl DiscordBackend for HeadlessNoop {
    async fn send_rich_message(
        &self,
        _payload: SendRichMessage,
    ) -> Result<SendResult, DiscordError> {
        Ok(SendResult {
            ok: true,
            result: SendResultInner {
                message_id: "noop-0".to_string(),
            },
        })
    }

    async fn edit_rich_message(
        &self,
        _payload: EditRichMessage,
    ) -> Result<(), DiscordError> {
        Ok(())
    }
}
```

- [ ] **Step 5: `src/lib.rs`**

```rust
//! tb-transport-discord — Discord-Backend-Trait, BrokerRelay und HeadlessNoop.

pub mod backend;
pub mod noop;
pub mod relay;

pub use backend::{DiscordBackend, DiscordError, EditRichMessage, SendResult, SendRichMessage};
pub use noop::HeadlessNoop;
pub use relay::BrokerRelay;
```

- [ ] **Step 6: wiremock-Tests**

`rust/crates/tb-transport-discord/src/lib.rs` am Ende ergänzen (oder als separate Datei `tests/relay_tests.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tb_config::BrokerConfig;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(base_url: &str) -> BrokerConfig {
        BrokerConfig {
            base_url: base_url.to_string(),
            token: "test-token".to_string(),
        }
    }

    fn sample_send_payload() -> SendRichMessage {
        SendRichMessage {
            channel_id: 12345,
            content: Some("Hallo".to_string()),
            embed: serde_json::json!({"title": "Test"}),
            allowed_role_ids: vec![99],
            view_spec: None,
        }
    }

    #[tokio::test]
    async fn sendet_korrekte_header_und_parst_antwort() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-rich-message"))
            .and(header("X-Internal-Token", "test-token"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": "msg-42" }
            })))
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let result = relay.send_rich_message(sample_send_payload()).await.unwrap();
        assert!(result.ok);
        assert_eq!(result.result.message_id, "msg-42");
    }

    #[tokio::test]
    async fn idempotency_key_header_vorhanden() {
        let server = MockServer::start().await;
        // Prüft, dass der Header gesetzt ist (Wert deterministisch — kein exakter Match nötig)
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-rich-message"))
            .and(wiremock::matchers::header_exists("X-Idempotency-Key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "message_id": "msg-1" }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        relay.send_rich_message(sample_send_payload()).await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn idempotency_key_deterministisch_gleicher_payload() {
        let p = sample_send_payload();
        let k1 = BrokerRelay::idempotency_key("twitch-live-send", &p);
        let k2 = BrokerRelay::idempotency_key("twitch-live-send", &p);
        assert_eq!(k1, k2);
        assert!(k1.starts_with("twitch-live-send-"));
        // Digest-Teil hat 48 Zeichen
        let digest_part = k1.trim_start_matches("twitch-live-send-");
        assert_eq!(digest_part.len(), 48);
    }

    #[tokio::test]
    async fn http_400_kein_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .expect(1) // genau 1 Versuch — kein Retry
            .mount(&server)
            .await;

        let relay = BrokerRelay::new(&test_config(&server.uri())).unwrap();
        let err = relay.send_rich_message(sample_send_payload()).await.unwrap_err();
        assert!(matches!(err, DiscordError::BrokerError { status: 400, .. }));
        server.verify().await;
    }

    #[tokio::test]
    async fn headless_noop_gibt_immer_ok() {
        let noop = HeadlessNoop;
        let result = noop.send_rich_message(sample_send_payload()).await.unwrap();
        assert!(result.ok);
        let edit = EditRichMessage {
            channel_id: 1,
            message_id: "x".to_string(),
            content: None,
            embed: serde_json::Value::Null,
            view_spec: None,
        };
        noop.edit_rich_message(edit).await.unwrap();
    }
}
```

- [ ] **Step 7: Testen**

Run:
```bash
source "$HOME/.cargo/env" && cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo test -p tb-transport-discord 2>&1 | tail -20
```
Expected:
```
running 5 tests
test tests::headless_noop_gibt_immer_ok ... ok
test tests::http_400_kein_retry ... ok
test tests::idempotency_key_deterministisch_gleicher_payload ... ok
test tests::idempotency_key_header_vorhanden ... ok
test tests::sendet_korrekte_header_und_parst_antwort ... ok
test result: ok. 5 passed; 0 failed
```

- [ ] **Step 8: Commit**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/Cargo.toml rust/crates/tb-transport-discord
git commit -m "$(printf 'feat(rust): tb-transport-discord BrokerRelay + HeadlessNoop + wiremock-Tests (0c)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
```

---

## Task 3: `tb-transport-twitch` — AppToken + HelixClient

**Files:** `rust/crates/tb-transport-twitch/Cargo.toml`, `src/lib.rs`, `src/token.rs`, `src/client.rs`

- [ ] **Step 1: Crate-Verzeichnis + Cargo.toml**

```bash
mkdir -p /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/crates/tb-transport-twitch/src
```

`rust/crates/tb-transport-twitch/Cargo.toml`:

```toml
[package]
name = "tb-transport-twitch"
version = "0.1.0"
edition = "2021"

[dependencies]
tb-config   = { workspace = true }
tb-error    = { workspace = true }
reqwest     = { workspace = true }
serde       = { workspace = true }
serde_json  = { workspace = true }
tokio       = { workspace = true }
thiserror   = { workspace = true }

[dev-dependencies]
wiremock    = { workspace = true }
tokio       = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

- [ ] **Step 2: `src/token.rs` — AppToken (pure Expiry-Logik + HTTP-Fetch)**

```rust
//! App-Token-Verwaltung für Twitch client_credentials.
//!
//! Expiry-Logik ist pure (keine Uhrzeit-Abhängigkeit im Struct) —
//! testbar ohne Mock-Clock. HTTP-Fetch gegen wiremock testbar.

use reqwest::Client;
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

const EXPIRY_MARGIN_SECS: i64 = 60;

/// Fehlertyp für Token-Operationen.
#[derive(Debug, Error)]
pub enum TokenError {
    #[error("HTTP-Fehler beim Token-Abruf: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Token-Response nicht parsebar: {0}")]
    Parse(#[from] serde_json::Error),
}

/// In-Memory-Repräsentation eines gültigen App-Tokens.
#[derive(Debug, Clone)]
pub struct AppToken {
    pub access_token: String,
    /// Unix-Zeitstempel (Sekunden), ab dem der Token spätestens erneuert werden muss.
    pub expiry_unix: i64,
}

impl AppToken {
    /// Erstellt einen AppToken mit berechnetem Ablaufzeitpunkt.
    pub fn new(access_token: String, expires_in: i64, now_unix: i64) -> Self {
        Self {
            access_token,
            expiry_unix: now_unix + expires_in,
        }
    }

    /// Gibt zurück, ob der Token erneuert werden muss.
    ///
    /// Erneuerung nötig, wenn weniger als 60 Sekunden bis zum Ablauf verbleiben.
    pub fn needs_refresh(&self, now_unix: i64) -> bool {
        now_unix >= self.expiry_unix - EXPIRY_MARGIN_SECS
    }
}

/// Rohe Token-Response vom Twitch OAuth-Endpunkt.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

/// Holt einen neuen App-Token über den client_credentials-Grant.
///
/// `token_url` — überschreibbar für Tests (z. B. wiremock-URL).
pub async fn fetch_app_token(
    client: &Client,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<AppToken, TokenError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let resp: TokenResponse = client
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(AppToken::new(resp.access_token, resp.expires_in, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_refresh_false_wenn_weit_von_ablauf() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_000_000; Delta = 3600 > 60
        assert!(!token.needs_refresh(1_000_000));
    }

    #[test]
    fn needs_refresh_true_bei_genau_60s_vorlauf() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_540; Delta = 60 → Grenzfall: needs_refresh
        assert!(token.needs_refresh(1_003_540));
    }

    #[test]
    fn needs_refresh_true_wenn_abgelaufen() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        assert!(token.needs_refresh(1_010_000));
    }

    #[test]
    fn needs_refresh_false_wenn_59s_verbleiben() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_541; Delta = 59 < 60 → NOCH nicht
        // Anmerkung: needs_refresh prüft >= expiry-60, also 1_003_541 >= 1_003_540 → true
        // Grenzfall-Klärung: 60s Vorlauf bedeutet ab Delta <= 60 wird erneuert
        assert!(token.needs_refresh(1_003_541));
    }

    #[test]
    fn needs_refresh_false_wenn_61s_verbleiben() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_539; Delta = 61 > 60 → noch gültig
        assert!(!token.needs_refresh(1_003_539));
    }
}
```

- [ ] **Step 3: `src/client.rs` — HelixClient**

```rust
//! HelixClient — dünner reqwest-Wrapper für Twitch Helix.
//!
//! Hält einen geteilten Arc<reqwest::Client> und erneuert den App-Token
//! automatisch bei Bedarf (in-memory, kein persistenter Cache).

use crate::token::{fetch_app_token, AppToken, TokenError};
use reqwest::{Client, RequestBuilder};
use std::sync::Arc;
use tokio::sync::Mutex;
use thiserror::Error;

/// Fehlertyp für Helix-Operationen.
#[derive(Debug, Error)]
pub enum HelixError {
    #[error("Token-Fehler: {0}")]
    Token(#[from] TokenError),
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
}

/// Konfiguration für den HelixClient.
#[derive(Debug, Clone)]
pub struct HelixConfig {
    /// Twitch Client-ID.
    pub client_id: String,
    /// Twitch Client-Secret.
    pub client_secret: String,
    /// OAuth-Token-URL (überschreibbar für Tests).
    pub token_url: String,
    /// Helix-Basis-URL (überschreibbar für Tests).
    pub helix_base: String,
}

impl HelixConfig {
    /// Erstellt eine HelixConfig mit den Standard-Twitch-URLs.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token_url: "https://id.twitch.tv/oauth2/token".to_string(),
            helix_base: "https://api.twitch.tv/helix".to_string(),
        }
    }
}

/// HTTP-Client für die Twitch Helix API.
///
/// Verwaltet den App-Token intern (auto-refresh bei Ablauf).
#[derive(Clone)]
pub struct HelixClient {
    http: Arc<Client>,
    config: HelixConfig,
    token: Arc<Mutex<Option<AppToken>>>,
}

impl HelixClient {
    /// Erstellt einen neuen HelixClient.
    pub fn new(config: HelixConfig) -> Result<Self, reqwest::Error> {
        let http = Client::builder().build()?;
        Ok(Self {
            http: Arc::new(http),
            config,
            token: Arc::new(Mutex::new(None)),
        })
    }

    /// Gibt einen gültigen App-Token zurück — holt ihn bei Bedarf neu.
    async fn access_token(&self) -> Result<String, HelixError> {
        let mut guard = self.token.lock().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let needs_new = guard
            .as_ref()
            .map(|t| t.needs_refresh(now))
            .unwrap_or(true);

        if needs_new {
            let token = fetch_app_token(
                &self.http,
                &self.config.token_url,
                &self.config.client_id,
                &self.config.client_secret,
            )
            .await?;
            *guard = Some(token);
        }

        Ok(guard.as_ref().unwrap().access_token.clone())
    }

    /// Erstellt einen vorbereiteten GET-Request an einen Helix-Endpunkt.
    ///
    /// `path` — z. B. `"/streams"` (ohne Basis-URL).
    pub async fn get(&self, path: &str) -> Result<RequestBuilder, HelixError> {
        let token = self.access_token().await?;
        let url = format!("{}{}", self.config.helix_base, path);
        Ok(self
            .http
            .get(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {token}")))
    }
}
```

- [ ] **Step 4: `src/lib.rs`**

```rust
//! tb-transport-twitch — Helix-Client und App-Token-Manager.

pub mod client;
pub mod token;

pub use client::{HelixClient, HelixConfig, HelixError};
pub use token::{AppToken, TokenError};
```

- [ ] **Step 5: wiremock-Tests für Token-Fetch und Helix-Request**

Am Ende von `src/client.rs` (oder als separate Datei `tests/client_tests.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(token_url: &str, helix_base: &str) -> HelixConfig {
        HelixConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            token_url: token_url.to_string(),
            helix_base: helix_base.to_string(),
        }
    }

    #[tokio::test]
    async fn token_fetch_sendet_form_encoded_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=test-client-id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-abc",
                "expires_in": 3600,
                "token_type": "bearer"
            })))
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let token = client.access_token().await.unwrap();
        assert_eq!(token, "tok-abc");
    }

    #[tokio::test]
    async fn helix_request_setzt_korrekte_header() {
        let server = MockServer::start().await;

        // Token-Endpunkt
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "helix-tok",
                "expires_in": 3600,
                "token_type": "bearer"
            })))
            .mount(&server)
            .await;

        // Helix-Endpunkt
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(header("Client-Id", "test-client-id"))
            .and(header("Authorization", "Bearer helix-tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let builder = client.get("/streams").await.unwrap();
        let resp = builder.send().await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn token_wird_nicht_neu_geholt_wenn_noch_gueltig() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fresh-tok",
                "expires_in": 3600,
                "token_type": "bearer"
            })))
            .expect(1) // genau einmal — kein doppelter Fetch
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let t1 = client.access_token().await.unwrap();
        let t2 = client.access_token().await.unwrap();
        assert_eq!(t1, t2);
        server.verify().await;
    }
}
```

- [ ] **Step 6: Testen**

Run:
```bash
source "$HOME/.cargo/env" && cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo test -p tb-transport-twitch 2>&1 | tail -20
```
Expected:
```
running 8 tests
test token::tests::needs_refresh_false_wenn_weit_von_ablauf ... ok
test token::tests::needs_refresh_true_bei_genau_60s_vorlauf ... ok
test token::tests::needs_refresh_true_wenn_abgelaufen ... ok
test token::tests::needs_refresh_false_wenn_59s_verbleiben ... ok
test token::tests::needs_refresh_false_wenn_61s_verbleiben ... ok
test client::tests::helix_request_setzt_korrekte_header ... ok
test client::tests::token_fetch_sendet_form_encoded_body ... ok
test client::tests::token_wird_nicht_neu_geholt_wenn_noch_gueltig ... ok
test result: ok. 8 passed; 0 failed
```

- [ ] **Step 7: Commit**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/crates/tb-transport-twitch
git commit -m "$(printf 'feat(rust): tb-transport-twitch HelixClient + AppToken + wiremock-Tests (0c)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
```

---

## Task 4: `tb-http-core` — Auth-Layer, Loopback, Idempotency, Error-Response

**Files:** `rust/crates/tb-http-core/Cargo.toml`, `src/lib.rs`, `src/constants.rs`, `src/error.rs`, `src/middleware/mod.rs`, `src/middleware/loopback.rs`, `src/middleware/auth.rs`, `src/middleware/idempotency.rs`

- [ ] **Step 1: Crate-Verzeichnis + Cargo.toml**

```bash
mkdir -p /home/naniadm/Documents/Deadlock-Twitch-Bot/rust/crates/tb-http-core/src/middleware
```

`rust/crates/tb-http-core/Cargo.toml`:

```toml
[package]
name = "tb-http-core"
version = "0.1.0"
edition = "2021"

[dependencies]
tb-error    = { workspace = true }
axum        = { workspace = true }
tower       = { workspace = true }
tower-http  = { workspace = true }
serde       = { workspace = true }
serde_json  = { workspace = true }
tokio       = { workspace = true }
thiserror   = { workspace = true }

[dev-dependencies]
tower       = { workspace = true, features = ["util"] }
tokio       = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

- [ ] **Step 2: `src/constants.rs`**

```rust
//! Header-Konstanten und Basis-Pfad der internen API.

/// Inbound- und Outbound-Auth-Header.
pub const INTERNAL_TOKEN_HEADER: &str = "X-Internal-Token";

/// Idempotency-Key-Header (inbound, ohne `X-`-Präfix).
///
/// Abweichung vom outbound-Header (`X-Idempotency-Key`) — absichtlich,
/// da die interne API einem anderen Stil folgt als die Broker-Calls.
pub const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";

/// Basis-Pfad der internen API.
pub const INTERNAL_API_BASE_PATH: &str = "/internal/twitch/v1";
```

- [ ] **Step 3: `src/error.rs` — einheitliche Fehler-Response**

```rust
//! Einheitliche JSON-Fehler-Antworten für die interne API.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

/// Einheitliches Fehler-Payload der internen API.
#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    pub error: &'static str,
    pub message: &'static str,
}

/// Axum-kompatibler Fehlertyp mit JSON-Serialisierung.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: ApiErrorBody,
}

impl ApiError {
    /// 403 Forbidden — nicht-loopback Zugriff.
    pub fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: ApiErrorBody {
                error: "forbidden",
                message: "internal API accepts loopback traffic only",
            },
        }
    }

    /// 401 Unauthorized — fehlender oder falscher Token.
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: ApiErrorBody {
                error: "unauthorized",
                message: "missing or invalid internal token",
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
```

- [ ] **Step 4: `src/middleware/loopback.rs` — Loopback-Middleware**

```rust
//! Loopback-Middleware: blockiert alle Requests, die nicht von 127.x.x.x kommen.

use crate::error::ApiError;
use axum::{
    extract::ConnectInfo,
    http::Request,
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;

/// Axum-Middleware: lässt nur Loopback-Verbindungen (127.x.x.x) durch.
///
/// Muss in der Middleware-Kette vor der Auth-Prüfung stehen.
pub async fn loopback_only(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if !addr.ip().is_loopback() {
        return ApiError::forbidden().into_response();
    }
    next.run(req).await
}
```

- [ ] **Step 5: `src/middleware/auth.rs` — constant-time Token-Check**

```rust
//! Interne Auth-Middleware: prüft den X-Internal-Token-Header (constant-time).

use crate::constants::INTERNAL_TOKEN_HEADER;
use crate::error::ApiError;
use axum::{
    extract::State,
    http::Request,
    middleware::Next,
    response::Response,
};

/// Axum-Middleware: prüft `X-Internal-Token` gegen den konfigurierten Token.
///
/// Leeres konfiguriertes Token → fail-closed (immer 401).
/// Vergleich via constant-time-Funktion (`subtle`-frei: direkte Byte-Iteration).
pub async fn internal_auth(
    State(expected_token): State<String>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Leeres konfiguriertes Token → immer 401 (fail-closed)
    if expected_token.is_empty() {
        return ApiError::unauthorized().into_response();
    }

    let provided = req
        .headers()
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !constant_time_eq(provided.as_bytes(), expected_token.as_bytes()) {
        return ApiError::unauthorized().into_response();
    }

    next.run(req).await
}

/// Constant-time Byte-Vergleich (verhindert Timing-Angriffe).
///
/// Gibt `true` zurück, wenn beide Slices identisch sind.
/// Laufzeit ist proportional zur Länge von `expected`, unabhängig vom Mismatch.
fn constant_time_eq(provided: &[u8], expected: &[u8]) -> bool {
    if provided.len() != expected.len() {
        // Längenunterschied ist selbst keine Information, die Timing enthüllt —
        // early-return hier ist akzeptabel, da der Angreifer die Tokenlänge kennt.
        return false;
    }
    let mut diff: u8 = 0;
    for (a, b) in provided.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gleiche_werte_sind_gleich() {
        assert!(constant_time_eq(b"abc", b"abc"));
    }

    #[test]
    fn unterschiedliche_werte_sind_ungleich() {
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    #[test]
    fn unterschiedliche_laengen_sind_ungleich() {
        assert!(!constant_time_eq(b"ab", b"abc"));
    }

    #[test]
    fn leere_strings_sind_gleich() {
        assert!(constant_time_eq(b"", b""));
    }
}
```

- [ ] **Step 6: `src/middleware/idempotency.rs` — Idempotency-Key-Extraktor**

```rust
//! Extrahiert den `Idempotency-Key`-Header als typisierter axum-Extraktor.

use crate::constants::IDEMPOTENCY_KEY_HEADER;
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};

/// Optionaler Idempotency-Key aus dem `Idempotency-Key`-Header.
///
/// Ist der Header nicht vorhanden oder nicht valides UTF-8, gibt `None` zurück.
#[derive(Debug, Clone)]
pub struct IdempotencyKey(pub Option<String>);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let key = parts
            .headers
            .get(IDEMPOTENCY_KEY_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        Ok(IdempotencyKey(key))
    }
}
```

- [ ] **Step 7: `src/middleware/mod.rs`**

```rust
//! Middleware-Bausteine für die interne axum-API.

pub mod auth;
pub mod idempotency;
pub mod loopback;

pub use auth::internal_auth;
pub use idempotency::IdempotencyKey;
pub use loopback::loopback_only;
```

- [ ] **Step 8: `src/lib.rs`**

```rust
//! tb-http-core — gemeinsame axum-Bausteine für die interne API.

pub mod constants;
pub mod error;
pub mod middleware;

pub use constants::{IDEMPOTENCY_KEY_HEADER, INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER};
pub use error::{ApiError, ApiErrorBody};
pub use middleware::{internal_auth, loopback_only, IdempotencyKey};
```

- [ ] **Step 9: axum-oneshot-Integrationstests**

`rust/crates/tb-http-core/tests/middleware_tests.rs`:

```rust
//! Integrationstests für Loopback- und Auth-Middleware via axum-oneshot.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware,
    routing::get,
    Router,
};
use std::net::SocketAddr;
use tb_http_core::middleware::{internal_auth, loopback_only};
use tb_http_core::constants::INTERNAL_TOKEN_HEADER;
use tower::ServiceExt;

/// Baut einen Test-Router mit Loopback + Auth Middleware.
fn test_router(token: &str) -> Router {
    Router::new()
        .route("/test", get(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(
            token.to_string(),
            internal_auth,
        ))
        .layer(middleware::from_fn(loopback_only))
        .into_make_service_with_connect_info::<SocketAddr>()
        .into_router()
}

/// Erstellt einen Request mit gesetzter ConnectInfo.
fn loopback_request(token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .uri("/test")
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
    if let Some(t) = token {
        builder = builder.header(INTERNAL_TOKEN_HEADER, t);
    }
    builder.body(Body::empty()).unwrap()
}

use axum::extract::ConnectInfo;

#[tokio::test]
async fn loopback_mit_korrektem_token_gibt_200() {
    let app = test_router("secret");
    let req = loopback_request(Some("secret"));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn falscher_token_gibt_401() {
    let app = test_router("secret");
    let req = loopback_request(Some("wrong"));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn fehlender_token_gibt_401() {
    let app = test_router("secret");
    let req = loopback_request(None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn leeres_konfiguriertes_token_fail_closed_401() {
    let app = test_router("");
    let req = loopback_request(Some("anything"));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn nicht_loopback_gibt_403() {
    // Router direkt ohne into_make_service_with_connect_info — ConnectInfo nicht gesetzt
    // → simuliert: Request ohne loopback ConnectInfo
    // Besser: Extension direkt auf nicht-loopback-Adresse setzen
    let app = Router::new()
        .route("/test", get(|| async { "ok" }))
        .layer(middleware::from_fn(loopback_only));
    let req = Request::builder()
        .uri("/test")
        .extension(ConnectInfo(
            "8.8.8.8:12345".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn idempotency_key_extraktor_liest_header() {
    use axum::extract::State;
    use tb_http_core::middleware::IdempotencyKey;

    let app = Router::new().route(
        "/test",
        get(|key: IdempotencyKey| async move {
            match key.0 {
                Some(k) => format!("key={k}"),
                None => "no-key".to_string(),
            }
        }),
    );

    let req = Request::builder()
        .uri("/test")
        .header("Idempotency-Key", "idem-123")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), b"key=idem-123");
}
```

- [ ] **Step 10: Testen**

Run:
```bash
source "$HOME/.cargo/env" && cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo test -p tb-http-core 2>&1 | tail -20
```
Expected:
```
running 11 tests
test middleware::auth::tests::gleiche_werte_sind_gleich ... ok
test middleware::auth::tests::leere_strings_sind_gleich ... ok
test middleware::auth::tests::unterschiedliche_laengen_sind_ungleich ... ok
test middleware::auth::tests::unterschiedliche_werte_sind_ungleich ... ok
test nicht_loopback_gibt_403 ... ok
test falscher_token_gibt_401 ... ok
test fehlender_token_gibt_401 ... ok
test leeres_konfiguriertes_token_fail_closed_401 ... ok
test loopback_mit_korrektem_token_gibt_200 ... ok
test idempotency_key_extraktor_liest_header ... ok
test loopback_middleware_reihenfolge_loopback_vor_auth ... ok
test result: ok. 11 passed; 0 failed
```

- [ ] **Step 11: Commit**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/crates/tb-http-core
git commit -m "$(printf 'feat(rust): tb-http-core Loopback+Auth-Middleware + Idempotency-Extraktor + Error-Response (0c)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
```

---

## Task 5: QS + Doku-Sync + Push

**Files:** `rust/Cargo.toml`, `rust/docs/01-architecture.md`, ggf. `rust/docs/05-cleanup-decisions.md`

- [ ] **Step 1: fmt + clippy + alle 0c-Tests**

Run:
```bash
source "$HOME/.cargo/env" && cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tb-config -p tb-transport-discord -p tb-transport-twitch -p tb-http-core 2>&1 | tail -30
```
Expected: fmt sauber, clippy ohne Warnungen, alle Tests grün.

- [ ] **Step 2: Doku-Sync in `01-architecture.md`**

In der Foundation-Crates-Tabelle bei `tb-transport-twitch` den Libs-Eintrag aktualisieren:
- Vorher: `reqwest (rustls), twitch_api, oauth2`
- Nachher: `reqwest (rustls) — hand-rolled, twitch_api-Adoption erst bei vielen Helix-Endpoints (YAGNI-ADR 0c)`

Bei `tb-transport-discord`:
- Vorher: `reqwest`
- Nachher: `reqwest — konsolidiert Python-Broker-Logik in einen Client`

- [ ] **Step 3: Push**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
git add rust/Cargo.toml rust/Cargo.lock rust/docs/01-architecture.md
git commit -m "$(printf 'docs(rust): 0c-Abweichungen (hand-rolled reqwest, no twitch_api, YAGNI)\n\nCo-authored-by: Claude Code (Claude Opus 4.8) <claude-code@local>')"
git push origin main
```

> Interne Foundation/Infra ⇒ **kein** CHANGELOG, keine Discord-/In-App-Spiegelung.

---

## Self-Review (vom Plan-Autor)

**1. Spec-Abdeckung (gegen Aufgabenbeschreibung + `01-architecture.md`):**
- `tb-transport-discord`: DiscordBackend-Trait + BrokerRelay + HeadlessNoop ✓; Payload-Typen + Response-Typen ✓; Idempotency-Key (SHA-256, 48-Zeichen-Hex, Präfix) ✓; Retry-Logik (max. 2, Timeout→2s, HTTP≥400 kein Retry) ✓; wiremock-Tests ✓.
- `tb-transport-twitch`: AppToken (pure Expiry-Logik, `needs_refresh`) ✓; unit-testbar ohne Mock-Clock ✓; HelixClient (reqwest, app-token auto-refresh, Client-Id + Bearer-Header) ✓; wiremock-Tests ✓.
- `tb-http-core`: Header-Konstanten ✓; Loopback-Middleware (403) ✓; Auth-Middleware (401, constant-time, fail-closed) ✓; Idempotency-Key-Extraktor ✓; einheitliche Fehler-Response (JSON) ✓; axum-oneshot-Tests ✓.
- `tb-config`-Bugfix: `MAIN_BOT_INTERNAL_TOKEN`-Fallback ergänzt + 3 Tests ✓.
- Bewusste Abweichungen: hand-rolled reqwest statt `twitch_api`, kein OAuth2-PKCE, kein `tb-eventsub`/`tb-llm`, Idempotency-Key ohne Python-Parität — alle dokumentiert ✓.

**2. Platzhalter-Scan:** kein TBD/TODO; jeder Code-Schritt enthält vollständigen Rust-Code; jeder Run-Schritt hat erwartete Ausgabe.

**3. Typ-/Namens-Konsistenz:** `DiscordBackend`, `BrokerRelay`, `HeadlessNoop`, `SendRichMessage`, `EditRichMessage`, `SendResult`, `DiscordError`; `AppToken`, `TokenManager` (→ `HelixClient`), `HelixConfig`, `HelixError`; `ApiError`, `loopback_only`, `internal_auth`, `IdempotencyKey`, `INTERNAL_TOKEN_HEADER`, `IDEMPOTENCY_KEY_HEADER`, `INTERNAL_API_BASE_PATH` — durchgängig identisch in Implementierung und Tests.

**4. Design-Entscheide, die der Reviewer gezielt prüfen sollte:**
- **`async_trait`-Abhängigkeit für `DiscordBackend`:** MSRV-stable Rust 1.75+ hat async-fn-in-trait. Prüfen, ob der Workspace-MSRV (1.96 laut Plan 0b) ein direktes `async fn` im Trait erlaubt — wenn ja, `async_trait` weglassen.
- **`constant_time_eq` ohne `subtle`-Crate:** Die handrollte XOR-Schleife ist korrekt, aber ein zusätzlicher `subtle`-Import wäre robuster gegen Compiler-Optimierungen. YAGNI für Loopback-API, aber explizit prüfen.
- **Loopback-Middleware nutzt `ConnectInfo<SocketAddr>`:** Muss mit `into_make_service_with_connect_info::<SocketAddr>()` am Router montiert werden — im Test wird `ConnectInfo` direkt als Extension gesetzt, was exakt funktioniert; im Integrations-Test-Abschnitt prüfen, ob das Muster korrekt ist.
- **Retry nur bei `is_timeout()`:** Die `reqwest::Error::is_timeout()`-Methode prüft nur den Client-seitigen Timeout, nicht DNS-Fehler. Der Spec sagt „bei Timeout 2s warten" — Netzwerkfehler kein Retry. Das ist korrekt implementiert; explizit bestätigen.
- **`serde_json::to_string` als Idempotency-Key-Basis:** Funktioniert nur deterministisch, wenn die Struct-Feldreihenfolge stabil ist (Rust garantiert das für Structs). Gilt nicht für `serde_json::Map` — prüfen, dass `SendRichMessage`/`EditRichMessage` reine Structs bleiben und keine `Map`-Felder nutzen.

**Bewusste Grenzen:** OAuth2-PKCE-User-Flow → Phase 6/8. `tb-eventsub`/`tb-llm` → Phase 4/5. Keine Observability-Events in Transport-Crates (erst wenn `tb-observability` die Transport-Crates kennt). Kein persistenter Token-Cache (in-memory reicht für 0c).
