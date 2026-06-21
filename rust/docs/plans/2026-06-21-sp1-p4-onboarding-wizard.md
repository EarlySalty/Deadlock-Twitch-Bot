# SP1 / P4 — Onboarding-Wizard + Steam-Link (Fork X) (Implementierungsplan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ein geführter, resumierbarer Onboarding-Wizard im Verwaltungs-Dashboard (dashboard_v2), der dem Streamer in 3–5 Schritten zeigt, was der Bot kann, ihn durch die **bestehenden** Discord- und Steam-Verknüpfungs-Flows führt (Identitäts-Spine) und den Fortschritt persistiert — damit „90 % verstehen den Bot nicht" gelöst wird.

**Architecture (Fork X):** Kein neuer Link-Code. Der Wizard verlinkt die **bestehenden** Flows: Discord-Verknüpfung (`GET /twitch/auth/discord/link`, schreibt `twitch_streamer_identities.discord_user_id` via `set_discord_profile`) und darüber die bestehende Steam-Verknüpfung (Discord-OpenID-Flow). Der Steam-Status wird über die P5-Kette geprüft (`twitch_user_id → discord_user_id → steam-core /rank`). Fortschritt liegt in neuer Tabelle `streamer_onboarding`. Frontend = neuer Tab in dashboard_v2 (React, URL-Routing), Backend = authentifizierte, CSRF-freie v2-Handler.

**Tech Stack:** Rust/Axum/sqlx (Postgres), dashboard_v2 (React 19 + Vite). Wiederverwendung: Discord-OAuth-Flow, `set_discord_profile`, P5-Resolver (`stats::resolve_discord_id` + `/rank`), Website-Onboarding-Pattern (`StreamerOnboardingPage`/`OnboardingProgress`) als visuelle Vorlage.

**Voraussetzung:** **P1 gemergt**; **P5-Resolver/`/rank`-Endpoint vorhanden** (für den Steam-Status-Schritt) — alternativ Wizard zuerst ohne Live-Status bauen, Status-Badge als P5-Nachzug.

## Global Constraints

