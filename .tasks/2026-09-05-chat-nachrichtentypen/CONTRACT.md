# Contract: Chat-Nachrichtentypen: gespeicherte Klassifikation für alle Kanäle

status: aktiv
datum: 2026-09-05
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Die Karte "Nachrichtentypen" im Chat-Tab zeigt für jeden Kanal genaue Anteile: der Sammeltopf "Other" schrumpft von rund 37 % auf einen kleinen Rest, weil jede Nachricht einmal klassifiziert und das Ergebnis gespeichert wird (Regeln zuerst, Deepseek V4 Flash für den Rest), rückwirkend für alle 555 Kanäle und laufend für neue Nachrichten.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Es gibt genau einen Klassifizierer im Code (`tb-analytics::chat_typen`), der die Regelstufe enthält; die beiden heutigen Kopien von `classify_message` (`chat_analytics.rs`, `viewers.rs`) verschwinden und nutzen ihn. Feste Typen-Menge als Enum mit stabilen API-Schlüsseln: `Command`, `Hype`, `Greeting`, `Question`, `Feedback`, `Technical`, `Social`, `Reaction`, `Game-Related`, `Statement`, `Other`, `System`.
- REQ-02: Die Regelstufe erkennt zusätzlich: reine Emote-Nachrichten (nur Twitch-Emote-Namen wie LUL, Kappa, PogChamp, NotLikeThis, Deadge, plus Unicode-Emoji) als `Reaction`; Nachrichten, die nur aus @Erwähnung plus höchstens zwei Wörtern bestehen, als `Social`; Bot- und Systemtexte (Kanalpunkte-Einlösungen "redeemed … for … Bits", eigene Bot-Ausgaben wie Truhen-, Loot- und Abo-Meldungen, Nachrichten der bekannten Bot-Logins aus `bekannte_bots`) als `System`; Deadlock-Vokabular (Helden, Items, Ränge wie Phantom, Oracle, Emissary, Eternus, Begriffe wie Souls, Urn, Lane, Build, Patch, Meta, Buff, Nerf, Matchmaking) als `Game-Related`. `System` wird in keiner Anteilsrechnung mitgezählt.
- REQ-03: Neue Tabelle `twitch_chat_message_labels` (`message_id` Primärschlüssel mit Bezug auf `twitch_chat_messages.id`, `label`, `quelle` in {`regel`,`modell`}, `modell` nullable, `erstellt_am`), Migration als Datei unter `rust/migrations/`, Anwendung wie im Repo üblich von Hand als `postgres` mit Grants: `twitchbot` SELECT/INSERT/UPDATE/DELETE, `twitchdash` SELECT, Eintrag in `_sqlx_migrations`. Der Bot migriert nicht selbst.
- REQ-04: Ein Hintergrundjob im Bot (tb-bot) klassifiziert unlabelte Nachrichten aller Kanäle: Regelstufe für alle, danach Deepseek V4 Flash über `tb_llm::complete("chat_message_type", …)` in Paketen von 40 Nachrichten (JSON-Antwort, Temperatur 0) nur für Nachrichten, deren Regelergebnis `Other` ist. Der Job läuft beim Start und danach stündlich, verarbeitet je Lauf höchstens 20000 Nachrichten und höchstens 2000 Modellaufrufe je Tag; Fehler eines Pakets lassen die Nachrichten unlabelt für den nächsten Lauf. Der Ledger in tb-llm verbucht jeden Aufruf unter `chat_message_type`.
- REQ-05: Die Karte "Nachrichtentypen" (`/twitch/api/v2/chat-analytics`, Feld `messageTypes`) und die Personality-Typen in `viewers.rs` lesen das gespeicherte Label und fallen für unlabelte Nachrichten auf die Regelstufe zurück; `System` wird ausgeschlossen. Die Antwort trägt zusätzlich `labelCoverage` (Anteil gespeicherter Labels im Zeitraum, 0 bis 1).
- REQ-06: Die Karte zeigt deutsche Bezeichnungen: Befehl, Hype, Begrüßung, Frage, Feedback, Technik, Sozial, Reaktion, Spielbezug, Aussage, Sonstiges; unter der Karte steht bei `labelCoverage < 0.95` der Hinweis "Ein Teil der Nachrichten wird noch zugeordnet."
- REQ-07: Regressionstests ohne Live-DB für die Regelstufe: jede Beispielzeile aus EVIDENCE (Abschnitt Stichprobe) bekommt den dort genannten Zieltyp; ein Test belegt, dass der Deepseek-Aufruf nur für `Other` erfolgt (Aufrufzähler über einen Test-Endpoint wie in `tb-llm`-Tests).
- REQ-08: `cargo test -p tb-analytics -p tb-dashboard-api -p tb-bot` gegen die Baseline grün; `npm run build`, `npm run lint`, `npm test` in `bot/dashboard_v2` grün.

