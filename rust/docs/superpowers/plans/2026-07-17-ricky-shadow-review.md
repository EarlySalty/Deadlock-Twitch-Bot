# Ricky Shadow Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Nach einer Chatnachricht der exakten Twitch-ID `147713656` belegte Ricky-Kontexte, Streamer-Transkripte und Fireworks-Entwürfe sechs Monate lang intern prüfen, vollständig nach Discord `1374364800817303632` spiegeln und fristgerecht aus DB und Discord löschen, ohne in dieser Phase auf Twitch zu senden.

**Architecture:** Ein unabhängiger Review-Trigger hängt an EventSub und Scout-IRC, schreibt zuerst in eine eigene PostgreSQL-Eventtabelle und wird von drei kleinen Hintergrundläufen verarbeitet: Modell/Audio, Discord-Forwarding und Retention. OpenAI erhält ausschließlich WAV-Bytes aus dem Arbeitsspeicher; Fireworks liefert streng validiertes JSON. Discord-Senden und -Löschen läuft über den vorhandenen Master-Broker, sodass der Twitch-Bot keinen Discord-Token besitzt.

**Tech Stack:** Rust, Tokio, SQLx/PostgreSQL, reqwest multipart, `yt-dlp`, `ffmpeg`, OpenAI Audio Transcriptions (`whisper-1`), Fireworks Chat Completions (`accounts/fireworks/models/deepseek-v4-flash`), Discord Components V2, systemd User-Services.

## Global Constraints

- Shadow-only: Neuer Code importiert oder ruft keinen Twitch-Sender, `ChatApi` oder `tb-transport-twitch` auf.
- Identität ausschließlich über Twitch-ID `147713656`; Login/Anzeigename sind kein Trigger.
- Keine Diagnose, kein Motiv, kein „Nazi“/„Narzisst“ und keine erfundene Augenzeugenschaft.
- Fakten-Whitelist: `community_ban_2026_05_29`, `racist_greeting_report`, `cs2_cheat_stream`, `post_ban_discord_recruitment`, `twitch_pitch_history`.
- Roh-Audio und Provider-Rohantworten werden nie persistiert oder geloggt.
- DB ist Source of Truth; Discord folgt erst nach erfolgreichem DB-Commit.
- Sechs Kalendermonate Retention gelten ebenfalls für jede Discord-Kopie.
- Provider-Ausfall führt zu `provider_error` und Stille, niemals zu einem Fallback.
- Alle Fehlertexte in DB/Journal sind klassifiziert und dürfen weder Tokens, Header, signierte HLS-URLs noch Provider-Bodies enthalten.
- Jede Teilaufgabe beginnt RED, endet grün, erhält einen kleinen Commit mit Trailer und wird sofort gepusht.

---

## Task 1: Idempotente Discord-Einzellöschung im Master-Broker

**Repo:** `/home/naniadm/Documents/Deadlock-Bots`

**Files:**

- Modify: `rust/crates/dl-broker/src/port.rs`
- Modify: `rust/crates/dl-broker/src/handlers.rs`
- Modify: `rust/crates/dl-broker/src/lib.rs`
- Modify: `rust/crates/dl-discord/src/adapter.rs`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Arbeitsbranch anlegen und Zustand sichern**

```bash
git status --short --branch
git log -1 --oneline
git worktree list
git checkout -b feature/ricky-review-delete-message
```

- [ ] **Step 2: Failing Broker-Tests schreiben**

In `handlers.rs` den vorhandenen `UnusedDiscordPort` um aufgezeichnete Delete-Aufrufe und ein konfigurierbares Ergebnis erweitern. Zwei Handler-Tests hinzufügen:

```rust
#[tokio::test]
async fn delete_message_ruft_port_mit_channel_message_und_reason()

#[tokio::test]
async fn delete_message_ist_bei_fehlender_nachricht_idempotent()
```

Der erste Test erwartet `(channel_id, message_id, reason)` im Fake-Port und HTTP 200. Der zweite lässt den Port `MessageNotFound` liefern und erwartet ebenfalls HTTP 200 mit `already_absent=true`.

Run:

```bash
cd rust
cargo test -p dl-broker delete_message -- --nocapture
```

Expected: RED, weil Route, Handler und Port-Methode fehlen.

- [ ] **Step 3: Minimalen Port und Handler implementieren**

