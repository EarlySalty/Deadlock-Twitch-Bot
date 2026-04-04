# Datenbank-Dokumentation

Alle Tabellen in PostgreSQL (DSN via ENV `TWITCH_ANALYTICS_DSN` oder Windows Keyring `DeadlockBot`).
Runtime-Bootstrap läuft über `bot/storage/pg.py` → `prepare_runtime_storage()`.
Beim Erststart wird das Schema darüber initialisiert; danach wird der Stand in `schema_version`
für die Runtime-Komponente `storage_pg` markiert, damit Folgestarts nicht erneut den kompletten
DDL-Pfad durchlaufen. Ein extern verwaltetes `schema_version`-Schema bleibt kompatibel.

## Kern-Tabellen

### twitch_streamers
Partner-Registry — welche Streamer sind Teil des Systems.

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| twitch_login | TEXT PK | Twitch-Login-Name |
| twitch_user_id | TEXT UNIQUE | Twitch User-ID |
| discord_user_id | TEXT | Verknuepfter Discord-Account |
| discord_display_name | TEXT | Discord-Anzeigename |
| is_on_discord | INTEGER | 0/1 ob auf dem Server |
| require_discord_link | INTEGER | 0/1 ob Discord-Verknuepfung Pflicht |
| manual_verified_permanent | INTEGER | Permanent verifiziert (Admin) |
| manual_verified_until | TEXT | Zeitlich begrenzte Verifikation |
| manual_verified_at | TEXT | Zeitpunkt der Verifikation |
| manual_partner_opt_out | INTEGER | Streamer hat Opt-Out gewaehlt |
| is_monitored_only | INTEGER | Nur monitoring, kein Partner-Status |
| raid_bot_enabled | INTEGER | Raid-Bot fuer diesen Streamer aktiv |
| silent_ban | INTEGER | Gebannt ohne Benachrichtigung |
| silent_raid | INTEGER | Raids ohne Ankuendigung |
| archived_at | TEXT | Archivierungszeitpunkt |
| live_ping_role_id | BIGINT | Discord-Rolle fuer Live-Ping |
| live_ping_enabled | INTEGER | Live-Ping ein/aus |
| created_at | TEXT | Erstellungszeitpunkt |

**View**: `twitch_streamers_partner_state` — berechnet `is_verified`, `is_partner`, `is_partner_active`.

**Schreibt**: `bot/community/admin.py`, `bot/dashboard/routes_mixin.py`
**Liest**: Fast alle Module

---

### twitch_live_state
Aktueller Live-Status pro Streamer (1 Zeile pro Streamer).

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| twitch_user_id | TEXT PK | Twitch User-ID |
| streamer_login | TEXT | Login-Name |
| is_live | INTEGER | Aktuell live? |
| last_stream_id | TEXT | Stream-ID des letzten Streams |
| last_started_at | TEXT | Stream-Start |
| last_title | TEXT | Stream-Titel |
| last_game | TEXT | Aktuelles Spiel |
| last_game_id | TEXT | Spiel-ID |
| last_viewer_count | INTEGER | Letzter Viewer-Count |
| last_discord_message_id | TEXT | Discord-Go-Live-Embed ID |
| last_notified_at | TEXT | Letzter Go-Live-Post |
| last_seen_at | TEXT | Letzter Tick wo Streamer live war |
| active_session_id | INTEGER | FK auf twitch_stream_sessions |
| had_deadlock_in_session | INTEGER | Deadlock gespielt in dieser Session |
| last_deadlock_seen_at | TEXT | Letzter Deadlock-Zeitpunkt |
| last_tracking_token | TEXT | Anti-Doppel-Post-Token |

**Schreibt**: `bot/monitoring/monitoring.py`

---

### twitch_stats_tracked
Viewer-Snapshots der ueberwachten Partner (Default: jeder Poll-Tick, also 15s).

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| ts_utc | TEXT | Zeitstempel |
| streamer | TEXT | Twitch-Login |
| viewer_count | INTEGER | Viewer zum Zeitpunkt |
| is_partner | INTEGER | Partner-Status zum Zeitpunkt |
| game_name | TEXT | Gespieltes Spiel |
| stream_title | TEXT | Stream-Titel |
| tags | TEXT | Tags (JSON-Array) |

