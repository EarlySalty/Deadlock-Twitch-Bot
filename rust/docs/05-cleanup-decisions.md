# 05 — Aufgeräumt statt 1:1

Konkrete Konsolidierungen aus den im Mapping belegten Smells. Diese werden im Rust-Design bewusst
**anders** gelöst als im Python-Original — kein blindes Übersetzen. Jeder Punkt nennt den Ist-Zustand
und die Ziel-Lösung.

## 1. `pg.py`-Godfile (4326 Z.) → `tb-db`-Submodule + Migrations-SSOT

Schema, Pool, Sessions und Domain-Queries (Global-Ban, Billing, Clip-Templates) werden getrennt.
Alle heute über `engagement/`, `chat/`, `analytics/`, `social_media/`, `migrations/` verstreuten
DDLs ziehen in **ein** `migrations/`-Verzeichnis (sqlx-native via `sqlx::migrate!`). Die linearen `if/elif`-Versionssprünge
(v1–v7) und `_pg_add_col_if_missing`-Drift entfallen.

## 2. Eigenbau-LIFO-Pool (`_pool.py`) → sqlx-Pool

Komplett ersetzen, keine Eigenimplementierung. Migrationen laufen ebenfalls über sqlx
(`sqlx::migrate!`) — **ein** PG-Treiber, kein refinery/tokio-postgres-Doppel.

## 3. Mixin-Gottklassen auflösen *(durchgängig)*

- `RaidChatBot` (8 Mixins) → `ChatConnection` / `Moderator` / `PromoScheduler` / `CommandHandler`
- `eventsub_mixin.py` (3413 Z., 92 Fn) → `EventSubManager` / `SubscriptionRegistry` /
  `WhisperNotifier` / `CapacityReporter`
- `auth.py` (1797 Z.) → `OAuthFlow` / `TokenRefresher` / `StateStore`
- `token_error_handler.py` (1402 Z.) → `OAuthBlacklist` / `GracePeriod` / `DmNotifier`
- `base.py` (2416 Z.) → `DiscordWiring` / `InviteManager` / `AlertSender`
- `DashboardV2Server` (10 Mixins) + `api_v2.py` (2848 Z.) → axum-Router-Module mit shared `AppState`

Komposition statt Method-Resolution-Order.

## 4. Dual-State-Stores auf eine Quelle reduzieren

`oauth_state_tokens` (DB) + `_state_tokens` (In-Memory-Dict) → **DB-only** mit
`SELECT … FOR UPDATE` (kein Split-Brain bei Restart). Ebenso `_eventsub_webhook_tracked` (set) +
`_active_subs` (list) → ein Typ.

## 5. Duplikate zusammenführen

- `backend.py` / `backend_extended.py` (identische Methoden) → ein Aggregations-Modul.
- `_safe_int` / `_safe_float` / `_parse_dt` / `_clamp` / `_row_value` (8–10× kopiert) →
  `tb-domain` bzw. `raid::util`.
- Doppelte ffprobe-/yt-dlp-Logik (upload_worker vs base) → ein `VideoProcessor`.
- `TWITCH_TOKEN_URL` u.ä. (3× definiert) → `tb-transport-twitch`-Konstanten.
- Frontend: drei parallele `fetchJson`-Wrapper → einer. *(Frontend bleibt React, aber dieser
  Aufräum-Punkt gehört zum Vertrag.)*

## 6. Inline-DDL aus Request-Handlern eliminieren

`_ensure_internal_home_changelog_storage`, `_billing_ensure_storage_tables`, `_ensure_*_schema`
in raid/scores laufen heute pro Request/Webhook. → einmalig beim Start als `tb-db`-Migration,
nie im Hot-Path.

## 7. Time-Series-Tabellen sauber modellieren

`twitch_stats_tracked` und `_category` (PK-los, strukturgleich) → konsolidieren oder beide als
korrekte Hypertable mit Constraint. **Während der Migration Schema halten**, aber als bekannte
Schuld dokumentiert (siehe [`02-db-contract.md`](02-db-contract.md)).

## 8. Tote/experimentelle Features bewusst weglassen oder isolieren

- `demo_data.py` (3589 Z.) → `include_str!`-Fixtures hinter Feature-Flag, raus aus dem Prod-Pfad.
- `exp_sessions_mixin` (experimentell, parallel zum echten Session-System) → Entscheid vor Port:
  ablösen oder klar geflaggtes `tb-monitoring`-Feature.