In `DiscordPort` ergänzen:

```rust
async fn delete_message(
    &self,
    channel_id: u64,
    message_id: u64,
    reason: &str,
) -> Result<(), PortError>;
```

Neuer authentifizierter Endpunkt:

```text
POST /internal/master/v1/discord/delete-message
{"channel_id":"…","message_id":"…","reason":"…"}
```

Der Handler nutzt `begin_action`, `positive_int`, einen deterministischen Payload-Hash und `run_idempotent`. `MessageNotFound` und `ChannelNotFound` sind erfolgreicher, idempotenter Zustand; andere Discord-Fehler ergeben 502.

Im Serenity-Adapter:

```rust
self.http
    .delete_message(ChannelId::new(channel_id), MessageId::new(message_id), Some(reason))
    .await
```

HTTP 404 wird auf `PortError::MessageNotFound` abgebildet. Alle vorhandenen Fake-Implementierungen erhalten eine explizite `unused`-Methode.

- [ ] **Step 4: Tests und Qualität prüfen**

```bash
cargo fmt --all -- --check
cargo test -p dl-broker delete_message -- --nocapture
cargo test -p dl-discord
cargo clippy -p dl-broker -p dl-discord --all-targets -- -D warnings
```

- [ ] **Step 5: Changelog #269, Commit und Push**

Der Eintrag beschreibt neutral: bisher konnten interne Consumer Review-Nachrichten nicht fristgerecht einzeln entfernen; der Broker besitzt nun eine idempotente Löschoperation; fehlende Nachrichten gelten als bereits erledigt.

```bash
git add CHANGELOG.md rust/crates/dl-broker/src rust/crates/dl-discord/src/adapter.rs
git commit -m "feat: delete Discord messages through broker" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push -u origin feature/ricky-review-delete-message
```

---

## Task 2: Löschvertrag im Twitch-Discord-Transport

**Repo:** `/home/naniadm/Documents/Deadlock-Twitch-Bot`

**Files:**

- Modify: `rust/crates/tb-transport-discord/src/backend.rs`
- Modify: `rust/crates/tb-transport-discord/src/relay.rs`
- Modify: `rust/crates/tb-transport-discord/src/noop.rs`
- Modify: `rust/bin/tb-bot/src/scam_notify_impl.rs`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Failing WireMock-Tests schreiben**

In `relay.rs`:

```rust
#[tokio::test]
async fn delete_message_sendet_auth_payload_und_idempotency_key()

#[tokio::test]
async fn delete_message_behandelt_404_als_bereits_geloescht()
```

Der erste Test prüft Pfad `/internal/master/v1/discord/delete-message`, `X-Internal-Token`, `Idempotency-Key` sowie Channel-/Message-ID. Der zweite antwortet 404 und erwartet `Ok(())`.

Run:

```bash
cd rust
cargo test -p tb-transport-discord delete_message -- --nocapture
```

Expected: RED, weil Payload und Trait-Methode fehlen.

- [ ] **Step 2: Minimalen Backend-Vertrag implementieren**

```rust
#[derive(Debug, Clone, Serialize)]
pub struct DeleteMessage {
    pub channel_id: i64,
    pub message_id: String,
    pub reason: String,
}

async fn delete_message(&self, payload: DeleteMessage) -> Result<(), DiscordError>;
```

`BrokerRelay` postet mit Präfix `delete`, akzeptiert 2xx und 404. `HeadlessNoop` und der Test-Backend in `scam_notify_impl.rs` implementieren den neuen Trait-Eintrag ohne Seiteneffekt.

- [ ] **Step 3: Testen und committen**

Vor dem ersten Code-Push den neutralen Changelog-Eintrag `#384` für den gesamten
Shadow-Review-Rollout oben anlegen; Task 8 prüft und finalisiert seine Aussagen.

```bash
cargo fmt --all -- --check
cargo test -p tb-transport-discord delete_message -- --nocapture
cargo test -p tb-bot scam_notify_impl -- --nocapture
cargo clippy -p tb-transport-discord --all-targets -- -D warnings
git add CHANGELOG.md rust/crates/tb-transport-discord rust/bin/tb-bot/src/scam_notify_impl.rs
git commit -m "feat: expose broker Discord deletion" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push
```

---

## Task 3: Eigene Review-Tabelle und Store

