# Dashboard Schema Reconcile 2026-06-29

Wahrheitsquelle: `rust/docs/audit/live_schema_dump_2026-06-29.txt`.
Migrationen wurden fuer diesen Abgleich nicht verwendet.

## Kurzfassung

- Geaenderte Dashboard-Read-Queries / Decode-Pfade: 12.
- Inhaltlich geaenderte Dateien: 14 plus dieser Report und `WORKFLOW.md`.
- Hinweis: Der geforderte Befehl `cargo fmt -p tb-dashboard-api -p tb-analytics`
  hat darueber hinaus viele bereits vorhandene Dateien in beiden Packages neu
  formatiert. Diese Formatierung ist im Working Tree sichtbar, aber nicht
  Bestandteil der fachlichen Reconciliation.

## Geaenderte Query-Abgleiche

### `tb-dashboard-api/src/handlers/engagement_settings.rs` - `get_log_handler`

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `cost_usd_estimate` | Live `numeric`; Query-Ausdruck jetzt `cost_usd_estimate::float8` | Decode `Option<f64>` direkt aus `numeric` | Decode `Option<f64>` aus `float8`-Cast |
| `prompt_tokens`, `completion_tokens`, `latency_ms` | `integer` | `Option<i32>` | unveraendert, bestaetigt |
| `ts` | `timestamp with time zone` | `DateTime<Utc>` | unveraendert, bestaetigt |

### `tb-dashboard-api/src/handlers/internal_home.rs` - `row_ts_iso`-Callsites

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `twitch_live_state.last_started_at` | Live `text` | helper versuchte nur `Option<DateTime<Utc>>` | helper akzeptiert `timestamptz`, `date` und `text` |
| `twitch_live_state.last_seen_at` | Live `text` | helper versuchte nur `Option<DateTime<Utc>>` | helper normalisiert parsebares Text-ISO zu RFC3339 |
| Ban-/Raid-/Session-Timestamps | `timestamp with time zone` | `Option<DateTime<Utc>>` | weiterhin unterstuetzt |

### `tb-dashboard-api/src/handlers/internal_home.rs` - `access_state_block` Partner

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `archived_at` | `COALESCE(admin_archived_at, departnered_at)` aus Live-`text` | Decode `Option<DateTime<Utc>>` | Decode `Option<String>`, non-empty gilt als gesetzt |
| `manual_partner_opt_out` | `integer` | `Option<i32>` | unveraendert, bestaetigt |

### `tb-dashboard-api/src/handlers/internal_home.rs` - `twitch_token_blacklist`

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `grace_expires_at` | Live `text` | Decode `Option<DateTime<Utc>>` | Decode `Option<String>`, toleranter Parser nach `DateTime<Utc>` |
| `error_count` | Live `integer` | Decode `Option<i64>` | Decode `Option<i32>`, danach `i64::from` |
| `role_removed` | Live `integer` | `Option<i32>` | unveraendert, bestaetigt |

### `tb-analytics/src/partner_access.rs` - `twitch_token_blacklist`

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `grace_expires_at` | Live `text`, Query `::text` | `Option<String>` | unveraendert, bestaetigt |
| `error_count` | Live `integer` | `Option<i64>` | `Option<i32>` |
| `role_removed` | Live `integer` | `Option<i32>` | unveraendert, bestaetigt |

### `tb-dashboard-api/src/handlers/admin_chat_action.rs` - `partner_send_allowed`

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `archived_at` | `twitch_partners_all_state.archived_at` live `text` | Decode `Option<DateTime<Utc>>` | Decode `Option<String>`, non-empty gilt als archiviert |
| `manual_partner_opt_out` | `integer` | `Option<i32>` | unveraendert, bestaetigt |

### `tb-analytics/src/ai_analysis.rs` - Deadlock-Filter

