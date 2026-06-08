# storage/ — Architektur & Funktionsreferenz

> Pfad: `bot/storage/` · Stand: 2026-06-08 · 8 Dateien, ~7.430 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [DATABASE.md](../DATABASE.md) (alle Tabellen + Spalten), [core.md](core.md) (`partner_utils` liest die Partner-View), [raid.md](raid.md) (Global-Ban + Auto-Raid-Pause), [reference: Partner-DB](../../README.md).

## 1. Zweck & Abgrenzung

`bot/storage/` ist die **einzige Postgres-Zugriffsschicht** des Systems. Beide Services (BotRuntime und DashboardRuntime) teilen sich **dieselbe Datenbank** und genau dieses Modul — direkte Python-Importe zwischen den Services gibt es nicht, die DB ist der gemeinsame Nenner (siehe [Architektur-Split](README.md)).

Das Modul kapselt:
- **Connection-Pooling** (`_pool.py`): prozesslokale psycopg-Pools pro DSN.
- **Schema-Bootstrap & Wartung** (`pg.py`): idempotentes Anlegen aller Tabellen/Views/Indizes.
- **Transaktions-Helfer** (`pg.py`): Context-Manager + Retry-Logik für Deadlocks.
- **Partner-Lebenszyklus** (`partner_registry.py`): Promote/Reactivate/Departner, History, Auto-Raid-Eligibility.
- **Verschlüsselte Web-Sessions** (`sessions_db.py`): Fernet-verschlüsselte Dashboard-Sessions + atomare Rate-Limit-Slots.
- **Kleine Zustands-Stores**: Promo-Cooldowns, Auto-Raid-Pause, globale Chatter-Bannliste, Observability-Events.

Abgrenzung: Hier liegt **kein** Domänenwissen über Raids/Analytics/Chat — nur „wie kommen Daten rein und raus“. Die fachlichen Queries der Analytics liegen in `bot/analytics/backend*.py`.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | praktisch allen Modulen; `core/partner_utils` (Partner-View), `raid/` (Auth-IDs, Global-Ban, Pause), `monitoring/`, `analytics/`, `dashboard/` (Sessions), `chat/` (Promo-Cooldowns, Global-Ban). |
| **Nutzt selbst** | extern `psycopg` (PostgreSQL), `cryptography`/Fernet (Session-Verschlüsselung), `keyring` (Fernet-Key-Ablage), `bot.secret_store`. |
| **DB** | die zentrale PostgreSQL-DB („analytics database“). DSN aus Secret (z. B. `TWITCH_ANALYTICS_DSN`), geladen über `_load_dsn()` — **nie** im Klartext geloggt; `analytics_db_fingerprint()` liefert nur einen gehashten Identitäts-Fingerprint. |
| **Externe Dienste** | keine direkten (reiner DB-Layer). |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `pg.py` | 4327 | Kern: Bootstrap, Schema, Connections/Transaktionen, Global-Ban-Store, Raid-Auth-IDs, Observability, Re-Export der Partner-API. |
| `partner_registry.py` | 2215 | Partner-Lebenszyklus + Legacy-Migration; definiert die meisten `set_*`/`promote_*`/`reactivate_*`-Funktionen. |
| `sessions_db.py` | 305 | Fernet-verschlüsselte Dashboard-Sessions + atomare Rate-Limit-Slots. |
| `_pool.py` | 290 | `PostgresConnectionPool`, `ConnectionPoolRegistry` (pro-DSN), `ConnectionStats`. |
| `auto_raid_pause.py` | 118 | Admin-gesetzte Auto-Raid-Pause (set/clear/get/is). |
| `promo_cooldowns.py` | 77 | Persistenz der Chat-Promo-Cooldowns. |
| `_rows.py` | 54 | `StorageRow` — dict-/sequence-artiger Zeilen-Wrapper. |
| `__init__.py` | 47 | Re-Export der öffentlichen Symbole aus `pg.py` + Submodulen. |