**Files:**

- Create: `rust/migrations/20260717120000_twitch_crew_review_events.sql`
- Create: `rust/crates/tb-engagement/src/crew_review.rs`
- Create: `rust/crates/tb-engagement/src/crew_review_store.rs`
- Create: `rust/crates/tb-engagement/tests/crew_review_store.rs`
- Modify: `rust/crates/tb-engagement/src/lib.rs`
- Modify: `rust/crates/tb-engagement/Cargo.toml`
- Modify: `rust/crates/tb-db/tests/fresh_schema_snapshot.txt`

- [ ] **Step 1: RED-Integrationstests für Vertrag schreiben**

Die Tests verwenden ausschließlich `TB_TEST_DATABASE_URL` und überspringen sich ohne Test-DB:

```rust
#[tokio::test]
async fn trigger_legt_session_und_nachricht_atomar_und_dedupliziert()

#[tokio::test]
async fn expires_at_sind_sechs_kalendermonate()

#[tokio::test]
async fn tombstone_entfernt_inhalt_aber_behaelt_delete_retry()

#[tokio::test]
async fn geloeschte_discord_gruppe_entfernt_erst_danach_db_zeilen()
```

Run:

```bash
./scripts/test_db.sh up
TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:5434/postgres \
  TB_TEST_REQUIRE_DB=1 cargo test -p tb-engagement --test crew_review_store -- --nocapture
```

Expected: RED wegen fehlender Migration und API.

- [ ] **Step 2: Migration implementieren**

Die Tabelle folgt exakt der Design-Spec, einschließlich `last_delete_error` und `tombstoned_at`. Zusätzlich:

```sql
CHECK (confidence IS NULL OR confidence BETWEEN 0.0 AND 1.0)

CREATE UNIQUE INDEX ...
ON twitch_crew_review_events (subject_twitch_user_id, source_message_id)
WHERE event_kind = 'ricky_message'
  AND source_message_id IS NOT NULL
  AND btrim(source_message_id) <> '';
```

Die drei Suchindizes aus der Spec kommen unverändert hinzu.

- [ ] **Step 3: Store mit dynamischem SQLx implementieren**

`crew_review.rs` enthält bereits die gemeinsamen, providerfreien Datentypen
`RickyChatInput`, `ReviewEventKind`, `NewReviewEvent`, `ReviewEvent`,
`ReviewCycle` und `ReviewSession` sowie die feste Twitch-ID. Damit bleibt dieser
Commit unabhängig von den späteren Chat-Adaptern kompilierbar. In
`Cargo.toml` kommen `serde`, `uuid` und die SQLx-Feature-Erweiterung `uuid`
hinzu; keine neue externe Bibliothek wird eingeführt.

Öffentliche Kern-API:

```rust
#[derive(Clone)]
pub struct CrewReviewStore { pool: PgPool }

pub async fn record_trigger(&self, input: &RickyChatInput) -> Result<Option<Uuid>, StoreError>;
pub async fn append_event(&self, event: NewReviewEvent) -> Result<i64, StoreError>;
pub async fn active_sessions(&self, now: DateTime<Utc>) -> Result<Vec<ReviewSession>, StoreError>;
pub async fn pending_model_inputs(&self, session_id: Uuid) -> Result<Vec<ReviewEvent>, StoreError>;
pub async fn pending_discord_cycles(&self, limit: i64) -> Result<Vec<ReviewCycle>, StoreError>;
pub async fn mark_discord_sent(&self, event_ids: &[i64], message_id: &str) -> Result<(), StoreError>;
pub async fn expired_discord_groups(&self, now: DateTime<Utc>, limit: i64) -> Result<Vec<ExpiredDiscordGroup>, StoreError>;
pub async fn tombstone_group(&self, message_id: &str, error_class: &str) -> Result<(), StoreError>;
pub async fn delete_expired_group(&self, message_id: &str) -> Result<u64, StoreError>;
pub async fn delete_expired_unposted(&self, now: DateTime<Utc>) -> Result<u64, StoreError>;
```