| Ausgabe / Praedikat | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `had_deadlock_in_session` in dynamischem `gf` | Live `boolean` | `had_deadlock_in_session = 1` | `COALESCE(had_deadlock_in_session, false)` |
| `deadlockSummary`-Query | Live `boolean` | `had_deadlock_in_session = 1` | `COALESCE(had_deadlock_in_session, false)` |
| Aggregates `COUNT`, `SUM(int4)`, `AVG(...)` | `bigint` / `float8` nach Cast | Rust `i64` / `f64` | unveraendert, bestaetigt |

### `tb-analytics/src/streamers_crud.rs` - `list_streamers`

| Ausgabe / Praedikat | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `last_deadlock_stream_at` CASE-Praedikat | `had_deadlock_in_session` live `boolean` | `had_deadlock_in_session = 1` | `COALESCE(had_deadlock_in_session, false)` |
| `last_deadlock_stream_at` | `MAX(timestamptz)` | `Option<DateTime<Utc>>` | unveraendert, bestaetigt |

### `tb-analytics/src/monetization.rs` - Ad-Drop-Analyse

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `a.is_automatic` | Live `boolean` | Decode `Option<i32>`, `!= 0` | Decode `Option<bool>` |
| `duration_seconds` | `integer` | `Option<i32>` | unveraendert, bestaetigt |
| `started_at` | `timestamp with time zone` | `DateTime<Utc>` | unveraendert, bestaetigt |

### `tb-analytics/src/monetization.rs` - Ad-Aggregat

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `auto_ads` | `SUM(CASE WHEN bool THEN 1 ELSE 0 END)::bigint` | `a.is_automatic = 1` | `COALESCE(a.is_automatic, false)` |
| `total_ads`, `sessions_with_ads` | `COUNT(*)::bigint`, `COUNT(DISTINCT ...)::bigint` | `i64` | unveraendert, bestaetigt |
| `avg_duration` | `AVG(integer)::float8` | `Option<f64>` | unveraendert, bestaetigt |

### `tb-analytics/src/monetization.rs` - Subs-Aggregat

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `gifted` | `SUM(CASE WHEN bool THEN 1 ELSE 0 END)::bigint` | `su.is_gift = 1` | `COALESCE(su.is_gift, false)` |
| `total_events` | `COUNT(*)::bigint` | `i64` | unveraendert, bestaetigt |

### `tb-analytics/src/overview.rs` / `tb-dashboard-api/src/handlers/overview.rs`

| Ausgabe | Live-/Ausdruckstyp | Alt | Neu |
| --- | --- | --- | --- |
| `duration_seconds`, `peak_viewers`, `follower_delta`, `followers_*`, `*_chatters` in Fixtures | Live `integer` | Test-DDL `BIGINT` | Test-DDL `INTEGER` |
| `retention_*`, `dropoff_pct` in Fixtures | Live `double precision` | Test-DDL `REAL` | Test-DDL `DOUBLE PRECISION` |
| Query-Ausgaben mit `::BIGINT` | Ausdruck `bigint` | Rust `i64` | unveraendert, bestaetigt |

## Fixture-Korrekturen