## 4. Datenfluss / Lebenszyklus

**Bootstrap (einmalig beim Start):** `prepare_runtime_storage()` läuft, bevor Traffic bedient wird → ruft `ensure_schema()` (legt idempotent alle Tabellen/Views/Indizes an, verwaltet `schema_version`), `ensure_billing_entitlement_schema()` und Startup-Maintenance (`_run_startup_maintenance`, u. a. Duplikat-Cleanups). Der INDEX nennt das als „DB-Bootstrap“.

**Lesen:** `with readonly_connection() as conn:` zieht eine **gepoolte** Raw-Connection aus dem prozesslokalen Pool (autocommit-Read). Bequeme One-Shots: `query_one(sql, params)`, `query_all(sql, params)`.

**Schreiben:** `with transaction() as conn:` (explizites commit/rollback) oder `run_transaction(operation, retries=…)` — Letzteres führt eine Schreib-Operation mit **begrenzten Retries** bei Deadlock/Serialization-Fehlern aus. Spezialvarianten: `repeatable_read_transaction`, `serializable_transaction`.

**Pooling:** Die `ConnectionPoolRegistry` hält **einen Pool pro DSN** (`get_pool(dsn)`); jeder `PostgresConnectionPool` verwaltet bis `max_size` Verbindungen mit Checkout-Timeout, Ping-vor-Nutzung und `open_dedicated()` für lang laufende dedizierte Verbindungen (z. B. EventSub-Listener).

**Partner-Lebenszyklus:** Streamer werden als `twitch_streamers`-Zeile angelegt; Verifizierung/Promotion schreibt den Partner-Status (`promote_streamer_to_partner`, `verification_payload(mode)`). Die kanonische Wahrheit lesen alle über die **Views** `twitch_streamers_partner_state` / `twitch_partners_all_state` (nicht über Rohspalten — siehe [core.md](core.md)).

**Sessions:** Dashboard-Login-Sessions liegen Fernet-verschlüsselt in `dashboard_sessions`; `pop_session` konsumiert atomar (Advisory-Lock), `reserve_rate_limit_slot` reserviert atomar einen Rate-Limit-Slot pro Bucket.

## 5. Funktionsreferenz pro Datei

### __init__.py
Definiert die **öffentliche Storage-API** durch Re-Export aus `pg.py`, `promo_cooldowns`, `auto_raid_pause`. Wer `from bot.storage import …` schreibt, sieht genau diese Symbole.

### pg.py
Moduldoc: „PostgreSQL storage layer for Twitch analytics.“ 79 Funktionen, keine Klassen; die Partner-Lifecycle-Funktionen werden hier aus `partner_registry.py` importiert und mit re-exportiert.

*Connections & Transaktionen:*
- `readonly_connection()` — Context-Manager, gepoolte Raw-Connection für Reads.
- `transaction(*, isolation_level=…)` — gepoolte Connection mit explizitem commit/rollback.
- `run_transaction(operation, *, isolation_level=…, retries=None)` — Schreib-Transaktion mit Retry bei Deadlock/Serialization.
- `repeatable_read_transaction(operation, *, retries=None)` / `serializable_transaction(operation, *, retries=None)` — wie oben mit fixem Isolation-Level.
- `execute(sql, params=None)` / `query_one(sql, params=None)` / `query_all(sql, params=None)` — direkte One-Shot-Ausführung.

*Bootstrap & Schema:*
- `prepare_runtime_storage()` — expliziter Storage-Bootstrap vor Runtime-Traffic.
- `ensure_schema(conn)` — legt/aktualisiert alle Nicht-Auth-Tabellen idempotent an.
- `ensure_billing_entitlement_schema(conn)` — die gemeinsamen Billing-/Entitlement-Tabellen (Dashboard + Bot).