**Index**: `idx_twitch_stats_tracked_ts` (ts_utc), `idx_twitch_stats_tracked_streamer` (streamer)
**Schreibt**: `bot/monitoring/monitoring.py`
**Liest**: `bot/analytics/backend.py`, `bot/analytics/backend_extended.py`

---

### twitch_stats_category
Alle Deadlock-Kategorie-Streamer-Snapshots (Default: jeder Poll-Tick, also 15s).

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| ts_utc | TEXT | Zeitstempel |
| streamer | TEXT | Twitch-Login |
| viewer_count | INTEGER | Viewer zum Zeitpunkt |
| language | TEXT | Sprache des Streams |
| game_name | TEXT | Gespieltes Spiel |
| stream_title | TEXT | Stream-Titel |
| tags | TEXT | Tags |

**Schreibt**: `bot/monitoring/monitoring.py` (TWITCH_CATEGORY_SAMPLE_LIMIT=400 pro Tick)

---

## Stream-Sessions

### twitch_stream_sessions
Aggregierte Session-Daten (1 Zeile pro Stream-Session).

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| id | SERIAL PK | Session-ID |
| streamer_login | TEXT | Twitch-Login |
| twitch_user_id | TEXT | Twitch User-ID |
| stream_id | TEXT | Twitch Stream-ID |
| started_at | TIMESTAMPTZ | Session-Start |
| ended_at | TIMESTAMPTZ | Session-Ende |
| peak_viewers | INTEGER | Peak-Viewer |
| avg_viewers | REAL | Durchschnitt Viewer |
| duration_seconds | INTEGER | Stream-Dauer |
| game_name | TEXT | Hauptspiel |
| title | TEXT | Stream-Titel |
| tags | TEXT | Tags |

### twitch_session_viewers / twitch_session_chatters
Viewer/Chatter pro Session (wird aus EventSub-Daten befuellt).

### twitch_viewer_presence_ticks
Granulares Presence-Tracking: ein Row pro Viewer pro Poll-Zyklus (alle 30 Sek). Basis fuer die Viewer-Timeline im Dashboard. TimescaleDB-Hypertable, Kompression nach 3 Tagen.

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| `session_id` | BIGINT PK/FK | Referenz auf `twitch_stream_sessions.id` |
| `streamer_login` | TEXT | Twitch-Login des Streamers |
| `viewer_login` | TEXT | Twitch-Login des Viewers |
| `tick_at` | TIMESTAMPTZ PK | Zeitstempel des Chatters-Polls |

Aus den Ticks werden zur Abfragezeit per Window Function zusammenhaengende Anwesenheits-Spans berechnet (Gap-Schwelle: >2 min). Siehe [docs/VIEWER_PRESENCE_TIMELINE.md](VIEWER_PRESENCE_TIMELINE.md).

**Schreibt:** `bot/analytics/mixin.py` → `collect_chatters_data()`

### twitch_chatter_rollup
Aggregierte Chatter-Statistiken (Aktivitaet, Loyalty-Metriken).

### twitch_chat_messages
Einzelne Chat-Nachrichten (fuer Chat-Analyse, Deep-Analytics).

---

## EventSub-Ereignisse

Alle Tabellen haben `streamer_login`, `twitch_user_id`, `occurred_at` und event-spezifische Felder:

| Tabelle | Event-Typ |
|---------|-----------|
| `twitch_bits_events` | Bits/Cheers |
| `twitch_hype_train_events` | Hype Train Start/Progress/Ende |
| `twitch_subscription_events` | Abos (neu, geschenkt, verlängert) |
| `twitch_channel_updates` | Titel-/Spiel-Wechsel |
| `twitch_ad_break_events` | Ad-Breaks |
| `twitch_ban_events` | Bans/Timeouts |
| `twitch_shoutout_events` | Shoutouts (gegeben/erhalten) |
| `twitch_follow_events` | Follows |
| `twitch_channel_points_events` | Channel-Points-Einloesungen |