| Datei | Tabelle/Spalte | Live-Typ | Korrektur |
| --- | --- | --- | --- |
| `tb-analytics/src/admin_affiliate.rs` | `affiliate_streamer_claims.id` | `integer` | Test-DDL von `BIGSERIAL` auf `INTEGER GENERATED BY DEFAULT AS IDENTITY` |
| `tb-analytics/src/admin_affiliate.rs` | `affiliate_commissions.id` | `integer` | Test-DDL von `BIGSERIAL` auf `INTEGER GENERATED BY DEFAULT AS IDENTITY` |
| `tb-analytics/src/admin_affiliate.rs` | `affiliate_gutschriften.id` | `integer` | Test-DDL von `BIGSERIAL` auf `INTEGER GENERATED BY DEFAULT AS IDENTITY` |
| `tb-analytics/src/ai_analysis.rs` | `twitch_stream_sessions.had_deadlock_in_session` | `boolean` | Test-DDL und Inserts auf `BOOLEAN` / `TRUE` / `FALSE` |
| `tb-analytics/src/streamers_crud.rs` | `twitch_stream_sessions.had_deadlock_in_session` | `boolean` | Test-DDL und Inserts auf `BOOLEAN` / `TRUE` |
| `tb-analytics/src/monetization.rs` | `twitch_ad_break_events.is_automatic` | `boolean` | Test-DDL und Inserts auf `BOOLEAN` |
| `tb-analytics/src/monetization.rs` | `twitch_subscription_events.is_gift` | `boolean` | Test-DDL und Inserts auf `BOOLEAN` |
| `tb-dashboard-api/src/handlers/monetization.rs` | `is_automatic`, `is_gift` | `boolean` | Handler-Test-DDL auf `BOOLEAN` |
| `tb-dashboard-api/src/handlers/system/health.rs` | `twitch_live_state.last_seen_at`, `last_started_at` | `text` | Handler-Test-DDL und Inserts auf `TEXT` |
| `tb-dashboard-api/src/handlers/system/health.rs` | `twitch_raw_chat_ingest_health.*_at`, `updated_at` | `text` | Handler-Test-DDL und Inserts auf `TEXT` |
| `tb-dashboard-api/src/handlers/partner_login.rs` | `twitch_partners.partnered_at`, `departnered_at`, `admin_archived_at` | `text` | Handler-Test-DDL auf `TEXT` |
| `tb-dashboard-api/src/handlers/overview.rs`, `tb-analytics/src/overview.rs` | Stream-Session-Metriken | `integer` / `double precision` | Test-DDL auf Live-Typen |

## Geprueft Ohne Codeaenderung

| Bereich | Ergebnis |
| --- | --- |
| `tb-analytics::admin_streamers` und `handlers/admin_streamers` | Partner-/Live-State-Flags bleiben `i32`, TEXT-Timestamps bleiben `String`, timestamptz aus Raid/Auth bleibt `DateTime<Utc>`. |
| `system_oauth_scopes` | `manual_partner_opt_out` wird als `::bigint`-Ausdruck dekodiert; `archived_at` ist Text. |
| `system_health` Analytics-Modul | Casts von Live-Texttimestamps nach `timestamptz` sind absichtlich Query-Ausdruecke; Decode bleibt `DateTime<Utc>`. |
| `system_eventsub` | int4-Snapshot-Spalten werden in der Query `::bigint` gecastet; Decode bleibt `i64`. |
| `admin_config`, `promo_mode`, `admin_announcements`, `billing`, `roadmap`, `market`, `ai_history`, `session_detail` | Output-Typen passen zu Live-Spalten oder expliziten Aggregat-/Cast-Ausdruecken. |

## Unklare Faelle / Nicht Blind Geaendert

| Stelle | Befund | Risiko |
| --- | --- | --- |
| `tb-analytics/src/system_errors.rs` | Query nutzt `twitch_admin_error_log`; diese Tabelle kommt im Live-Dump nicht vor. | Admin-System-Errors koennen je nach Live-DB mit Missing-Table-Fallback leer bleiben; kein Typfix moeglich ohne Live-Spalte. |
| `tb-dashboard-api/src/handlers/affiliate_portal.rs` | Query liest `twitch_streamers.display_name`; Live-Dump fuer `twitch_streamers` enthaelt nur `id`, `twitch_login`, `twitch_user_id`, `created_at`. | Portal-Display-Name-Query kann gegen die Live-DB als fehlende Spalte scheitern. Nicht geraten, weil unklar ist, ob `display_name` aus anderer Quelle kommen soll. |

## Verifikation

```text
cargo fmt -p tb-dashboard-api -p tb-analytics
```

lief ohne Ausgabe.

```text
set -o pipefail; cargo clippy -p tb-dashboard-api -p tb-analytics 2>&1 | tail -25
```

Exit 0. Tail enthaelt bestehende Warnungen in `auth/streamer_scope.rs` und `handlers/demo.rs`; keine neuen Fehler.

```text
set -o pipefail; cargo test -p tb-dashboard-api 2>&1 | tail -15
```

`681 passed; 0 failed; 1 ignored`.

```text
set -o pipefail; cargo test -p tb-analytics 2>&1 | tail -15
```

`359 passed; 0 failed; 0 ignored`.
