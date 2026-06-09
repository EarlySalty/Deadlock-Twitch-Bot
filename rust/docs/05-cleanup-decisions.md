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
