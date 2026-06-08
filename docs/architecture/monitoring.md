# monitoring/ — Architektur & Funktionsreferenz

> Pfad: `bot/monitoring/` · Stand: 2026-06-08 · 13 Dateien, ~11.360 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [api.md](api.md) (Helix/EventSub-Calls), [storage.md](storage.md) (Live-State, Sessions), [raid.md](raid.md) (Raid-Score-Refresh, channel.raid), [live-announce.md](live-announce.md), [SESSION_LIFECYCLE.md](../SESSION_LIFECYCLE.md).

## 1. Zweck & Abgrenzung

`monitoring/` ist das **Daten-Ingestion-Herz**: Es weiß zu jedem Zeitpunkt, **wer live ist**, schreibt Stream-Sessions + Statistiken in die DB und postet **Go-Live-Ankündigungen** auf Discord. Zwei Erkennungswege ergänzen sich:

1. **Polling** (`monitoring.py`): alle 15 s (`POLL_INTERVAL_SECONDS`) ein Helix-Abgleich der getrackten Streamer + der Deadlock-Kategorie → Live-State/Stats/Sessions aktualisieren, Postings auslösen.
2. **EventSub** (push, `eventsub_*`): Twitch schickt Events (`stream.online/offline`, `channel.update`, `channel.raid`, Subs/Ad-Break/Shoutout) über **WebSocket** und/oder **Webhook**; das ist schneller und genauer als Polling, besonders für `stream.offline` und Raids.

Abgrenzung: `monitoring/` **erkennt** und **persistiert** — die fachliche Auswertung liegt in [analytics.md](analytics.md), das Raid-Verhalten in [raid.md](raid.md). Die eigentlichen HTTP-Calls macht [api.md](api.md).

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `TwitchStreamCog` (als `TwitchMonitoringMixin`); löst Raid-Score-Refreshs (`raid/`) und Live-Ankündigungen aus. |
| **Nutzt** | `api/` (Helix-Streams, EventSub-Subscriptions), `storage/` (Live-State, Sessions, Stats), `core/` (Partner-Gate, Konstanten), Discord (Embeds/Rollen), den **Master-Broker** (Port 8770) als alternativen Announcement-Transport. |
| **DB-Tabellen** | `twitch_live_state`(+`_viewers`), `twitch_stream_sessions`, `twitch_stats_tracked`, `twitch_stats_category`, `exp_sessions`/`exp_snapshots`/`exp_game_transitions`, `eventsub_guard_state` + die EventSub-Processing-Inbox. |
| **Externe Dienste** | Twitch-EventSub (WSS + Webhook), Twitch-Helix, Discord. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `eventsub_mixin.py` | 3414 | `_EventSubMixin` — Kapazität + Listener-Orchestrierung über beide Transporte, dynamische Raid-Subscriptions. |
| `monitoring.py` | 2155 | `TwitchMonitoringMixin` — Polling-Loop, Live-State-Persistenz, Auto-Archiv, Announcement-Transport. |
| `embeds_mixin.py` | 1157 | `_EmbedsMixin` — Go-Live-/Offline-Embeds, Live-Ping-Rolle, Tracking-Button-Views. |
| `sessions_mixin.py` | 1141 | `_SessionsMixin` — Stream-Session-Lebenszyklus (Start/Sample/Finalize). |
| `eventsub_ws.py` | 808 | `EventSubWSListener` — ein WebSocket-Client mit Reconnect + Message-Dedup. |
| `eventsub_webhook.py` | 758 | Webhook-Handler für eingehende EventSub-Requests (HMAC-Verify, Challenge). |
| `eventsub_processing_inbox.py` | 570 | `EventSubProcessingInboxStore` — durable Leased-Work-Queue für asynchrone Verarbeitung. |
| `eventsub_ws_pool.py` | 389 | `EventSubWSListenerPool` — verteilt Subscriptions auf bis zu 3 WS-Transporte. |
| `exp_sessions_mixin.py` | 284 | `_ExpSessionsMixin` — parallele Session-Logik fürs Experimental-Analytics. |
| `partner_ops.py` | 260 | Helfer zum (Neu-)Berechnen der Partner-Raid-Scores. |
| `eventsub_state_store.py` | 220 | `EventSubStateStore` — persistenter Guard-Store (Dedup/Throttle) über Transporte hinweg. |
| `eventsub_core_callbacks.py` | 202 | Gemeinsame Callback-Registrierung für WS + Webhook. |

