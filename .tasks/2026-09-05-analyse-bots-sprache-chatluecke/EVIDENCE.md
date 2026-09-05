# Evidence: Analyse-Backend Bot-Ausschluss, Sessionsprache, Chat-Lücken-Warnung

status: aktiv
datum: 2026-09-05
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- rust/crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs:3-14: `KNOWN_CHAT_BOTS` (botrix, fossabot, moobot, nightbot, ...), ohne own3d, kofistreambot, justinfan.
- rust/crates/tb-dashboard-api/src/handlers/viewer_exclusion.rs:92-118: `viewer_exclusion_logins_from_dynamic`, `viewer_exclusion_logins`, `is_known_or_dynamic_excluded` (exakter Login-Vergleich, kein Präfix-Muster).
- rust/crates/tb-dashboard-api/src/handlers/lurker_analysis.rs:19-29,62,113,183: eigene Kopie `KNOWN_CHAT_BOTS`, als `$3` in SQL gebunden.
- rust/crates/tb-chat/src/chatter_tracking.rs:47: dritte Kopie `KNOWN_CHAT_BOTS`, Test `known_bots_erkannt` Zeile 563.
- rust/crates/tb-monitoring/src/irc_lurker.rs:23: vierte Kopie; Zeile 425 nutzt selbst `justinfan12345` als anonymen Nick (dieser Login landet als Zuschauer in den Daten).
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

- rust/crates/tb-analytics/src/bekannte_bots.rs: neue gemeinsame Liste plus `ist_anonymer_login`
- rust/crates/tb-dashboard-api/src/handlers/{viewer_exclusion,lurker_analysis,audience_demographics,audience,viewers}.rs
- rust/crates/tb-chat/src/chatter_tracking.rs, rust/crates/tb-monitoring/src/irc_lurker.rs
- rust/crates/tb-analytics/src/raw_chat_status.rs plus Test
- Session-Schreiber in tb-monitoring (Fundstelle vom Implementierer belegen)

## Offene Architekturfrage

- keine
