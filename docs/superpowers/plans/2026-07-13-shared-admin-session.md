# Shared Admin Session Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Discord- und Twitch-Admin-Dashboard verwenden dieselbe zentral widerrufbare Session und Twitch erhält CSRF-Tokens ohne Legacy-HTML-Scraping.

**Architecture:** `dl-dashboard` bleibt Autorität für `master_dash_session`. `tb-dashboard` importiert eigene Logins zentral, validiert jede Admin-Session zentral und hält nur einen lokalen Spiegel für Fingerprint- und CSRF-Daten. Der Browser bekommt weiterhin genau ein Domain-Cookie.

**Tech Stack:** Rust/Axum/SQLx/Reqwest, React/TypeScript, Python `unittest` für den kleinen Frontend-Quellvertrag.

## Global Constraints

- Kein neues Dependency.
- Keine Caddy- oder Datenbankschema-Änderung.
- TDD: Jeder Verhaltensschritt muss vor der Implementierung sichtbar rot sein.
- Zentrale API-Ausfälle gewähren keinen neuen Zugriff.
- Mehrere gleichnamige Cookies werden vollständig geprüft.
- Logout löscht das Domain-Cookie auch dann, wenn der zentrale Widerruf fehlschlägt.

---

### Task 1: Zentrale Session widerrufbar machen

**Files:**
- Modify: `/home/naniadm/.worktrees/Deadlock-Bots-shared-admin-session/rust/crates/dl-dashboard/src/web.rs`
- Modify: `/home/naniadm/.worktrees/Deadlock-Bots-shared-admin-session/CHANGELOG.md`

**Interfaces:**
- Consumes: `SessionStore::remove(&str)` und `guard_twitch`.
- Produces: `POST /internal/twitch/v1/discord/revoke-session` mit `{ "session_id": "..." }`; Erfolg `{ "ok": true }`.

- [ ] **Step 1: Failing Router-Test schreiben**

Im bestehenden `web.rs`-Testmodul eine Session importieren und dann den neuen Endpunkt aufrufen:

```rust
let response = router(app.clone())
    .oneshot(internal_post(
        "/internal/twitch/v1/discord/revoke-session",
        "twitch-test-token",
        json!({ "session_id": "shared-session" }),
    ))
    .await
    .expect("response");
assert_eq!(response.status(), StatusCode::OK);
assert!(app.inner.sessions.touch("shared-session", now_unix_f64()).await.unwrap().is_none());
```

- [ ] **Step 2: RED belegen**

Run: `RUSTC_WRAPPER= cargo test -p dl-dashboard revoke_session_entfernt_gemeinsame_session -- --nocapture`

Expected: FAIL, weil die Route noch `404 Not Found` liefert.

- [ ] **Step 3: Minimalen Handler implementieren**

Route und Handler neben `validate_session`/`import_session` ergänzen:

```rust
.route(
    "/internal/twitch/v1/discord/revoke-session",
    post(revoke_session),
)

async fn revoke_session(
    State(app): State<DashboardApp>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = guard_twitch(&app, &peer, &headers) {
        return resp;
    }
    let payload = match parse_value(&body) {
        Ok(value) => value,
        Err(resp) => return resp,
    };
    let session_id = string_field(&payload, "session_id").unwrap_or_default();
    if session_id.is_empty() {
        return err_json(400, "missing_session_id");
    }
    match app.inner.sessions.remove(&session_id).await {
        Ok(()) => ok_json(json!({ "ok": true })),
        Err(error) => {
            tracing::error!(%error, "Admin-Session konnte nicht widerrufen werden");
            err_json(500, "session_persistence_failed")
        }
    }
}
```

- [ ] **Step 4: GREEN und kompletter Crate-Test**

Run: `RUSTC_WRAPPER= cargo test -p dl-dashboard --lib`

Expected: 45 Tests grün.

- [ ] **Step 5: Changelog, Commit und Push**

`CHANGELOG.md` ergänzt Problem → zentraler Widerruf → gemeinsamer Logout. Danach:

```bash
git add CHANGELOG.md rust/crates/dl-dashboard/src/web.rs
git commit -m "fix(auth): gemeinsame Admin-Session widerrufen" -m "Co-authored-by: GPT-5 Codex <gpt-5-codex@local>"
git push -u origin fix/shared-admin-session-revoke
```

### Task 2: Twitch mit der zentralen Session-API verbinden

**Files:**
- Modify: `rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs`
- Modify: `rust/crates/tb-dashboard-api/src/auth/session.rs`

**Interfaces:**
- Produces: `ValidatedAdminSession { user_id, username, display_name, expires_at }`.
- Produces: `DiscordAdminOAuthClient::{validate_session, import_session, revoke_session}`.
- Produces: `DashboardAuthState::import_central_admin_session` und `admin_csrf_token`.