## 4. Datenfluss / Lebenszyklus

**Polling-Tick (alle 15 s):** `poll_streams` (discord.py `tasks.loop`) → `_tick()`:
1. `_load_tracked_streamers()` lädt alle zu überwachenden Kanäle.
2. Helix liefert Live-Streams (getrackt per Logins + Deadlock-Kategorie-Sample bis `TWITCH_CATEGORY_SAMPLE_LIMIT`); `_stream_is_in_target_category` + `_language_filter_values` filtern.
3. `_load_live_state_snapshot` + `_persist_live_state_rows` schreiben den neuen Live-State (Batch).
4. `_process_postings` postet Go-Live für frisch live gegangene Streamer; `_log_stats` schreibt Stats-Samples.
5. `_ensure_stream_session`/`_record_session_sample` pflegen die Session; bei Offline `_finalize_stream_session`.
6. `_auto_archive_inactive_streamers(days=10)` archiviert Partner, die >10 Tage nicht live waren.

**EventSub (push):** Beim Start registriert `_EventSubMixin` die Core-Subscriptions (`eventsub_core_callbacks`) über den WS-Pool und/oder Webhook. Eingehende Notifications werden **dedupliziert** (Message-ID-Guard im `EventSubStateStore`) und in die **Processing-Inbox** gelegt; ein Hintergrund-Worker (`_process_due_batch`) leaset fällige Einträge, verarbeitet sie und retryt mit Backoff (bis `_INBOX_MAX_ATTEMPTS`). So ist Empfang von Verarbeitung entkoppelt — ein langsamer Handler blockiert nicht den WS.

**Transport-Wahl der Ankündigung:** `_announcement_transport_ready` entscheidet pro Posting, ob direkt über Discord oder über den **Master-Broker** (`_master_broker_base_url`, Token-Fallback) gesendet wird; `_build_announcement_idempotency_key` verhindert Doppel-Postings.

**Raid-Targets dynamisch:** Vor einem Raid stellt `ensure_raid_target_dynamic_ready` sicher, dass für das Ziel eine `channel.raid`-Subscription existiert (`subscribe_raid_target_dynamic`), damit der Raid-Erfolg gemessen werden kann.

## 5. Funktionsreferenz pro Datei

### monitoring.py — `TwitchMonitoringMixin(_EventSubMixin, _ExpSessionsMixin, _SessionsMixin, _EmbedsMixin)`
*Loop & Tick:*
- `poll_streams()` / `_before_poll()` — die Haupt-`tasks.loop` (15 s) + Vorbereitung.
- `_tick()` — ein kompletter Durchlauf: tracked + Kategorie-Streams prüfen, Postings/DB aktualisieren.
- `_process_postings(tracked, streams_by_login)` — Go-Live-Postings auslösen.
- `_load_tracked_streamers()` / `_load_tracked_streamers_async()` — alle zu überwachenden Kanäle laden.
- `_load_live_state_snapshot(tracked)` / `_persist_live_state_rows(rows)` — Live-State lesen/batch-schreiben.
- `_auto_archive_inactive_streamers(*, days=10)` — inaktive Partner automatisch archivieren.
- `invites_refresh()` / `_before_invites()` — periodischer Discord-Invite-Refresh.
- `_ensure_category_id()` — Deadlock-`game_id` cachen.

*Filter & Meta:* `_get_target_game_lower()`, `_stream_is_in_target_category(stream)`, `_normalize_stream_meta(stream)`, `_language_filter_values()`, `_normalize_tracking_login(value)`, `_chunk_values(values, *, chunk_size=200)`.

*Storage-Resilienz:* `_run_storage_write_with_retry(writer, *, failure_message, max_attempts=3, retry_delay=0.5)`, `_is_retryable_storage_error`, `_summarize_storage_error`, `_log_storage_write_failure`, `_executemany`, `_storage_error_details`.

*Announcement-Transport:* `_announcement_transport_prefers_master_broker()`, `_announcement_transport_can_use_direct_discord()`, `_announcement_transport_ready(*, channel_id, notify_channel)`, `_build_announcement_idempotency_key(*, action, login, discriminator)`, `_master_broker_base_url()`, `_master_broker_token()`, `_normalize_master_broker_base_url`, `_is_loopback_host`.

