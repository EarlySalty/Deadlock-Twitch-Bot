# Evidence: Analyse-Backend Bot-Ausschluss, Sessionsprache, Chat-Lücken-Warnung

status: überholt
datum: 2026-09-05
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- rust/crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs:3-14: `KNOWN_CHAT_BOTS` (botrix, fossabot, moobot, nightbot, ...), ohne own3d, kofistreambot, justinfan.
- rust/crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs:92-118: `viewer_exclusion_logins_from_dynamic`, `viewer_exclusion_logins`, `is_known_or_dynamic_excluded` (exakter Login-Vergleich, kein Präfix-Muster).
- 20 Kopien der Bot-Liste (Stand 2026-09-05, `grep -rn "KNOWN_CHAT_BOTS: \|WHITELISTED_BOTS: " rust/crates`): tb-analytics `chat_deep_minimax.rs:11`, `overview.rs:143` (pub), `chat_hype_timeline.rs:20`, `chat_analytics.rs:21`, `chat_social_graph.rs:19`, `watch_time.rs:28`, `chat_content_analysis.rs:22`; tb-dashboard-api `internal_home.rs:2027`, `follower_funnel.rs:22`, `loyalty_curve.rs:13`, `audience_demographics.rs:25`, `session_detail.rs:21`, `audience.rs:23`, `viewer_exclusion.rs:3`, `lurker_analysis.rs:19`, `raid_analytics.rs:22`; tb-chat `chatter_tracking.rs:47`, `mention_scoring.rs:52` (`WHITELISTED_BOTS`); tb-monitoring `irc_lurker.rs:23`; tb-engagement `irc_message.rs:74`.
- rust/crates/tb-dashboard-api/src/handlers/lurker_analysis.rs:62,113,183: Liste als `$3` in SQL gebunden (Muster für alle SQL-Konsumenten).
- rust/crates/tb-monitoring/src/irc_lurker.rs:425: nutzt selbst `justinfan12345` als anonymen Nick (dieser Login landet als Zuschauer in den Daten).
- rust/crates/tb-dashboard-api/src/handlers/viewers.rs:18: nutzt `viewer_exclusion_logins`.
- rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs:272-296: Sprach-Mix aus `twitch_stream_sessions.language`, leere Werte werden zu "unknown" und zählen als eigene Gruppe; `lang_rows.first()` gewinnt, also "Unbekannt" bei Mehrheit leerer Sessions.
- rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs:648: `peakHoursMethod` "weighted_chat_activity_exp_decay_h{}_w{}_winsor_p90".
- rust/crates/tb-analytics/src/raw_chat_status.rs:280-282: `suspected_issue` ist wahr, wenn Presence-Zeilen ohne Roh-Zeilen existieren oder wenn `gap_sessions > 1` bei `sessions_with_raw > 0`.
- rust/crates/tb-analytics/src/raw_chat_status.rs:78-160: `query_scope_presence` zählt `gap_sessions` ohne Mindestdauer oder Chatter-Bedingung.
- rust/crates/tb-analytics/src/raw_chat_status.rs:308-320: Notiz-Texte mit "Presence-/Rollup-Daten", "Roh-Chat", "message-basierte KPIs", "Insert-Fehler".
- rust/crates/tb-monitoring/src/telemetry.rs:619: `twitch_channel_updates` trägt `language` je `twitch_user_id` (Quelle für den Backfill; für earlysalty 58 Zeilen "de", 73 leer).
- rust/crates/tb-monitoring/src/stats.rs:61: Stats-Insert trägt `language`; Session-Insert-Pfade in tb-analytics `ai_analysis.rs:859,911` und `admin_streamers.rs:1083` schreiben keine `language`.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- rust/crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs:30: `push_normalized_login`.
- rust/crates/tb-analytics/src/overview.rs:143: einzige bereits öffentliche Liste (`pub const`), Kandidat als Ausgangspunkt für das Modul `bekannte_bots`.
- rust/crates/tb-analytics/src/raw_chat_status.rs:208: `build_raw_chat_status(pool, streamer, scope)`.

## Relevante Tests (laufen vorher, laufen nachher)

- rust/crates/tb-chat/src/chatter_tracking.rs:563: `known_bots_erkannt`.
- rust/crates/tb-dashboard-api/src/handlers/viewers.rs:1340,1661,1726: Exklusivitäts- und Status-Tests.
- rust/crates/tb-dashboard-api/src/handlers/lurker_analysis.rs:301: `anonymous_null_login_lurker_counted`.
- rust/crates/tb-monitoring/tests/poller.rs:1255: Session-Fixture.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- rust/crates/tb-dashboard-api/src/handlers/audience_demographics.rs:661: JSON-Feld `primaryLanguage`.
- rust/crates/tb-analytics/src/raw_chat_status.rs:217-222: JSON-Felder `coverageStart`, `gapStart`, `suspectedIngestionIssue`, `note`.

## Datenlage (DB twitch_analytics, 2026-09-05)

- `twitch_stream_sessions` 90 Tage: de 2498, leer 1029, en 110; earlysalty 90 Tage: leer 49, de 13.
- earlysalty 30 Tage: 30 Sessions, davon 8 mit `samples <= 16` oder `unique_chatters = 0` (Geister-Sessions), Chat-Nachrichten an 13 Tagen vorhanden.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- rust/crates/tb-analytics/src/bekannte_bots.rs: neue gemeinsame Liste plus `ist_ausgeschlossener_login`
- alle 20 Fundstellen oben
- rust/crates/tb-analytics/src/raw_chat_status.rs plus Test
- Session-Schreiber in tb-monitoring (Fundstelle vom Implementierer belegen)

## Offene Architekturfrage

- keine