- [ ] **Step 1: Failing Session-Store-Test schreiben**

```rust
let mirrored = state
    .import_central_admin_session("central-id", "42", "admin", "Admin", 9_999_999_999.0)
    .await
    .unwrap();
assert_eq!(mirrored.session_id, "central-id");
assert!(!mirrored.csrf_token.is_empty());
assert_eq!(state.admin_csrf_token("central-id").await.unwrap().as_deref(), Some(mirrored.csrf_token.as_str()));
```

- [ ] **Step 2: RED belegen**

Run: `RUSTC_WRAPPER= cargo test -p tb-dashboard-api import_central_admin_session_behaelt_id_und_erzeugt_csrf -- --nocapture`

Expected: Compile-FAIL, weil beide Methoden fehlen.

- [ ] **Step 3: Lokalen Spiegel minimal implementieren**

`import_central_admin_session` persistiert dieselbe ID als `discord_admin`, setzt `source: "discord_dashboard"`, `fp_pending: false`, `js_fp: "discord_validated"` und erzeugt einen CSRF-Token. `admin_csrf_token` liest `csrf_token` aus dem gültigen Payload.

```rust
pub async fn admin_csrf_token(&self, session_id: &str) -> Result<Option<String>, sqlx::Error> {
    Ok(self
        .fetch_session_payload(session_id, "discord_admin", unix_now())
        .await?
        .and_then(|payload| payload.get("csrf_token").and_then(Value::as_str).map(str::to_string)))
}
```

- [ ] **Step 4: Broker-Vertrag testen und implementieren**

Dem vorhandenen Mock einen aufgezeichneten Validate-/Import-/Revoke-Aufruf geben. Danach im Trait und HTTP-Client implementieren:

```rust
async fn validate_session(&self, session_id: &str) -> Result<ValidatedAdminSession, DiscordAdminOAuthError>;
async fn import_session(&self, session: &ValidatedAdminSession, session_id: &str) -> Result<(), DiscordAdminOAuthError>;
async fn revoke_session(&self, session_id: &str) -> Result<(), DiscordAdminOAuthError>;
```

Die Pfade sind `/internal/twitch/v1/discord/validate-session`, `import-session` und `revoke-session`; `valid != true` ist ein Fehler.

- [ ] **Step 5: Twitch-Login und Logout synchronisieren**

Nach `create_discord_admin_session` wird vor dem Cookie `client.import_session(...)` aufgerufen. Bei Fehler wird die lokale Session invalidiert und 503 ohne Cookie geliefert. Logout ruft `revoke_session` auf, invalidiert lokal und löscht das Cookie unabhängig vom Revoke-Ergebnis.

- [ ] **Step 6: GREEN**

Run: `RUSTC_WRAPPER= cargo test -p tb-dashboard-api auth::discord_admin_login auth::session -- --nocapture`

Expected: alle gefilterten Tests grün.

- [ ] **Step 7: Commit und Push**

```bash
git add rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs rust/crates/tb-dashboard-api/src/auth/session.rs
git commit -m "fix(auth): zentrale Admin-Session synchronisieren" -m "Co-authored-by: GPT-5 Codex <gpt-5-codex@local>"
git push
```

### Task 3: Zentrale Cookies in Twitch akzeptieren und CSRF ausliefern

**Files:**
- Modify: `rust/crates/tb-dashboard-api/src/auth/level.rs`
- Modify: `rust/crates/tb-dashboard-api/src/handlers/auth_status.rs`

**Interfaces:**
- Consumes: Task 2 `validate_session`, `import_central_admin_session`, `admin_csrf_token`.
- Produces: gültiges `DashboardAuthLevel::Admin` für zentral ausgestellte Cookies und `csrfToken`/`csrf_token` im Auth-Status.

- [ ] **Step 1: Failing Extractor-Test schreiben**

Ein Mock validiert nur den zweiten von zwei `master_dash_session`-Werten. Der Test erwartet Admin und einen lokalen Spiegel für die zweite ID.

```rust
let auth = extract_auth_with_config(
    request_parts(Some("master_dash_session=stale; master_dash_session=central".into())),
    state.clone(),
    config,
).await;
assert!(matches!(auth, DashboardAuthLevel::Admin { .. }));
assert!(state.load_admin_session("central").await.unwrap().is_some());
```

- [ ] **Step 2: RED belegen**

Run: `RUSTC_WRAPPER= cargo test -p tb-dashboard-api zentrale_admin_session_und_cookie_duplikat -- --nocapture`

Expected: FAIL, weil nur der erste lokale Cookie-Kandidat geprüft wird.

- [ ] **Step 3: Extraktor minimal ändern**