### sessions_mixin.py — `_SessionsMixin`
- `_ensure_stream_session(*, login, stream, previous_state, twitch_user_id)` — sorgt für eine offene Session (legt an oder findet die bestehende).
- `_start_stream_session(*, login, stream, started_at_iso, twitch_user_id, followers_start, title="", language="", is_mature=False, tags="")` — neue Session anlegen.
- `_record_session_sample(*, login, stream)` — Viewer-/Meta-Snapshot in die Session schreiben.
- `_finalize_stream_session(*, login, reason="done", session_id=None, ended_at=None)` — Session abschließen (Offline/Cleanup).
- `_adopt_incomplete_session(session_id, stream)` — eine vom Scout unvollständig angelegte Session nachfüllen.
- `_cleanup_orphaned_sessions()` — verwaiste offene Sessions schließen.
- `_rehydrate_active_sessions()` / `_get_active_session_id(login)` / `_lookup_open_session_id(login)` / `_get_active_sessions_cache()` — In-Memory-Cache der offenen Sessions.
- `_fetch_followers_total_safe(*, twitch_user_id, login, stream)` — Follower-Zahl best-effort (mit Fallback-Warnung pro Login).
- `_extract_stream_start(stream, previous_state)`, `_parse_dt(value)`, `_log_stats(streams_by_login, category_streams)`, `_get_latest_vod_preview_url(*, login, twitch_user_id)`.
- Observability: `_next_analytics_observability_flow_id`, `_increment_analytics_observability_counter`, `_log_analytics_decision`, `_build_analytics_runtime_state`, `_scope_presence_state`, `_structured_result_meta`.

### exp_sessions_mixin.py — `_ExpSessionsMixin`
Additive Hooks fürs Experimental-Analytics-System (`exp_*`-Tabellen):
- `_exp_on_session_start(*, login, stream, started_at_iso) -> int | None` — neuen `exp_sessions`-Eintrag anlegen.
- `_exp_on_session_sample(*, login, exp_session_id, stream)` — Snapshot in `exp_snapshots`.
- `_exp_on_game_transition(*, login, exp_session_id, from_game, to_game, viewer_count)` — Spielwechsel in `exp_game_transitions`.
- `_exp_on_session_finalize(*, login, exp_session_id, follower_delta, now_dt=None)` — Session abschließen.
- Cache: `_get_exp_session_id`/`_set_exp_session_id`/`_clear_exp_session_id`/`_get_exp_sessions_cache`.

### embeds_mixin.py — `_EmbedsMixin`
- `_build_live_embed(login, stream, *, rendered_payload=None, cache_buster_seed=None, render_now=None) -> discord.Embed` — Go-Live-Embed mit Stream-Vorschau.
- `_build_offline_embed(*, login, display_name, last_title, last_game, preview_image_url) -> discord.Embed` — Offline-/VOD-Overlay im gleichen Stil.
- `_render_live_announcement_payload(...)` / `_build_live_announce_context(...)` — Embed-Payload + Kontext rendern.
- `_ensure_live_ping_role(*, login, streamer_entry=None, notify_channel=None)` — pro Streamer eine Live-Ping-Rolle sicherstellen/anlegen.
- `_load_live_announcement_config(login)` / `_normalize_live_announcement_config(config)` / `_default_live_announcement_config()` — Announcement-Konfig je Streamer.
- `_build_offline_link_view(referral_url, *, label=None)` / `_resolve_live_button_label(login)` — Buttons.
- Persistente Views: `_TwitchLiveAnnouncementView` (trackt Klicks vor Redirect), `_TrackedTwitchButton` (`callback` loggt den Klick), `_TwitchReferralLinkView` (einfacher Link); plus Registry `_get_live_view_registry`/`_register_live_view`/`_drop_live_view`, `_log_link_click`, `_handle_tracked_button_click`.