*DB-Identität (secret-sicher):*
- `analytics_db_fingerprint(dsn=None)` — stabiler, nicht-geheimer Fingerprint der DB.
- `analytics_db_fingerprint_details(dsn=None)` — gehashte Identitätsdetails (log-/health-tauglich).

*Globale Chatter-Bannliste* (siehe [raid.md](raid.md) → Global-Ban-Sweep):
- `is_chatter_globally_banned(chatter_login, chatter_id)` — Treffer per Login **oder** ID.
- `add_chatter_global_ban(chatter_login, chatter_id=None, reason="", added_by="manual")` / `remove_chatter_global_ban(login)` / `list_chatter_global_bans()`.
- `record_global_ban_applied(chatter_login, broadcaster_id)` / `load_applied_global_ban_pairs()` — merkt sich bereits proaktiv ausgeführte (Chatter, Kanal)-Bans.
- `schedule_global_ban_sweep(broadcaster_login, broadcaster_id, delay_seconds)` / `load_due_global_ban_sweeps()` / `delete_global_ban_sweep(broadcaster_login)` — geplante Offline-Sweeps (z. B. 1 h nach Stream-Ende).

*Sonstiges:*
- `load_valid_raid_auth_ids()` — `twitch_user_id`s aller Streamer mit aktivem, gültigem OAuth (`needs_reauth = FALSE`).
- `list_unlinked_streamers()` — nicht-archivierte Streamer ohne Discord-Verknüpfung.
- `insert_observability_event(*, flow_type, flow_id, step, decision, …)` — schreibt ein strukturiertes Observability-Event (Entscheidungs-Trace).
- `backfill_tracked_stats_from_category(conn, login)` — kopiert historische Kategorie-Stats idempotent in Tracked-Stats.
- `delete_streamer(conn, login)` — Streamer + zugehörige Clip-Records löschen (manueller Cascade-Helfer).
- Re-exportierte Partner-/Streamer-Funktionen (Implementierung in `partner_registry.py`): `load_active_partner`, `load_latest_partner_history`, `load_offline_auto_raid_eligibility`, `OfflineAutoRaidEligibility`, `load_partner_by_discord_user_id`, `load_streamer_identity`, `promote_streamer_to_partner`, `reactivate_partner`, `reactivate_partner_after_valid_auth`, `departner_active_partner`, `bulk_update_partner_flags`, `set_partner_live_ping_settings`, `set_partner_raid_bot_enabled`, `set_partner_silent_flags`, `set_streamer_archive_state`, `set_streamer_block_state`, `set_streamer_discord_member`, `save_streamer_discord_profile`, `upsert_non_partner_streamer`, `upsert_streamer_identity`, `verification_payload`.

*Private Helfer-Gruppen* (nicht exportiert): `_ensure_*` (Index-/Bootstrap-Sicherung), `_load_dsn`/`_dsn_conninfo`/`_normalize_conninfo_value` (DSN-Aufbereitung), `_run_startup_maintenance`/`_cleanup_duplicate_*` (Start-Wartung), `_execute_with_savepoint`/`_run_transaction_operation` (Transaktions-Interna), `_advisory_lock_pair` (Advisory-Lock-Schlüssel), `_pg_add_col_if_missing`/`_seed_default_templates_pg` (Migrations-Helfer), `_env_int`/`_env_float`.