**Schreibt**: `bot/monitoring/eventsub_ws.py`, `bot/monitoring/eventsub_webhook.py`

---

## Raid-Tabellen

### twitch_raid_history
Log aller ausgefuehrten Raids.

### twitch_raid_blacklist
Streamer die nicht geraided werden sollen.

### twitch_raid_retention
Viewer-Retention nach Raids (wie viele bleiben beim Raid-Ziel).

### twitch_raid_auth
OAuth-Tokens fuer Raid-Bot (verschluesselt mit AES-256-GCM).

| Spalte | Typ | Beschreibung |
|--------|-----|--------------|
| streamer_login | TEXT PK | Twitch-Login |
| access_token_enc | BYTEA | Verschluesselter Access-Token |
| refresh_token_enc | BYTEA | Verschluesselter Refresh-Token |
| scopes_enc | BYTEA | Verschluesselte Scopes |
| saved_at | TIMESTAMPTZ | Speicherzeitpunkt |

**Hinweis**: Legacy-Klartext-Spalten (`legacy_*`) sollen per `bot/migrations/drop_legacy_tokens.py` entfernt werden (Phase 3).

---

## Auth / Sessions

### oauth_state_tokens
Ephemaere OAuth-State-Tokens (10min TTL, Klartext OK weil kurzlebig).

### twitch_token_blacklist
Widerrufene/abgelaufene Auth-Tokens.

---

## Social Media

### twitch_clips_social_media
Clips die fuer Social-Media vorgesehen sind (Metadaten, Status).

### twitch_clips_social_analytics
Upload-Ergebnisse und Performance-Daten pro Clip/Platform.

### twitch_clips_upload_queue
Upload-Warteschlange mit Status (pending/processing/done/failed).

### social_media_platform_auth
OAuth-Tokens fuer Social-Media-Plattformen (AES-256-GCM verschluesselt).

### clip_templates_global / clip_templates_streamer
Clip-Templates (global und pro Streamer) fuer automatische Titel/Beschreibung.

### clip_fetch_history
Log wann Clips zuletzt von der Twitch API geholt wurden.

### clip_last_hashtags
Zuletzt verwendete Hashtags pro Streamer/Plattform.

---

## Billing / Abo

### streamer_plans
Aktueller Abo-Plan pro Streamer (welcher Plan, wann, Stripe-IDs).

Wichtige Feature-Flags in dieser Tabelle:

- `promo_disabled`
- `promo_message`
- `lurker_tax_enabled`

---

## Community / Discord

### discord_invite_codes
Discord-Invite-Links mit Tracking (wer hat wen eingeladen).

### twitch_streamer_invites
Welcher Streamer wurde mit welchem Invite eingeladen.

### twitch_partner_outreach
Outreach-Log fuer Partner-Ansprache.

---

## Monitoring-Snapshots

| Tabelle | Inhalt |
|---------|--------|
| `twitch_subscriptions_snapshot` | Abo-Snapshots (welche EventSub-Subs sind aktiv) |
| `twitch_eventsub_capacity_snapshot` | EventSub-Kapazitaets-Nutzung |
| `twitch_ads_schedule_snapshot` | Geplante Ad-Breaks |
| `twitch_link_clicks` | Klicks auf Affiliate/Dashboard-Links |
| `twitch_live_announcement_configs` | Go-Live-Announcement-Konfiguration pro Streamer |
| `twitch_global_promo_modes` | Globale Promo-Mode-Einstellungen |

---

## Indexes (bekannte Performance-Indexes)

- `idx_twitch_stats_tracked_ts` — twitch_stats_tracked(ts_utc)
- `idx_twitch_stats_category_streamer` — twitch_stats_category(streamer)
- `idx_twitch_stats_category_ts` — twitch_stats_category(ts_utc)
- `idx_twitch_streamers_user_id` — twitch_streamers(twitch_user_id) UNIQUE
- `idx_dashboard_sessions_expires` — dashboard_sessions(expires_at) [geplant Phase 3]