### eventsub_mixin.py — `_EventSubMixin`
EventSub-Kapazität + Listener-Management. Konstanten u. a. `_EVENTSUB_WEBHOOK_REQUIRED_SUB_TYPES`, `_EVENTSUB_WEBHOOK_CORE_SUB_TYPES`, Retry-Delays.
- `_get_eventsub_processing_inbox()` / `_ensure_eventsub_processing_inbox_started()` — durable Inbox starten.
- `ensure_raid_target_dynamic_ready(broadcaster_id, broadcaster_login, *, raid_flow_id=None, wait_timeout_seconds=8.0, poll_interval_seconds=0.5)` — wartet, bis eine `channel.raid`-Subscription fürs Ziel bereit ist.
- `subscribe_raid_target_dynamic(broadcaster_id, broadcaster_login)` — dynamische `channel.raid`-Subscription anlegen.
- `_eventsub_has_sub`/`_eventsub_untrack_sub`/`_eventsub_subscription_matches` — Tracking der aktiven Subscriptions.
- `_get_eventsub_webhook_subscription_status(...)` — Webhook-Subscription-Status abfragen.
- `_cleanup_old_eventsub_subscriptions(webhook_url, *, active_target_user_ids=None)` — verwaiste Subscriptions abräumen.
- `_collect_eventsub_capacity_snapshot`/`_record_eventsub_capacity_snapshot`, `_spawn_eventsub_task`, `_handle_eventsub_background_processing_failure`, `_process_eventsub_processing_record`.

### eventsub_ws.py — `EventSubWSListener`
Ein konsolidierter WebSocket-Client. Konstanten `MAX_SUBSCRIPTIONS_PER_TRANSPORT`, `_MAX_MESSAGE_AGE_SECONDS`.
- `run()` — Listener starten + Reconnects behandeln; `_run_once(is_reconnect=False)`, `_wait_for_welcome(ws)`, `_resolve_token()`, `_register_all_subscriptions(session_id)`, `_handle_message(data)`.
- `add_subscription_dynamic(...)` — Subscription im laufenden Betrieb hinzufügen; `set_callback(sub_type, callback)`.
- Dedup: `_cleanup_expired_message_ids(now=None)`, `_callback_accepts_message_id(callback)`.
- Exceptions: `EventSubReconnect` (Twitch verlangt Reconnect), `EventSubTransportSessionInvalid`.

### eventsub_ws_pool.py — `EventSubWSListenerPool`
Verteilt Subscriptions auf bis zu `MAX_WEBSOCKET_TRANSPORTS` (3) WS-Transporte (Fallback-Modus).
- `run()`, `wait_until_ready(timeout=8.0, ...)`, `wait_until_initial_registration(...)`.
- `add_subscription(...)` / `add_subscription_dynamic(...)`, `has_registered_subscription(...)`, `is_subscription_ready(...)`, `get_tracked_subscriptions()`, `get_capacity_rows()`, `has_capacity()`.
- `_active_listeners()`, `_finalize_completed_listener_tasks()`, `_start_listener_task(listener)`.

### eventsub_webhook.py
- `handle_request(request) -> web.Response` — Haupt-Handler für eingehende EventSub-Webhook-Requests (Signatur-Verify + Challenge-Handshake + Notification).
- `dispatch_notification_internal_async(data, sub_type, *, message_id="")` — interne Dispatch-Variante (await), honoriert synchrone Zustellung.
- `_dispatch_notification(data, sub_type, *, message_id="")` — Notification verarbeiten und passenden Callback rufen.
- `_track_message_id`/`_forget_message_id` — Message-ID-Dedup.

### eventsub_processing_inbox.py — `EventSubProcessingInboxStore`
Durable Queue. Konstanten: `_INBOX_BATCH_SIZE`, `_INBOX_LEASE_SECONDS`, `_INBOX_IDLE_WAIT_SECONDS`, `_INBOX_RETRY_BASE_SECONDS`, `_INBOX_RETRY_MAX_SECONDS`, `_INBOX_MAX_ATTEMPTS`.
- `ensure_initialized()`, `enqueue(*, work_type, payload, message_id, now)`, `lease_due(*, now, lease_seconds, limit)`, `mark_delivered(...)`, `_process_due_batch()`, `_retry_delay_seconds(attempts)`.