## Invarianten (darf sich nicht ändern)

- INV-01: Modell ausschließlich Deepseek V4 Flash über den tb-llm-Hub (Default-Endpoint); kein direkter HTTP-Client, kein anderes Modell, kein MiniMax.
- INV-02: Keine ENV-Variablen; Limits als Konstanten im Job-Modul.
- INV-03: `twitch_chat_messages` bleibt unverändert (kein neues Feld, kein Update dort); Labels leben nur in der neuen Tabelle.
- INV-04: API-Feldnamen bestehender Antworten bleiben; `messageTypes` behält Form `[{type, count, pct}]` mit den englischen Schlüsseln, die Übersetzung passiert im Frontend.
- INV-05: Keine Code-Kommentare; bestehende Kommentare nicht erweitern; echte Umlaute, keine Em-Dashes.
- INV-06: Bestehende Tests werden nicht gelöscht oder abgeschwächt.

## Nicht-Ziele

- Änderungen am MiniMax-Tiefenreport (`chat_deep_minimax.rs`); dessen Ablösung ist ein eigener Auftrag.
- Sentiment oder Toxizität.
- Umbau anderer Chat-Karten.

## Erlaubter Änderungsbereich

- rust/crates/tb-analytics/src/**
- rust/crates/tb-analytics/tests/**
- rust/crates/tb-analytics/.sqlx/**
- rust/crates/tb-analytics/Cargo.toml
- rust/crates/tb-dashboard-api/src/handlers/chat_analytics.rs
- rust/crates/tb-dashboard-api/src/handlers/viewers.rs
- rust/crates/tb-dashboard-api/.sqlx/**
- rust/bin/tb-bot/src/**
- rust/bin/tb-bot/Cargo.toml
- rust/migrations/2026090*_twitch_chat_message_labels.sql
- rust/.sqlx/**
- rust/Cargo.lock
- bot/dashboard_v2/src/pages/chatAnalyticsContent.tsx
- bot/dashboard_v2/src/types/analytics.ts
- bot/dashboard_v2/src/i18n/dictionary.ts
- bot/dashboard_v2/tests/nachrichtentypen.test.ts
- bot/dashboard_v2/package.json
- .tasks/2026-09-05-chat-nachrichtentypen/

## Verbotene Änderungen

- rust/crates/tb-llm/** (Hub bleibt, Use-Case-Name reicht)
- rust/crates/tb-analytics/src/chat_deep_minimax.rs
- Dateien der Contracts `.tasks/2026-09-05-analyse-frontend-optik/` und `.tasks/2026-09-05-backend-bots-sprache/` sowie deren Änderungsbereiche außer den oben genannten Überschneidungen (`viewers.rs`, `chat_analytics.rs` in tb-dashboard-api: dort nur die Klassifikation anfassen)
- systemd-Units, Caddy, Python

## Offene Produktfragen

- keine

## Amendments