`record_trigger` nimmt pro Kanal einen PostgreSQL-Advisory-Transaction-Lock, findet die höchstens zehn Minuten alte offene Sitzung oder erzeugt eine neue und schreibt `session_started` plus `ricky_message`. Der Unique-Index macht EventSub/IRC-Doppelzustellung zu `Ok(None)`. Jede neue Ricky-Nachricht erhält eine neue `cycle_id`; `session_started` teilt beim ersten Trigger dieselbe ID. Alle neuen Ereignisse erhalten `expires_at = occurred_at + INTERVAL '6 months'` in PostgreSQL. Ein Modellzyklus gilt als verarbeitet, sobald zu seiner `cycle_id` ein `ai_decision` oder `provider_error` existiert; dadurch wird kein Input nach Neustart doppelt an den Provider geschickt.

Inhalte über 1.200 Zeichen werden an Wortgrenzen in mehrere Events mit `chunk_index`, `chunk_count` und derselben `cycle_id` geteilt.

- [ ] **Step 4: Migration und Store verifizieren**

```bash
./scripts/test-fresh-schema.sh
TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:5434/postgres \
  TB_TEST_REQUIRE_DB=1 cargo test -p tb-engagement --test crew_review_store -- --nocapture
cargo clippy -p tb-engagement --all-targets -- -D warnings
./scripts/test_db.sh down
```

- [ ] **Step 5: Commit und Push**

```bash
git add rust/migrations rust/crates/tb-engagement rust/crates/tb-db/tests/fresh_schema_snapshot.txt
git commit -m "feat: persist Ricky review events" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push
```

---

## Task 4: Exakter ID-Trigger in beiden Chatpfaden

**Files:**

- Modify: `rust/crates/tb-engagement/src/crew_review.rs`
- Modify: `rust/crates/tb-engagement/src/lib.rs`
- Modify: `rust/crates/tb-chat/src/crew_guard.rs`
- Modify: `rust/crates/tb-chat/src/pipeline.rs`
- Modify: `rust/bin/tb-bot/src/scout_chat.rs`
- Modify: `rust/bin/tb-bot/src/chat_wiring.rs`

- [ ] **Step 1: RED-Tests für Identität und Feature-Unabhängigkeit**

```rust
#[test]
fn exakte_ricky_id_triggert_auch_wenn_crew_guard_aus_ist()

#[test]
fn gleicher_login_mit_anderer_id_triggert_nicht()

#[tokio::test]
async fn scout_adapter_reicht_exakte_id_an_review_trigger_weiter()
```

Ein Fake-Trigger zeichnet ausschließlich `RickyChatInput` auf.

Run:

```bash
cargo test -p tb-chat crew_review_trigger -- --nocapture
cargo test -p tb-bot scout_review_trigger -- --nocapture
```

Expected: RED wegen fehlendem Trigger-Port.

- [ ] **Step 2: Kleinen synchronen Port ergänzen**

`RICKY_TWITCH_USER_ID` und `RickyChatInput` stammen bereits aus Task 3. Jetzt
kommt nur der synchrone Adaptervertrag hinzu:

```rust
pub trait CrewReviewTrigger: Send + Sync {
    fn observe(&self, input: RickyChatInput);
}
```

`CrewGuard` erhält einen optionalen Trigger und ruft ihn vor seinem bisherigen `enabled`-Early-Return ausschließlich bei exakter ID auf. Als deduplizierende Quellen-ID gilt `source_message_id`, wenn Twitch sie im Shared Chat liefert, sonst `message_id`. `ChatPipelineParts` reicht denselben Port an den EventSub-Guard.

`ScoutChatAdapter` erhält den Port separat, damit IRC auch bei `TB_CHAT_ENABLED=0` triggert. Der Unique-Index aus Task 3 fängt Doppelzustellung beider Pfade ab.

- [ ] **Step 3: Tests, Clippy, Commit**

```bash
cargo fmt --all -- --check
cargo test -p tb-chat crew_review_trigger -- --nocapture
cargo test -p tb-bot scout_review_trigger -- --nocapture
cargo clippy -p tb-chat -p tb-bot --all-targets -- -D warnings
git add rust/crates/tb-engagement/src rust/crates/tb-chat/src rust/bin/tb-bot/src
git commit -m "feat: trigger shadow review by exact Twitch ID" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push
```

---

## Task 5: Audio vollständig im Arbeitsspeicher und Whisper

**Files:**

- Modify: `rust/crates/tb-engagement/src/audio_capture.rs`
- Modify: `rust/crates/tb-engagement/src/transcribe.rs`