- `abbo_entry` (738 Z., nie als Route registriert), `compose_vertical` (Fn + Methode doppelt),
  `keyring`-„DeadlockBot"-Windows-Pfad (auf Linux tot), `tokens.py`-Windows-Credential-Manager →
  weglassen.
- `InternalApiClient` (fast leer) → in Basis-Client auflösen.

## 9. Zweigleisige Discord-Sends abstrahieren

Die heute über `monitoring.py` verstreute Umschaltlogik (direkt vs Broker-Relay) →
`enum AnnouncementTransport { BrokerRelay, HeadlessNoop }` im `DiscordBackend`-Trait.
HeadlessBot-Duck-Typing → explizites Trait. *(Gateway-Variante entfällt, da Discord per ADR 0001
ausschließlich über die Bridge läuft.)*

## 10. Legacy-CSRF-Kopplung brechen

HTML-Scraping von `/twitch/admin/announcements` als CSRF-Quelle → dedizierter JSON-CSRF-Endpoint,
**bevor** admin_dashboard migriert wird.

## 11. `fpdf2`-Abhängigkeit

Kein API-gleicher Rust-Port. Bewusster Entscheid (vor Billing-Phase): `printpdf`/`genpdf` mit
manuellem Layout **oder** Python-Sidecar nur für das Gutschrift-PDF, bis Rust-PDF reif ist.
Nicht 1:1 erzwingen. Siehe [`06-open-questions.md`](06-open-questions.md).

## 12. `exp_sessions`-Doppelsystem → nach Cutover konsolidieren