### partner_registry.py
Partner-Lebenszyklus + Migration der Legacy-Partnerspalten. Arbeitet auf `twitch_partners`/History und der Partner-View.
- `load_active_partner(conn, *, twitch_login=None, twitch_user_id=None)` — aktive Partner-Zeile laden.
- `load_latest_partner_history(conn, *, …)` — jüngsten History-Eintrag.
- `OfflineAutoRaidEligibility` (Dataclass-artig) mit `can_auto_raid()` + `load_offline_auto_raid_eligibility(conn, *, twitch_user_id)` — darf dieser Partner offline auto-raiden?
- `load_partner_by_discord_user_id(...)` / `load_streamer_identity(...)` — Lookups.
- `promote_streamer_to_partner(...)`, `reactivate_partner(...)`, `reactivate_partner_after_valid_auth(...)`, `departner_active_partner(...)` — Statuswechsel.
- `bulk_update_partner_flags(...)`, `set_partner_live_ping_settings(...)`, `set_partner_raid_bot_enabled(...)`, `set_partner_silent_flags(...)` — Partner-Schalter.
- `set_streamer_archive_state(...)`, `set_streamer_block_state(...)`, `set_streamer_discord_member(...)`, `save_streamer_discord_profile(...)`, `upsert_non_partner_streamer(...)`, `upsert_streamer_identity(...)` — Streamer-Stammdaten.
- `verification_payload(mode)` — baut das Verify-Update-Payload je Modus.
- `migrate_legacy_partner_registry(conn)` / `_streamer_has_partner_columns(conn)` — einmalige Migration weg von den Partner-Spalten in `twitch_streamers` (True, solange die Legacy-Spalten noch existieren).

### _pool.py
- `PostgresConnectionPool(*, dsn, max_size, checkout_timeout, connect_fn, prepare_fn=None)` — prozesslokaler Pool. `connection(*, autocommit)` (Context-Manager), `open_dedicated(*, autocommit)` (dedizierte Verbindung außerhalb des Pools), `close()`. Intern: `_create_connection`, `_ping_connection`, `_acquire_connection`, `_release_connection`, `_discard`.
- `ConnectionPoolRegistry(*, max_size, checkout_timeout, connect_fn, prepare_fn=None)` — `get_pool(dsn)` liefert/erzeugt den Pool je DSN; `close_all()`.
- `ConnectionStats` — einfache Zähler. `_dsn_registry_key(dsn)` erzeugt einen kanonischen, nicht-rohen Registry-Schlüssel (kein DSN-Klartext als Key).

### sessions_db.py
Verschlüsselte Web-Session-Ablage (`dashboard_sessions`).
- Schlüssel: `_load_or_create_key()` holt den Fernet-Key aus keyring (legt ihn beim ersten Mal an), `_get_fernet()`, `_encrypt(payload)` / `_decrypt(data)`.
- `upsert_session(session_id, session_type, payload, created_at, expires_at)` — Session anlegen/erneuern (Payload Fernet-verschlüsselt).
- `delete_session(session_id)` — Logout/Invalidierung.
- `load_valid_sessions(session_type, min_expires_at)` / `load_session(session_id, session_type, min_expires_at)` — nicht-abgelaufene Sessions laden.
- `pop_session(session_id, session_type, min_expires_at)` — **atomar** eine Session/State konsumieren (Advisory-Lock).
- `count_valid_sessions(session_type, min_expires_at, *, session_id_prefix=None)` — aktive Sessions zählen.
- `delete_expired_sessions(now)` — abgelaufene aufräumen.
- `reserve_rate_limit_slot(*, bucket_key, session_type, session_id, payload, created_at, expires_at, max_requests)` — **atomar** einen Rate-Limit-Slot pro Bucket reservieren (gibt `False`, wenn voll).
- Helfer: `_escape_like` (LIKE-Sonderzeichen escapen), `_row_payload`, `_advisory_lock_pair`.

### auto_raid_pause.py
Admin-gesetzte Auto-Raid-Pause pro Kanal.
- `set_auto_raid_pause(conn, *, twitch_user_id, twitch_login, minutes, reason=None, set_by=None)` — Pause setzen/erneuern, gibt neue `paused_until` zurück (Minuten werden geklammert).
- `clear_auto_raid_pause(conn, *, twitch_user_id)` — Pause aufheben (`True`, wenn entfernt).
- `get_auto_raid_pause(conn, *, twitch_user_id)` — **aktive** Pause (`paused_until > now`) inkl. Restzeit, sonst `None`.
- `is_auto_raid_paused(conn, *, twitch_user_id)` — bool-Kurzform.