- [ ] **Step 1: RED-Tests mit Fake-Binaries und WireMock**

Neue Tests:

```rust
#[tokio::test]
async fn memory_capture_nutzt_ytdlp_url_und_ffmpeg_stdout()

#[tokio::test]
async fn memory_capture_schreibt_keine_audio_datei()

#[tokio::test]
async fn transcribe_bytes_sendet_whisper_1_und_language_de()
```

Fake-`yt-dlp` gibt eine harmlose Test-URL aus; Fake-`ffmpeg` gibt `RIFF...` nach stdout aus. Der Multipart-Test prüft `model=whisper-1`, `language=de` und Dateiname `audio.wav`.

Run:

```bash
cargo test -p tb-engagement memory_capture -- --nocapture
cargo test -p tb-engagement transcribe_bytes -- --nocapture
```

Expected: RED wegen fehlender Memory-APIs.

- [ ] **Step 2: Bestehende Module minimal erweitern**

```rust
pub struct MemoryAudioCapturer { ytdlp_bin: String, ffmpeg_bin: String }

pub async fn capture_wav(
    &self,
    channel_login: &str,
    duration: Duration,
) -> Result<Vec<u8>, CaptureError>;

pub async fn transcribe_bytes(
    &self,
    wav_bytes: Vec<u8>,
) -> Result<TranscriptionResult, String>;
```

`yt-dlp --get-url --no-playlist` löst den HLS-Pfad. Dieser Wert bleibt nur lokal in der Funktion und wird nie in Fehler/Log übernommen. `ffmpeg -t 20 -i <url> -vn -ac 1 -ar 16000 -c:a pcm_s16le -f wav pipe:1` liefert WAV nach stdout. `kill_on_drop(true)`, Timeout und Statusklasse verhindern hängende Prozesse. Der Vec wird nach dem OpenAI-Aufruf fallengelassen.

Die vorhandene dateibasierte API bleibt für ihre bisherigen Consumer erhalten; `transcribe_wav` delegiert intern nach dem Lesen an `transcribe_bytes`.

- [ ] **Step 3: Tests und Commit**

```bash
cargo fmt --all -- --check
cargo test -p tb-engagement memory_capture -- --nocapture
cargo test -p tb-engagement transcribe -- --nocapture
cargo clippy -p tb-engagement --all-targets -- -D warnings
git add rust/crates/tb-engagement/src/audio_capture.rs rust/crates/tb-engagement/src/transcribe.rs
git commit -m "feat: transcribe Twitch audio without files" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push
```

---

## Task 6: Fireworks-Entscheidung und Faktenvalidator

**Files:**

- Modify: `rust/crates/tb-engagement/src/crew_review.rs`
- Modify: `rust/crates/tb-engagement/Cargo.toml`

- [ ] **Step 1: RED-Tests für JSON-Vertrag und Guardrails**

```rust
#[test]
fn akzeptiert_valides_silent_initial_warning_und_reply_json()

#[test]
fn verwirft_unbekannte_fakten_id()

#[test]
fn verwirft_diagnose_augenzeugenschaft_und_rohes_schimpfwort()

#[tokio::test]
async fn fireworks_nutzt_exakten_endpoint_und_modellpfad()

#[tokio::test]
async fn fireworks_http_fehler_enthaelt_keinen_response_body()
```

Run:

```bash
cargo test -p tb-engagement crew_review -- --nocapture
```

Expected: RED wegen fehlendem Client und Parser.