`exp_sessions`/`exp_snapshots`/`exp_game_transitions` sind ein paralleler Session-Tracker
(„Experimental-Analytics") neben `twitch_stream_sessions` — klassische Doppelung. Sie haben aber
echte Konsumenten (AI-Stream-Reports lesen das Game-Breakdown, `/twitch/api/v2/exp/game-transitions`),
deshalb portiert Schritt 4 die 4 Write-Hooks dünn mit (Schema-Vertrag). **Ziel-Lösung:** Nach dem
Monitoring-Cutover gehen Game-Transitions als Spalten/Beziehung ins Haupt-Session-Modell und die
`exp_*`-Tabellen entfallen; die Konsumenten werden auf die Haupt-Tabellen umgezogen.

## 13. Guard-GC raus aus dem Claim-Hot-Path

Python löscht bei **jedem** `claim` alle abgelaufenen Guard-Rows (`DELETE … WHERE expires_at <= now`)
— unnötiger Write-Traffic pro Event. Die Korrektheit steckt allein im konditionalen Upsert.
Rust: `claim` ist ein einzelner Upsert, die GC läuft als periodischer `sweep_expired` im Poll-Loop.

## 14. External-Recruitment-Follow-ups bei Raid-Arrival → bewusst auf Phase 6g zurückgestellt

`arrival_confirmation.rs` berechnet bei der Bestätigung einer Raid-Ankunft fünf Folge-Flags
(`should_delete_external_recruitment_blacklist_pending`, `should_persist_confirmed_external_recruitment_raid`,
`should_schedule_external_recruitment_blacklist_pending`, `should_send_partner_raid_message`,
`should_send_recruitment_message`). Der Wiring-Adapter in `bin/tb-bot/src/raid_arrival_wiring.rs`
**konsumiert sie noch nicht** — das Port-Audit (13.6.) hat das korrekt als „tote Flags" markiert.

**Warum bewusst offen:** Die dahinterliegenden Effekte sind das External-/Partner-Recruitment-Subsystem
(Python `raid_arrival_runtime.py:265-268,337-363,403-416` → `record_confirmed_external_recruitment_raid`
auf `twitch_confirmed_external_recruitment_raids`, `delete/maybe_schedule_external_recruitment_blacklist_pending`
auf `twitch_external_recruitment_blacklist_pending` inkl. Bot-Ban-Check-Scheduler, sowie
`send_partner_raid_message`/`send_recruitment_message` via Broker). In Rust existiert von dieser
Infrastruktur **nichts** außer den Flags. Laut Cutover-Plan ist „6g Recruitment" bis zur Outreach-Phase
**pausiert** — auch auf der Python-Seite läuft das Recruitment-Anhängsel derzeit nicht.

**Entscheidung:** Die Flags bleiben gesetzt-aber-inert, bis Phase 6g die Recruitment-Schicht (Daten-Stores
+ Broker-Messaging) nativ baut. Das ad-hoc gegen reverse-engineerte Python-Schemas vorzuziehen wäre gegen
die bewusste Staffelung. **Beim Bau von 6g zu schließen:** die drei Daten-Effekte (delete/persist/schedule)
zuerst (frühes Abbrechen bei `record … == None` wie Python Z.354), die zwei Messaging-Effekte über den
Master-Broker. Bis dahin ist diese Zurückstellung hier die Single Source of Truth.

## 15. Port-Audit-Backlog (13.6.) — bewusst zurückgestellt bzw. nicht gefixt

Aus dem Port-Audit (`docs/audit/2026-06-13-...md`) sind die meisten Med/Low-Befunde
gefixt (Welle 1–4: Entitlements, Admin-Präzedenz, Percentile, Retention, Panic,
Doppel-Ban-Event, Lurker-SQL, channel.points-Telemetrie, deterministischer Announce-
Token, correlation_status, Nicht-Deadlock-Raid-Auflösung, get_users-Chunking u.a.).
Folgendes ist **bewusst offen** — entweder echte Feature-Ports (kein Backlog-Cleanup)
oder bewusste Nicht-Fixes:

**Größere Feature-Ports (eigene Arbeit, nicht „Bugfix"):** — ✅ ERLEDIGT 13.6.:
Targeted-Promo-Presets (1:1), Ad-Viewer-Drop-Rework, Discord-Action-Scope-Guard,
viewer-detail.personality (`classify_message`), **!clip nativ** (Helix `POST /clips`
+ Broadcaster-Token via RaidAuthStore; Bot-Fallback bewusst weg, da kein `clips:edit`).
Noch offen:
- `viewer-detail.personality` — braucht den `_classify_message`-Port (war per #180 schon bewusst `null`).
- `/stats` vier Sektionen (retention/chat/discovery/content_performance) — großer zweiter DB-Block aus `leaderboard.py:1135-1256`.
- `verify` Nicht-Partner-Promote (`promote_streamer_to_partner` + `backfill_tracked_stats_from_category`) — Lifecycle-Port; bis dahin verifiziert verify nur aktive Partner.
- Targeted-Promo-Presets 1:1 (Texte/IDs/Tags + Tag-Struktur `&'static str`→Slice).
- Lurker-Tax Per-Session-Mention-Dedup (session-keyed State) + Bot-Token-Scope-Fallback (TokenManager-Scope-Injektion).
- Spam-Filter periodischer Reload-Loop (ArcSwap + 120s-Task) — Self-Learning greift sonst erst nach Neustart.
- `channel.ban` Bot-Selbst-Timeout-Erkennung (TimeoutGuard + Bot-ID ins Dispatch durchreichen).
- Ad-Viewer-Drop Vorzeichen/Fenster-Rework (5-Min-Mittel vor/nach statt Punkt/Min-während).
- Score-Snapshot zur Sendezeit einfrieren (`PendingRaid`-Feld + Plumbing) statt frisch zur Confirm-Zeit.
- iapi `discord-flag`/`discord-profile` Scope-Guard — Helfer-Refactor (`enforce_scope_allowlist` crate-weit); **niedriges Praxis-Risiko**, da die interne API 8776 loopback-only ist (UFW + Loopback-Middleware).
- Helix-GET-Retry (3× Backoff bei 5xx/Netz) — querschnittliche Änderung am `get()→send()`-Muster aller Caller.

**Bewusste Nicht-Fixes (Rust besser als Python):**
- `rankings ORDER BY value DESC NULLS LAST` — Python lässt es weg → Postgres-Default `NULLS FIRST` setzt NULL-Werte (keine Daten) an die Spitze des Wachstums-Rankings. Rusts `NULLS LAST` ist korrekt; **nicht** auf Pythons Quirk zurückgebaut.

**Marginale Kosmetik (near-zero Impact, zurückgestellt):**
Scam-Warntext-Whitespace, Banker's- vs. Half-away-Rundung, EventSub-AVG-2-Dezimal-Rundung,
Byte- vs. Codepoint-Längenprüfung bei link-click-Text, Float-`channel_id`-Trunkierung,
p95-Utilization-Index-Methode, diverse Tie-Break-/Timing-Edge-Cases (retention `type:"unknown"`-
ad_break-Klassifizierung, telemetry `a or b or 0`-Kette, viewer-directory sort=first_seen).