### promo_cooldowns.py
- `save_promo_cooldown(login, cooldown_type, wall_ts)` — Cooldown-Zeitstempel persistieren (fehlertolerant).
- `load_promo_cooldowns()` — alle als `(login, cooldown_type, ts)`-Tupel.
- `cleanup_stale_promo_cooldowns(max_age_hours=24)` — alte Einträge löschen.

### _rows.py
- `StorageRow(names, values)` — leichter Zeilen-Wrapper, der sowohl wie ein dict (`row["x"]`, `.get`, `.keys`, `.items`) als auch wie eine Sequence (`row[0]`, `len`, Iteration) funktioniert. Sorgt dafür, dass psycopg-Zeilen sich wie die früher genutzten sqlite-Rows verhalten.

## 6. Datenbank & externe Schnittstellen

`ensure_schema()` legt u. a. an (vollständige Spalten in [DATABASE.md](../DATABASE.md)): `twitch_streamers`, `twitch_streamer_identities`, `twitch_stream_sessions`, `twitch_stats_tracked`, `twitch_stats_category`, `twitch_live_state`/`*_viewers`, `twitch_token_blacklist`, `twitch_subscription_events`/`twitch_subscriptions_snapshot`, `twitch_ad_break_events`/`twitch_ads_schedule_snapshot`, `twitch_shoutout_events`, `clip_*` (fetch_history, last_hashtags, templates_global/streamer), `dashboard_sessions`, `oauth_state_tokens`, `social_media_platform_auth`, `streamer_plans`, `discord_invite_codes`, `eventsub_guard_state`, `schema_version`, sowie eine Sicherungstabelle `twitch_streamers_backup_preconsolidation`.

**Views:** `twitch_streamers_partner_state` und `twitch_partners_all_state` — die kanonische Partner-Wahrheit.

**Secret:** DSN via `_load_dsn()` (z. B. `TWITCH_ANALYTICS_DSN`) — wird nie im Klartext geloggt.

## 7. Stolperfallen / Besonderheiten

- **Pools sind prozesslokal:** `ConnectionPoolRegistry` lebt pro Prozess. BotRuntime und DashboardRuntime haben **getrennte** Pools auf dieselbe DB — die Koordination passiert in der DB (Locks, Constraints), nicht im Speicher.
- **Atomarität über Advisory-Locks:** `pop_session` und `reserve_rate_limit_slot` nutzen `_advisory_lock_pair` für race-freie Konsum-/Reservierungs-Operationen. Nicht durch naive SELECT-then-UPDATE ersetzen.
- **Session-Verschlüsselung braucht keyring:** Der Fernet-Key liegt im keyring (`_load_or_create_key`). Ohne keyring/Key sind bestehende Sessions nicht entschlüsselbar — Login-Verlust, kein Datenleck.
- **Partner-Wahrheit nur über die View:** Code, der Rohspalten von `twitch_streamers` für Partner-Status liest, ist veraltet (Konsolidierung abgeschlossen). Immer die View `twitch_streamers_partner_state` bzw. die `partner_registry`-Funktionen nutzen.
- **Idempotenz ist Pflicht:** `ensure_schema` und die `*_migrate`/`_pg_add_col_if_missing`-Helfer müssen mehrfach ausführbar bleiben — beim Start laufen sie jedes Mal.
- **DSN niemals loggen:** Für Diagnose `analytics_db_fingerprint[_details]()` verwenden, nie die rohe DSN — der Registry-Key ist bewusst „non-raw“.
- **Schreib-Retries sind begrenzt:** `run_transaction(retries=…)` fängt Deadlocks/Serialization ab, aber nicht beliebig — lang laufende Schreibpfade sollten kurz und idempotent sein.