- [ ] **Step 2: Fakten und strikt typisierte Entscheidung implementieren**

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecision {
    pub action: ReviewAction,
    pub topic_active: bool,
    pub confidence: f64,
    pub used_fact_ids: Vec<String>,
    pub reason: String,
    pub draft: Option<String>,
}
```

Validator:

- Confidence `0.0..=1.0`.
- `silent` verlangt `draft=null`; andere Aktionen verlangen 1–500 Zeichen.
- Jede Fakten-ID muss aus der festen Registry stammen.
- Rohes rassistisches Schimpfwort, Diagnose-/Extremismusbegriffe und persönliche Augenzeugenbehauptungen werden abgelehnt.
- Markdown-Fences um ein einzelnes JSON-Objekt dürfen entfernt werden; zusätzlicher Text wird abgelehnt.

Der Prompt gibt die fünf Fakten jeweils mit Quellenart an, verlangt natürliche kurze bis mittlere deutsche Chat-Sprache aus dritter Person und erlaubt ausschließlich die Formulierungsperspektive „nach dem, was ich dazu mitbekommen habe“. Er behauptet niemals eine menschliche Identität oder eigene Anwesenheit.

- [ ] **Step 3: Kleinen Fireworks-Client implementieren**

Konfiguration:

```text
FIREWORKS_API_KEY (Fallback nur auf vorhandenen Legacy-Namen FIREWORK_API_KEY)
FIREWORKS_BASE_URL=https://api.fireworks.ai/inference/v1
FIREWORKS_RICKY_REVIEW_MODEL=accounts/fireworks/models/deepseek-v4-flash
```

POST ausschließlich an `/chat/completions`, ohne Provider-Fallback. Fehler werden auf `unavailable`, `timeout`, `http_status`, `decode` oder `validation` reduziert. Weder Antwortbody noch Requestprompt werden im Fehlerstring gespeichert.

- [ ] **Step 4: Tests und Commit**

```bash
cargo fmt --all -- --check
cargo test -p tb-engagement crew_review -- --nocapture
cargo clippy -p tb-engagement --all-targets -- -D warnings
git add rust/crates/tb-engagement
git commit -m "feat: validate Fireworks review decisions" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push
```

---

## Task 7: Orchestrierung, Discord-Karten und Retention

**Files:**

- Create: `rust/bin/tb-bot/src/ricky_review_wiring.rs`
- Modify: `rust/bin/tb-bot/src/main.rs`
- Modify: `rust/bin/tb-bot/src/chat_wiring.rs`
- Modify: `rust/bin/tb-bot/src/scout_chat.rs`

- [ ] **Step 1: RED-Tests für den vollständigen Shadow-Zyklus**

Im neuen Modul mit Fake-Store/Provider oder einem kleinen Test-DB-Fixture prüfen:

```rust
#[tokio::test]
async fn trigger_bis_draft_schreibt_db_vor_discord()

#[tokio::test]
async fn providerfehler_schreibt_error_und_keinen_draft()

#[test]
fn components_v2_karte_hat_gold_und_keine_mentions()

#[tokio::test]
async fn cleanup_loescht_discord_vor_db()

#[tokio::test]
async fn cleanup_tombstoned_bei_discord_ausfall()
```

Run:

```bash
cargo test -p tb-bot ricky_review -- --nocapture
```

Expected: RED, weil Wiring fehlt.

- [ ] **Step 2: Trigger-Adapter und unabhängigen Startpunkt implementieren**

`PgRickyReviewTrigger::observe` macht ausschließlich einen kurzen `tokio::spawn` auf `CrewReviewStore::record_trigger`; die Chatpipeline blockiert nie.

Direkt nach Migration/DB-Verbindung in `main.rs`:

```rust
let ricky_review = ricky_review_wiring::start(
    &supervisor,
    pool.clone(),
    &settings.broker,
);
```

Der Startpunkt läuft unabhängig von `TB_CHAT_ENABLED`. Der optionale Trigger wird an EventSub-`ChatPipelineParts` und `ScoutChatAdapter` gereicht.

- [ ] **Step 3: Drei kleine Hintergrundläufe implementieren**

1. `ricky_review_processor`, Tick 5 Sekunden:
   - Offene Vorgänger-Sessions beim Start mit Grund `process_restart` schließen.
   - Abgelaufene Sitzungen nach zehn Minuten ohne Ricky-Nachricht, eindeutige Namensnennung oder `topic_active=true` beenden.
   - Neue Ricky-Nachrichten sofort durch Fireworks entscheiden lassen.
   - Für aktive Sitzungen 20-Sekunden-Audio im RAM erfassen, mit OpenAI transkribieren und nur bei nichtleerem Text erneut entscheiden.
   - Nicht verfügbare Streams beenden die Sitzung; andere Providerfehler erzeugen `provider_error`.

2. `ricky_review_discord_forwarder`, Tick 60 Sekunden:
   - Pending `cycle_id`s laden, deterministisch in Karten bis 3.500 Zeichen packen.
   - Components V2 Container `type=17`, Gold `0xC8A86B`, Textblöcke `type=10`.
   - `SendRichMessage { channel_id: 1374364800817303632, content: None, embed: {}, components: Some(...), allowed_role_ids: vec![], view_spec: None }`.
   - Erst nach erfolgreichem Broker-POST die Message-ID allen enthaltenen Event-IDs zuordnen.

3. `ricky_review_retention`, sofort und danach täglich:
   - Ungepostete abgelaufene Events direkt löschen.
   - Pro vollständig abgelaufener Discord-Gruppe zuerst `delete_message` aufrufen.
   - 2xx/404: DB-Zeilen löschen.
   - Andere Fehler: Inhalte sofort tombstonen und nur technische Retry-Daten behalten.

- [ ] **Step 4: Shadow-Grenze strukturell testen**

```bash
! rg -n "ChatApi|send_chat|send_message|tb_transport_twitch|tb-transport-twitch" \
  rust/bin/tb-bot/src/ricky_review_wiring.rs \
  rust/crates/tb-engagement/src/crew_review.rs \
  rust/crates/tb-engagement/src/crew_review_store.rs
