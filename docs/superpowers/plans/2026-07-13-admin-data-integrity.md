# Admin Data Integrity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan.

**Goal:** Vier irreführende bzw. unvollständige Admin-Ansichten auf belastbare Live- und Persistenzdaten umstellen.

**Architecture:** Bestehende Tabellen und Handler werden additiv erweitert. Der Dashboard-Server bleibt für Admin-Queries und Auditierung zuständig; der Twitch-Bot persistiert seinen EventSub-Livezustand. Das React-Frontend erhält nur die notwendigen Felder, Filter und Zustandsanzeigen.

**Tech Stack:** Rust, Axum, SQLx/PostgreSQL, React, TypeScript, Vite.

---

### Task 1: Partner seit

**Files:** `rust/crates/tb-analytics/src/admin_streamers.rs`, `rust/crates/tb-dashboard-api/src/handlers/admin_streamers.rs`, `bot/admin_dashboard/src/api/types.ts`, `bot/admin_dashboard/src/pages/streamers/StreamerList.tsx`

1. Roten Rust-Vertragstest für `twitch_raid_auth.created_at`, Von-/Bis-Filter und Sortierung ergänzen.
2. Query/API additiv um `partner_since` erweitern und Zieltest grün machen.
3. Spalte, Sortierung und Datumsfilter im Frontend ergänzen; Produktionsbuild ausführen.

### Task 2: Research-Vorschläge

**Files:** `rust/crates/tb-dashboard-api/src/handlers/admin_research.rs`, Routerdatei, `bot/admin_dashboard/src/api/client.ts`, `bot/admin_dashboard/src/api/types.ts`, `bot/admin_dashboard/src/pages/community/Research.tsx`

1. Roten Handler-/Ranking-Test für Nicht-Partner, Reihenfolge und Limit ergänzen.
2. Additiven Suggestions-Endpoint mit vorhandenen Research-Metriken implementieren.
3. Rangliste samt Lade-, Leer- und Fehlerzustand einbauen; Build ausführen.

### Task 3: EventSub-Livezustand

**Files:** EventSub-Reconcile-/Monitoringcode, `rust/crates/tb-dashboard-api/src/handlers/system/eventsub.rs`, `rust/crates/tb-analytics/src/system_eventsub.rs`, `bot/admin_dashboard/src/pages/monitoring/EventSubStatus.tsx`

1. Roten Test für veraltete Snapshots und gespeicherte Subscriptions ergänzen.
2. Snapshot-Schreibpfad an den tatsächlichen Rust-Subscription-Zustand anbinden.
3. Handler und UI für `stale`/offline sowie Subscription-Zeilen korrigieren.

### Task 4: Vollständiges Admin-Audit

**Files:** neue SQL-Migration, Dashboard-Router/Middleware, `rust/crates/tb-dashboard-api/src/handlers/admin_audit_log.rs`

1. Roten Test für persistierte mutierende Requests und fehlende Read-Requests ergänzen.
2. Append-only Audit-Tabelle und serverseitige Middleware ohne Body/Query/Secrets implementieren.
3. Neue Quelle in die bestehende Audit-Abfrage aufnehmen und UI-Vertrag erhalten.

### Task 5: Abschluss

**Files:** `CHANGELOG.md`, relevante technische Dokumentation

1. Zieltests, kompletter betroffener Rust-Testlauf, Build, Clippy, Format-Check und Frontend-Build ausführen.
2. Änderungen reviewen, Changelog ergänzen, committen und Feature-Branch pushen.
3. Nach `main` mergen, pushen, Release bauen, betroffene User-Services neu starten.
4. PID-Wechsel, `/proc/<pid>/exe`, fehlerfreie Journals und Live-API-Verträge nachweisen.