- Rust-Standard; Code unter `rust/` + `bot/dashboard_v2/`. Keine DB-Migration außer der einen Onboarding-Tabelle.
- **User-sichtbare deutsche Texte** (Wizard-Schritte, Buttons, Erklärungen) schreibt **Claude**. Inhaltsquelle: bestehende Texte wiederverwenden — `twitchKnowledgeBase.ts` `ONBOARDING_VISUAL_STEPS` + Steam-Panel-Text aus `Deadlock-Bots/cogs/welcome_dm/step_steam_link.py:37-72` (verbatim, „Discord-ID"→Kontext anpassen).
- **Keine Verknüpfungs-Chatbefehle** (User-Lock): alles im Dashboard.
- v2-POSTs **CSRF-frei** + authentifiziert (`DashboardAuthLevel::Partner`), Muster `silent_settings.rs`/`lurker_tax_settings.rs`.
- Git/Delegation wie P1; GPT baut, Claude reviewt + schreibt DE-Texte.

---

## Dateistruktur

**Neu:**
- `rust/migrations/20260621080000_streamer_onboarding.sql`
- `rust/crates/tb-dashboard-api/src/handlers/onboarding.rs` (GET status + POST step/complete)
- `bot/dashboard_v2/src/pages/Onboarding.tsx`
- `bot/dashboard_v2/src/components/onboarding/OnboardingWizard.tsx` + Step-Komponenten

**Geändert:**
- `rust/crates/tb-dashboard-api/src/lib.rs` (Routen in `build_authed_router`)
- `bot/dashboard_v2/src/App.tsx` + `TabNavigation.tsx` (neuer Tab/Route)
- `CHANGELOG.md`

---

## Task 1: Migration `streamer_onboarding`

**Files:** Create `rust/migrations/20260621080000_streamer_onboarding.sql`.

- [ ] **Step 1: Migration**

```sql
-- Onboarding-Fortschritt pro Streamer (resumierbar).
CREATE TABLE IF NOT EXISTS public.streamer_onboarding (
    twitch_user_id   TEXT PRIMARY KEY,
    twitch_login     TEXT NOT NULL,
    current_step     INTEGER NOT NULL DEFAULT 0,
    completed        BOOLEAN NOT NULL DEFAULT FALSE,
    completed_at     TIMESTAMPTZ,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 2: Commit** — `git commit -m "feat(db): streamer_onboarding (Wizard-Fortschritt)"`.

---

## Task 2: Backend — Onboarding-Status + Steam-Status

**Files:** Create `rust/crates/tb-dashboard-api/src/handlers/onboarding.rs`; Modify `lib.rs`.

**Interfaces:**
- `GET /twitch/api/v2/streamer/onboarding` → `{ current_step, completed, discord_linked, steam_linked }`.
- `POST /twitch/api/v2/streamer/onboarding` Body `{ current_step?, completed? }` → upsert.
- Steam-/Discord-Status: `discord_linked` = `twitch_streamer_identities.discord_user_id IS NOT NULL`; `steam_linked` = via P5-Resolver (`stats::resolve_discord_id` → `stats::fetch_rank(discord_id).linked`).

- [ ] **Step 1: Handler-Muster lesen** — `rg -n "DashboardAuthLevel|fn post_handler|resolve_login" rust/crates/tb-dashboard-api/src/handlers/silent_settings.rs` (Extractor + Identität + Upsert-Muster übernehmen).

- [ ] **Step 2: Failing test (reiner Status-Mapper)** — in `onboarding.rs` eine pure Funktion `fn onboarding_json(step: i32, completed: bool, discord: bool, steam: bool) -> serde_json::Value` + Test, dass die Felder exakt `current_step/completed/discord_linked/steam_linked` heißen.

- [ ] **Step 3: Handler implementieren** — GET liest `streamer_onboarding` (Default step 0/not completed falls kein Row) + `discord_linked` aus `twitch_streamer_identities` + `steam_linked` via Resolver (best-effort; bei HTTP-Fehler `steam_linked:false`). POST upsert `current_step`/`completed` (`completed=true` setzt `completed_at=NOW()`). Identität aus `DashboardAuthLevel::Partner { twitch_user_id, twitch_login }`.

- [ ] **Step 4: Routen** in `build_authed_router` (bei `streamer/*`): `.route("/twitch/api/v2/streamer/onboarding", get(onboarding::get_status).post(onboarding::post_status))`.

- [ ] **Step 5: Build/Test + Commit** — `cargo test -p tb-dashboard-api onboarding`; `git commit -m "feat(dashboard-api): Onboarding-Status + Discord/Steam-Link-Status"`.

---

## Task 3: Frontend — Wizard im dashboard_v2 (Claude-Texte)

**Files:** Create `bot/dashboard_v2/src/pages/Onboarding.tsx` + `components/onboarding/OnboardingWizard.tsx` (+ Step-Komponenten); Modify `App.tsx`, `TabNavigation.tsx`.

**Interfaces:** Konsumiert `GET/POST /twitch/api/v2/streamer/onboarding`. Schritte (Claude-Texte, Inhalt aus `ONBOARDING_VISUAL_STEPS` + Steam-Panel):
1. **Willkommen / Was der Bot kann** (kurzer Überblick: Auto-Raid, Moderation, Dashboard, Discord-Go-Live).
2. **Discord verknüpfen** → Button öffnet `GET /twitch/auth/discord/link?next=…`; Status `discord_linked`.
3. **Steam verknüpfen** → Hinweis + Link zum Discord-Steam-Verknüpfen (Panel-Text wiederverwendet); Status `steam_linked`; „danach zeigt !rank deinen Rang".
4. **Go-Live-Tipps** → Opt-in/Hinweis auf den Go-Live-Tipp (P3) + Opt-out-Toggle.
5. **Abschluss** → „Fertig!"-Button setzt `completed`.

- [ ] **Step 1: dashboard_v2-Routing lesen** — `App.tsx` (URL-Matching ~180-260) + `TabNavigation.tsx`: wie ein Tab/Conditional-Render ergänzt wird; `useAuthStatus()` für Identität.

- [ ] **Step 2: `OnboardingWizard`** — linearer, resumierbarer Stepper (lädt `current_step` initial, `OnboardingProgress`-Muster von der Website adaptieren), Fortschritt via POST persistieren, Abschluss-Button. Schritt-Status (discord/steam_linked) aus GET; erledigte Schritte als ✓.

- [ ] **Step 3: Step-Komponenten** mit den Claude-Texten (deutsch, Umlaute). Discord-/Steam-Schritt mit echtem Link-Button + Live-Status-Refresh.

- [ ] **Step 4: Tab/Route** in `App.tsx` + `TabNavigation` einhängen (First-Run: Wizard prominent, solange `!completed`).

- [ ] **Step 5: Build** — `cd bot/dashboard_v2 && npm run build` (Output `bot/analytics/dashboard_v2/dist`). Commit.

---

## Task 4: Verifikation, CHANGELOG, Push, Spiegelung

- [ ] **Step 1: Gesamt** — `cargo build/test/clippy/fmt -p tb-dashboard-api`; `cd bot/dashboard_v2 && npm run build`. Grün.
- [ ] **Step 2: CHANGELOG (Claude, oben)** — `## #N — Geführtes Onboarding im Dashboard`. Drei Schläge: (1) neue Streamer standen vor einem leeren Dashboard und verstanden den Bot nicht; (2) jetzt führt ein Wizard in wenigen Schritten durch Funktionen, Discord- und Steam-Verknüpfung, Go-Live-Tipps — resumierbar, mit Fortschritt; (3) die Steam-Verknüpfung schaltet `!rank` frei und zieht Streamer zugleich in den Discord. Kein Datei-/Funktionsname.
- [ ] **Step 3: Commit + Push + Merge + Cleanup** (wie P1).
- [ ] **Step 4: Spiegelung** In-App + Discord (`target:"twitch"`).
- [ ] **Step 5: Live-Smoke** — Test-Streamer ohne Discord-Link: Wizard zeigt Schritt „Discord verknüpfen" offen; nach Verknüpfung ✓; Steam-Status korrekt; „Fertig" persistiert `completed` (Reload = resumiert/abgeschlossen).

---

## Self-Review (vom Plan-Autor)

**1. Spec-Coverage (§5 Wizard):** 3–5 lineare, resumierbare Schritte mit Fortschritt + Abschluss ✓ (T3); Schritte Profil/Steam verknüpfen → Stat/Go-Live → Abschluss ✓; Identitäts-Spine (Steam via Discord) ✓ (Fork X); Inhalt aus Bestandstexten ✓. In-Chat-Feedback („probier !rank") = über P3-Tipps/`!rank` abgedeckt; Wizard-Status im Dashboard.

**2. Placeholder-Scan:** Migration + Status-Mapper + Routen konkret; Handler/Frontend an benannte Muster (`silent_settings.rs`, `App.tsx`-Routing, `OnboardingProgress`) gebunden + `rg`-Leseschritte (keine geratene dashboard_v2-Struktur). Deutsche Texte = Claude, Quelle benannt.

**3. Typ-Konsistenz:** `{current_step, completed, discord_linked, steam_linked}` einheitlich GET-Handler (T2) ↔ Wizard-Konsum (T3). `steam_linked` via P5 `stats::resolve_discord_id`/`fetch_rank` (gleiche Signaturen wie P5).

**Abhängigkeit:** Steam-Status-Schritt nutzt P5 (`/rank` + Resolver) → P5 vor P4s Live-Status bauen, oder Wizard zuerst mit „Steam verknüpfen"-Link ohne Live-Badge, Badge als P5-Nachzug.

**Scope-Grenze P4:** kein neuer Link-/OpenID-Code (Fork X), keine Co-Stream-Features (SP3). P4 endet: geführter, resumierbarer Wizard im Dashboard, der Verständnis schafft und die Identitäts-Spine setzt — live verifiziert.