```

Expected: kein Treffer.

- [ ] **Step 5: Tests, Clippy, Commit**

```bash
cargo fmt --all -- --check
cargo test -p tb-bot ricky_review -- --nocapture
cargo test -p tb-chat crew_review_trigger -- --nocapture
cargo clippy -p tb-bot -p tb-chat -p tb-engagement --all-targets -- -D warnings
git add rust/bin/tb-bot/src rust/crates/tb-chat/src rust/crates/tb-engagement/src
git commit -m "feat: run Ricky review in shadow mode" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push
```

---

## Task 8: Betrieb, Doku, Changelog und Evaluierung

**Repos:** Twitch-Bot und `/home/naniadm/Documents/Deadlock-Docs`

**Files:**

- Modify: `rust/scripts/run_tb_bot_service.sh`
- Modify: `rust/crates/tb-chat/tests/ricky_eval.rs`
- Modify: `CHANGELOG.md`
- Modify: `/home/naniadm/Documents/Deadlock-Docs/internal/deadlock-twitch-bot/architektur.html`
- Modify: `/home/naniadm/Documents/Deadlock-Docs/internal/deadlock-twitch-bot/datenmodell.html`
- Modify: `/home/naniadm/Documents/Deadlock-Docs/internal/deadlock-twitch-bot/betrieb.html`

- [ ] **Step 1: Service-Konfiguration ergänzen**

Nicht geheime Defaults:

```bash
export RICKY_SHADOW_REVIEW_ENABLED="${RICKY_SHADOW_REVIEW_ENABLED:-1}"
export RICKY_SHADOW_REVIEW_CHANNEL_ID="${RICKY_SHADOW_REVIEW_CHANNEL_ID:-1374364800817303632}"
export RICKY_SHADOW_REVIEW_SEGMENT_SECONDS="${RICKY_SHADOW_REVIEW_SEGMENT_SECONDS:-20}"
export RICKY_SHADOW_REVIEW_YTDLP_BIN="${RICKY_SHADOW_REVIEW_YTDLP_BIN:-$ROOT_DIR/.venv/bin/yt-dlp}"
export FFMPEG_BIN="${FFMPEG_BIN:-/usr/bin/ffmpeg}"
export FIREWORKS_BASE_URL="${FIREWORKS_BASE_URL:-https://api.fireworks.ai/inference/v1}"
export FIREWORKS_RICKY_REVIEW_MODEL="${FIREWORKS_RICKY_REVIEW_MODEL:-accounts/fireworks/models/deepseek-v4-flash}"
```

Keine Secret-Werte oder Secret-Defaults ergänzen. Vor Deploy wird über den
erlaubten Loader-`--list`-Pfad ausschließlich geprüft, dass `OPENAI_API_KEY`
und `FIREWORKS_API_KEY` als Secret-Namen vorhanden sind. Werte werden weder
gelesen noch ausgegeben.

- [ ] **Step 2: Deterministische Eval-Fälle ergänzen**

Der vorhandene redigierte Ricky-Korpus wird um lokale Parser-/Guardrail-Fälle ergänzt. Normale CI ruft keinen echten Provider auf. Erwartet werden nur erlaubte Fakten-IDs, keine Diagnosen und maximal 500 Zeichen Entwurf.

- [ ] **Step 3: Changelog #384 schreiben**

Neutral und ohne öffentliche Namensnennung: Problem = interne Moderationshinweise waren nicht zusammenhängend prüfbar und Discord-Kopien hatten keine eigene Frist; Änderung = exakter Account-Trigger, RAM-Transkription, beleggebundene Shadow-Entwürfe, dedizierte sechsmonatige Review-Ablage; aktuelles Verhalten = nur interner Review, kein Twitch-Versand, automatische Löschung in DB und Discord.

- [ ] **Step 4: Interne HTML-Dokumentation aktualisieren**

In einem neuen Docs-Branch `feature/ricky-shadow-review-docs` die Architektur-, Datenmodell- und Betriebsseiten ergänzen. Dokumentieren: Prozessgrenzen, Tabelle/Eventarten, Provider/Modelle, Flags, 10-Minuten-Session, 6-Monats-Cleanup, Broker-Delete, Shadow-only. Keine Prompttexte, Rohbelege, personenbezogenen Chat-Inhalte oder Secrets aufnehmen.

- [ ] **Step 5: Vollständige Twitch-Verifikation und Commit**

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot/rust
cargo fmt --all -- --check
cargo test -p tb-transport-discord
cargo test -p tb-engagement
cargo test -p tb-chat
cargo test -p tb-bot
cargo clippy -p tb-transport-discord -p tb-engagement -p tb-chat -p tb-bot \
  --all-targets -- -D warnings
./scripts/test-fresh-schema.sh
```

