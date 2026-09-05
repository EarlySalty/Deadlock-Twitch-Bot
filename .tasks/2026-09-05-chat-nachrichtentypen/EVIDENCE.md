# Evidence: Chat-Nachrichtentypen

status: aktiv
datum: 2026-09-05
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- rust/crates/tb-analytics/src/chat_analytics.rs:36-69: `classify_message`, First-Match über Wortlisten (Command, Hype, Greeting, Question, Feedback, Technical, Social, Reaction, Game-Related, Other); Zeile 447 Aufruf je Nachricht, Zeilen 639-711 Aggregation zu `messageTypes`.
- rust/crates/tb-dashboard-api/src/handlers/viewers.rs:884-1001: zweite Kopie `classify_message` für Personality-Typen.
- rust/crates/tb-analytics/src/chat_deep_minimax.rs:60-72: Prompt mit denselben Kategorien plus Zählobjekt (MiniMax, nicht freigegeben; Nicht-Ziel).
- rust/crates/tb-analytics/src/post_stream.rs:366-388: Muster für Hub-Aufruf `tb_llm::Request::prompt(..).max_tokens(..).timeout_secs(..)`, `tb_llm::complete(use_case, request.ledger_purpose(..))`, Fehlerbehandlung `LlmError::Http`.
- rust/crates/tb-llm/src/hub.rs:143: `Request::json_object()` für JSON-Antworten; hub.rs:279 `complete(use_case, request)`.
- rust/crates/tb-llm/src/selection.rs:10,26: Default-Endpoint Fireworks `deepseek-v4-flash-0731`, `endpoint_for(use_case)`.
- rust/bin/tb-bot/src/ad_manager_wiring.rs:24-41: Muster für Hintergrundjob mit `tokio::time::interval` (täglich und 25 s).
- rust/bin/tb-bot/src/chatters_wiring.rs:264-287: Sammel- und Retention-Ticks als zweites Muster.
- rust/migrations/20260903090000_twitch_moderation_settings.sql: jüngste Migration, Muster `CREATE TABLE IF NOT EXISTS public.…`.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- rust/crates/tb-analytics/src/bekannte_bots.rs: entsteht im Contract `.tasks/2026-09-05-backend-bots-sprache/` (Branch `feat/backend-bots-sprache`); bis zum Merge die lokale Liste `chat_analytics.rs:21` nutzen und im PLAN vermerken.
- rust/crates/tb-analytics/src/chat_analytics.rs:186-319: SQL-Loader über `twitch_chat_messages cm` mit Zeitraum und Streamer; hier per LEFT JOIN auf `twitch_chat_message_labels` erweitern.
- bot/dashboard_v2/src/pages/chatAnalyticsContent.tsx:251-265: Karte "Nachrichtentypen" (`data.messageTypes.map`), Leerzustand Zeile 265.
- bot/dashboard_v2/src/types/analytics.ts:67: `MessageTypeStat`.

## Relevante Tests (laufen vorher, laufen nachher)

- rust/crates/tb-analytics/src/chat_analytics.rs:737-749: `classify` (Beispiele moin, warum lagt das?, lol haha, haze build ist gut, zzz).
- rust/crates/tb-analytics/src/chat_deep_minimax.rs:111-158: Prompt- und Fetch-Tests (unverändert).
- bot/dashboard_v2/tests/i18n.test.ts: Wörterbuch-Test, neue Schlüssel dort mitprüfen.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- `GET /twitch/api/v2/chat-analytics` Feld `messageTypes` `[{type, count, pct}]` (chat_analytics.rs:642-711).
- `twitch_chat_messages` Spalten: id, message_id, session_id, streamer_login, chatter_id, chatter_login, content, is_command, message_ts, moderation_action, moderation_reason.

## Datenlage (DB twitch_analytics, 2026-09-05)

- 226457 Nachrichten ohne Befehl, 555 Kanäle, seit 2026-01-31; größte Kanäle timosius 15969, dehackxas 15247, denoshock 11246, miracleghost9 10792, earlysalty 10335.
- earlysalty 30 Tage: Other 36,6 % (1174), Question 23,7 %, Reaction 17,8 %, Greeting 7,8 %, Game-Related 4,3 %, Hype 3,9 %, Technical 2,1 %, Command 1,6 %, Social 1,2 %, Feedback 1,2 %.

## Stichprobe "Other" mit Zieltyp (Regelstufe)

- "LUL LUL LUL LUL LUL", "KappaPride", "NotLikeThis NotLikeThis NotLikeThis  ich sag ja lexus": Reaction
- "@Zenkay123 IAmClap missmo107FLEISCHWURST": Social
- "Plorki_GER redeemed Confetti (duo) for 0 Bits", "🔧 Eisen-Truhe! !loot (riskant) · !prüfen (vorsichtig) · !dodge (sicher) ... 45s", "Brain_BP Abofalle schlägt zu! Vielen Dank HappyPag": System
- "phantom 4 gerade", "du bist phantom 5 und nicht top 1000 ingame", "Emmi 1 angefangen solo hoch gecarryt auf Emmi 6", "Rigged matchmaking", "ich versteh auch nicht wie niedrig die velocity von der sniper ist", "Graves Doorman hätten nie ins Game kommen sollen": Game-Related
- "ist schon echt gut muss ich sagen und sieht qualitativ auch echt ok aus ngl", "das ist der reiz.. arc raider war mir zu viel mimimi", "Astronomie meint er übrigens, nicht Astrologie": Statement (Aussage)
- "Ai viewers streamboo . Com": Other (Spam bleibt Rest)
- "jo", "yes", "rip", "woa": Reaction

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- rust/crates/tb-analytics/src/chat_typen.rs (neu): Enum, Regelstufe, Deepseek-Paketaufruf, Label-Speicher
- rust/crates/tb-analytics/src/chat_analytics.rs, rust/crates/tb-dashboard-api/src/handlers/viewers.rs: Kopien entfernen, Labels lesen
- rust/bin/tb-bot/src/chat_typen_wiring.rs (neu): Job
- rust/migrations/2026090…_twitch_chat_message_labels.sql
- bot/dashboard_v2/src/pages/chatAnalyticsContent.tsx, types, i18n

## Offene Architekturfrage

- keine