Alle Cookie-Kandidaten sammeln. Wenn `DiscordAdminLoginConfig` vorhanden ist, jeden Kandidaten zentral validieren und fehlende lokale Spiegel importieren; ohne Config bleibt der bestehende lokale Test-/Dev-Pfad erhalten. Kein zentral gültiger Kandidat bedeutet `None`.

- [ ] **Step 4: Failing Auth-Status-Test schreiben**

```rust
assert_eq!(json["csrfToken"], created.csrf_token);
assert_eq!(json["csrf_token"], created.csrf_token);
```

- [ ] **Step 5: RED belegen und Auth-Status implementieren**

Run: `RUSTC_WRAPPER= cargo test -p tb-dashboard-api auth_status_liefert_admin_csrf -- --nocapture`

Expected zunächst: `null`; danach liest der Handler den gültigen Admin-Cookie über `DashboardAuthState::admin_csrf_token` und gibt denselben Wert in beiden JSON-Feldern zurück.

- [ ] **Step 6: GREEN und kompletter Crate-Test**

Run: `RUSTC_WRAPPER= cargo test -p tb-dashboard-api --lib`

Expected: bisherige 756 plus neue Tests grün, ein bestehender Test ignoriert.

- [ ] **Step 7: Commit und Push**

```bash
git add rust/crates/tb-dashboard-api/src/auth/level.rs rust/crates/tb-dashboard-api/src/handlers/auth_status.rs
git commit -m "fix(auth): gemeinsame Session und CSRF ausliefern" -m "Co-authored-by: GPT-5 Codex <gpt-5-codex@local>"
git push
```

### Task 4: Legacy-CSRF-Scraping entfernen und ausrollen

**Files:**
- Create: `tests/test_admin_dashboard_csrf_contract.py`
- Modify: `bot/admin_dashboard/src/api/client.ts`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Auth-Status `csrfToken` aus Task 3.
- Produces: alle JSON- und Form-Schreibaktionen verwenden `resolveJsonCsrfToken`; keine HTML-Abhängigkeit.

- [ ] **Step 1: Failing Frontend-Vertrag schreiben**

```python
from pathlib import Path

CLIENT = Path(__file__).parents[1] / "bot/admin_dashboard/src/api/client.ts"

def test_admin_writes_do_not_scrape_legacy_html_for_csrf():
    source = CLIENT.read_text(encoding="utf-8")
    assert "LEGACY_CSRF_PAGE" not in source
    assert "fetchLegacyCsrfToken" not in source
    assert "await resolveJsonCsrfToken(fields)" in source
```

- [ ] **Step 2: RED belegen**

Run: `python -m pytest tests/test_admin_dashboard_csrf_contract.py -q`

Expected: FAIL wegen `LEGACY_CSRF_PAGE`.

- [ ] **Step 3: Minimalen Client-Fix schreiben**

`LEGACY_CSRF_PAGE` und `fetchLegacyCsrfToken` löschen. `resolveJsonCsrfToken` wirft nach erfolglosem Auth-Status `ApiError("CSRF-Token fehlt.", 403)`. `submitLegacyAction` bezieht seinen Token mit:

```typescript
const csrfToken = fields.csrf_token || cachedCsrfToken || (await resolveJsonCsrfToken(fields));
```

- [ ] **Step 4: Frontend und Gesamtumfang verifizieren**

Run:

```bash
python -m pytest tests/test_admin_dashboard_csrf_contract.py -q
npm --prefix bot/admin_dashboard run build
RUSTC_WRAPPER= cargo test --manifest-path rust/Cargo.toml -p tb-dashboard-api --lib
RUSTC_WRAPPER= cargo clippy --manifest-path rust/Cargo.toml -p tb-dashboard-api -p tb-dashboard --all-targets -- -D warnings
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
```

Expected: alles grün, keine Warnungen.

- [ ] **Step 5: Changelog, Commit und Push**

`CHANGELOG.md` ergänzt Cookie-Desync → zentrale Synchronisierung/CSRF-API → ein Login/Logout für beide Dashboards. Danach committen und pushen.

- [ ] **Step 6: Beide Feature-Branches nach `main` mergen**

Je Repo: aktuellen Zustand prüfen, `main` aktualisieren, `--no-ff` mergen, Gate-Fehler beheben, `main` pushen. Keine fremden Änderungen überschreiben.

- [ ] **Step 7: Release-Build, Neustart und Live-Beweis**

Deadlock-Bots: `cargo build --release --workspace`, betroffenen Dashboard-Service neu starten. Twitch-Bot: `cargo build --release --workspace`, `tb-dashboard.service` neu starten. Je Service PID-Wechsel, `/proc/<pid>/exe` und Journal ohne `error|panic|fatal` prüfen. Danach beide Admin-URLs mit demselben Cookie, Twitch-Schreibaktion und gemeinsamen Logout live belegen.