Dann `CHANGELOG.md`, Service-Skript und Eval committen/pushen. Docs separat committen/pushen.

---

## Task 9: Merge, Deploy und Live-Beweis

- [ ] **Step 1: Eigenes Changed-Files-Review**

In beiden Code-Repos:

```bash
git diff --stat main...HEAD
git diff --check main...HEAD
git status --short --branch
git log --oneline main..HEAD
```

Jede geänderte Datei gegen diese Spec prüfen; insbesondere keine Secret-Ausgabe, keine direkte Discord-Berechtigung im Twitch-Bot und keinen Twitch-Sendepfad akzeptieren.

- [ ] **Step 2: Deadlock-Bots zuerst mergen und deployen**

```bash
git checkout main
git pull --ff-only
git merge --no-ff feature/ricky-review-delete-message \
  -m "merge: Discord review deletion" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push origin main
cd rust
cargo build --release --workspace
```

Vorherige PID erfassen, `systemctl --user restart deadlock-bot-rust.service`, danach beweisen: neue PID, `/proc/<pid>/exe` zeigt auf die frisch gebaute `dl-bot`-Binary, Journal seit Neustart enthält kein `error|panic|fatal`.

- [ ] **Step 3: Twitch-Bot mergen und deployen**

```bash
git checkout main
git pull --ff-only
git merge --no-ff feature/ricky-shadow-review \
  -m "merge: Ricky shadow review" \
  -m "Co-authored-by: GPT 5.4 <gpt-5.4@local>"
git push origin main
cd rust
cargo build --release --workspace
```

`systemctl --user restart deadlock-twitch-bot-rust.service` und dieselben drei Live-Beweise für `tb-bot` ausführen. Zusätzlich nur technische Signale prüfen:

- Migration ist vorhanden.
- Review-Prozessor, Discord-Forwarder und Retention melden aktiv/bereit.
- Provider-Konfiguration wird ausschließlich als vorhanden/fehlend gemeldet, nie als Wert.
- Keine Twitch-Nachricht wurde durch dieses Feature gesendet.

- [ ] **Step 4: Docs mergen und Branches sicher entfernen**

Docs-Branch nach grünem HTML-/Linkcheck nach `main` mergen und pushen. Vor jedem Löschen `git merge-base --is-ancestor <branch> main` prüfen. Danach Remote-/Lokalbranches entfernen und `git worktree prune`; fremde Worktrees/Branches bleiben unberührt.

- [ ] **Step 5: Sechsmonats-Cleanup im Testsystem beweisen**

In der Test-DB einen künstlich abgelaufenen Review-Zyklus mit einer WireMock-Discord-ID anlegen. Belegen, dass der Delete-Aufruf vor dem DB-Delete geschieht und ein simulierter Discord-Ausfall den Inhalt tombstoned. Niemals Produktions-Chatdaten oder echte Discord-Nachrichten für diesen Test verändern.