### eventsub_state_store.py — `EventSubStateStore`
Persistenter Guard-Store über Transporte hinweg (Tabelle `eventsub_guard_state`). Kinds: `MESSAGE_ID`, `WS_MESSAGE_ID`, `OFFLINE_THROTTLE`, `BUSINESS_EFFECT`.
- `is_active(kind, key, *, now)`, `claim(kind, key, *, ttl_seconds, now)`, `release(kind, key)`, `_normalize(kind, key)`.
- Protokoll `EventSubStateRepository` + Postgres-Implementierung `_PostgresEventSubStateRepository`.

### eventsub_core_callbacks.py
- `register_core_eventsub_callbacks(owner, handler, *, logger=None, propagate_callback_errors=False, delivery_mode="inline")` — registriert die Core-Event-Callbacks auf beiden Transporten.
- `is_core_eventsub_delivery_type(sub_type)` + Konstante `EVENTSUB_CORE_DELIVERY_TYPES`; Protokoll `EventSubCallbackSink`.

### partner_ops.py
Funktionen (kein Mixin) zum (Neu-)Berechnen der Partner-Raid-Scores, vom Monitoring getriggert:
- `request_partner_raid_score_refresh(host, *, twitch_user_id=None, login=None, trigger, full_refresh=False)` / `schedule_partner_raid_score_refresh(...)` / `schedule_partner_raid_score_refreshes(host, refreshes)` — Refresh anstoßen.
- `run_partner_raid_score_refresh_task(host, *, task_key, …)` — die eigentliche Hintergrund-Task.
- `maybe_schedule_partner_raid_score_reconciliation(host, *, trigger)` — periodischer Abgleich.
- `partner_raid_score_refresh_interval_seconds`, `partner_raid_score_refresh_preferred_names`, `partner_raid_score_refresh_candidates`, `build_partner_raid_score_refresh_kwargs`.

## 6. Datenbank & externe Schnittstellen

- **DB:** `twitch_live_state`(+`_viewers`), `twitch_stream_sessions`, `twitch_stats_tracked`, `twitch_stats_category`, `exp_sessions`/`exp_snapshots`/`exp_game_transitions`, `eventsub_guard_state`, EventSub-Inbox-Tabelle.
- **Twitch:** EventSub via WebSocket (`wss://…`) **und** Webhook (HMAC-signiert), Helix-Streams (über `api/`).
- **Discord:** Go-Live-/Offline-Embeds, Live-Ping-Rolle, Tracking-Buttons.
- **Master-Broker:** Port 8770 als alternativer Sende-Transport für Ankündigungen (Token-Fallback-Kette).

## 7. Stolperfallen / Besonderheiten

- **Zwei Transporte, eine Wahrheit:** WS-Pool und Webhook können dieselbe Notification liefern. Der `EventSubStateStore` (Message-ID-Guard, `claim`/`is_active`) verhindert Doppelverarbeitung **über Transporte hinweg** — nicht pro Listener lösen.
- **Empfang ≠ Verarbeitung:** Notifications landen erst in der durable Inbox und werden geleased verarbeitet. Ein abstürzender Handler verliert nichts (Retry bis `_INBOX_MAX_ATTEMPTS`), blockiert aber auch nicht den WS-Empfang.
- **WS-Kapazität ist begrenzt:** je Transport `MAX_SUBSCRIPTIONS_PER_TRANSPORT`; der Pool fächert auf bis zu 3 Transporte auf. Wer viele Kanäle trackt, muss die Kapazität (`get_capacity_rows`) im Blick behalten — Webhook ist für die Pflicht-Sub-Typen vorgesehen.
- **Offline-Throttle:** Der `OFFLINE_THROTTLE`-Guard verhindert, dass kurzzeitige Offline/Online-Flapping doppelte Session-Finalisierungen/Postings auslöst.
- **Polling bleibt Fallback:** Auch mit EventSub läuft das 15-s-Polling weiter (Kategorie-Sampling, Stats, Selbstheilung bei verpassten Events). EventSub ist die schnelle Spur, nicht der einzige Weg.
- **Announcement-Idempotenz:** Ohne `_build_announcement_idempotency_key` würden Master-Broker- und Direct-Discord-Pfad bei Race doppelt posten — der Key ist die Absicherung.
- **Auto-Archiv greift nach 10 Tagen Inaktivität** (`_auto_archive_inactive_streamers`) — Partner verschwinden also „von selbst“ aus der aktiven Liste, das ist gewollt.
