# Research: Audience-Überblick bei 365 Tagen (Tempo)

status: aktiv
datum: 2026-09-06
contract: CONTRACT.md

Read-only-Messung durch Explore-Agent gegen Live-DB und Journal von `deadlock-twitch-dashboard-rust.service`, Stand origin/main 542e6c17.

## Ergebnis

Die 20 Sekunden kommen nicht aus SQL. Alle fünf Audience-Endpunkte messen zusammen unter 500 ms. Die Wartezeit entsteht im Auth-Extractor: jeder Request ruft synchron den Discord-OAuth-Broker auf `http://127.0.0.1:8766/internal/twitch/v1/discord/validate-session` mit 20-s-Timeout und ohne Cache. Antwortet der Broker nicht, blockiert jeder Request volle 20 s.

Beweis (Journal, 2026-09-06 UTC):

```
10:58:24.255  HTTP Request gestartet  path=/twitch/api/v2/watch-time-distribution   (+4 weitere)
10:58:44.256  WARN auth::discord_admin_login: Discord-OAuth-Broker request failed   (4x)
10:58:47.919  HTTP Request abgeschlossen status=200 latency_ms=23658 ... 24451
```

Über 7 Tage: 43 Audience-Requests, Median 30 ms, p95 1688 ms, genau ein Burst mit 23,6 bis 24,5 s, deckungsgleich mit den vier Broker-Timeouts.

## Fundstellen

| Endpunkt | Handler | SQL | Dauer ms | Ursache |
|---|---|---|---|---|
| watch-time-distribution | rust/crates/tb-dashboard-api/src/handlers/watch_time.rs:84, rust/crates/tb-analytics/src/watch_time.rs:254 | Backfill-UPDATE watch_time.rs:107, Count :167, Minuten :179 | 39 / 7 / 6 | Schreibvorgang im Lesepfad (0 Zeilen heute) |
| follower-funnel | rust/crates/tb-dashboard-api/src/handlers/follower_funnel.rs:73 | :73, :132, :167, :188 | 2 / 6 / 13 / 20 | unauffällig |
| audience-demographics | rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs:210 | :322 (Q5), :473 (Q7), :289 (Q3b) | 192 / 31 (Planning 104) / 22 | Q5 ohne `sv.ts_utc`-Prädikat (240 Timescale-Chunks); Q7 `LOWER(cm.streamer_login)` ohne Funktionsindex |
| lurker-analysis | rust/crates/tb-dashboard-api/src/handlers/lurker_analysis.rs:55, :140 | | 5 | unauffällig |
| viewer-profiles | rust/crates/tb-dashboard-api/src/handlers/audience.rs:202 | :225, :247 | 13 / 2 | unauffällig |
| Auth-Vorstufe aller fünf | rust/crates/tb-dashboard-api/src/auth/level.rs:374 | kein SQL | 20000 (Timeout) | `validate_session` je Request und je `master_dash_session`-Cookie, `BROKER_TIMEOUT = 20s` in rust/crates/tb-dashboard-api/src/auth/discord_admin_login.rs:50, kein Cache |

Coaching: der Audience-Überblick lädt `/coaching` nicht (bot/dashboard_v2/src/pages/Publikum.tsx:20, SubTabs.tsx:56 rendert nur den aktiven Unter-Reiter, kein prefetch). `schedule_optimizer` (rust/crates/tb-analytics/src/coaching.rs:451) ist mit 29,9 s belegt, läuft aber nur bei Planning und Was-tun.

Tabellen: `twitch_stats_category` 9,35 Mio Zeilen (45 MB, 48 Chunks); `twitch_session_viewers` Hypertable 240 Chunks, Partitionsschlüssel `ts_utc`; `twitch_chat_messages` 197 Chunks, Index `(streamer_login, message_ts)` vorhanden, wegen `LOWER()` nicht nutzbar. DB ist TimescaleDB.

## Fixes

1. Broker-Validierung cachen und Timeout senken (löst das Symptom vollständig): Ergebnis von `validate_session` in den bestehenden `TimedCache` (rust/crates/tb-dashboard-api/src/auth/session.rs:433-451, `CACHE_TTL_SECS = 5` in :336) mit Schlüssel `session_id`, TTL 30 bis 60 s; `BROKER_TIMEOUT` auf 2 s; bei Timeout auf `state.load_admin_session` (level.rs:377) zurückfallen statt `continue`. Liegt in `auth/`, im Contract verboten: braucht Freigabe des Users (`~/.claude/.contract-approvals/2026-09-06-analyse-scores-tempo-optik`, drei Pfade).
2. Q5 in audience_demographics.rs:322: `AND sv.ts_utc >= $1` ergänzen, Chunk-Pruning greift; kein Index nötig.
3. Q7 in audience_demographics.rs:473: prüfen, ob `streamer_login` beim Schreiben schon kleingeschrieben liegt; dann `LOWER()` entfernen, sonst Funktionsindex `(lower(streamer_login), message_ts)` per Migration.
4. Backfill-UPDATE (watch_time.rs:107, :268) aus dem GET-Pfad nehmen: `last_seen_at` beim Schreiben pflegen, einmaliger Backfill als Migration. Schreibpfad liegt im Bot (tb-monitoring), im Contract verboten; eigener Auftrag.
