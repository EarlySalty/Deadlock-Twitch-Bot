# Contract: KI-Hilfe-Assistent im Streamer-Dashboard

status: aktiv
datum: 2026-09-04
klasse: hoch
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

## Ziel

Ein eingeloggter Streamer findet auf jeder Seite von `/twitch/dashboard` unten rechts den Knopf "Hilfe bekommen" (wie auf `/streamer`), stellt dort jede Frage zum Bot, zum Partnernetz und zu seinem eigenen Kanal und bekommt eine freundliche, persönliche Antwort, die seine echten Dashboard-Daten auswertet.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Auf jeder Route der Dashboard-SPA (Übersicht, Verwaltung, Uplink, Social Media, Analyse, Overlay, Pricing) liegt unten rechts ein schwebender Knopf "Hilfe bekommen"; Klick öffnet ein Chat-Panel mit Begrüßung, Vorschlagsfragen, Verlauf, Eingabefeld (Enter sendet, Escape schließt), Ladezustand und Fehlerzeile. Bedienmuster und Optik folgen dem Widget der Landingpage (`SiteChatbot.tsx`), umgesetzt mit den Gold-Tokens der Dashboard-Shell.
- REQ-02: Die Begrüßung nennt den Streamer mit seinem Twitch-Anzeigenamen und duzt. Antworten kommen in der im Dashboard gewählten Sprache (Deutsch oder Englisch), Deutsch mit echten Umlauten und ohne Gedankenstriche.
- REQ-03: Der Chat ruft `POST /twitch/api/v2/dashboard/assistent/ask` mit `{question, history, page}`. Ohne gültige Dashboard-Session antwortet der Endpoint 401. Die Identität kommt ausschließlich aus der Session (Twitch-User-ID), nie aus dem Request-Body.
- REQ-04: Die Antwort ist grounded auf zwei Quellen: (a) dem Wissenskorpus aus `tb-knowledge` (Namespace Bot, inklusive der nur für eingeloggte Streamer bestimmten Dokumente, weil der Nutzer authentifiziert ist) und (b) Live-Datenkarten des eingeloggten Streamers: Partner- und Bot-Status, Stream-Kennzahlen der letzten 7 und 30 Tage, letzte Raids (ein- und ausgehend), Moderations- und Scam-Guard-Schalter, Uplink-Verbindungsstand, erteilte Scopes, plus die aktuelle Dashboard-Seite. Eine Frage wie "Wie liefen meine letzten Streams?" oder "Ist mein Spam-Schutz an?" wird mit den echten Werten des Streamers beantwortet.
- REQ-05: Der Kontext enthält nur Daten des eingeloggten Streamers. Nie Daten anderer Kanäle, nie Tokens, Schlüssel, Stream-Keys, Session-Werte oder interne Adressen. Fragen nach fremden Kanälen werden freundlich abgelehnt.
- REQ-06: Rate-Limit je Twitch-User-ID (20 Fragen pro Minute); darüber 429 und im Widget "Zu viele Fragen gerade. Probier es gleich noch einmal." Modellfehler oder Zeitüberschreitung liefern eine ehrliche Fehlerzeile in Nutzersprache, nie eine erfundene Antwort.
- REQ-07: Jede Frage samt Antwort wird dauerhaft in der Twitch-DB protokolliert mit Twitch-User-ID, Seite, grounded-Flag, Injection-Flag und Zeitstempel. Kein Discord-Log je Frage (keine Nachrichtenflut).
- REQ-08: Der Modellaufruf läuft über den zentralen `tb-llm`-Hub mit eigenem Use-Case `dashboard_assistent` und der Standard-Kette (Deepseek V4 Flash bei Fireworks). Kein Modell wird hart verdrahtet, kein neuer LLM-Client entsteht.
- REQ-09: Die Vorschlagsfragen hängen von der aktuellen Seite ab (mindestens je drei für Übersicht, Verwaltung, Uplink, Social Media; Standardvorschläge für alle anderen).
- REQ-10: Prompt-Injection-Marker in der Frage werden erkannt wie im öffentlichen Frage-Endpoint (gleiches Regex-Muster), die Frage wird trotzdem beantwortet, das Flag landet im Protokoll.
- REQ-11: Fragen, die der Korpus und die Datenkarten nicht abdecken, beantwortet der Assistent ehrlich mit einem Hinweis auf den Community-Discord, statt zu raten.

## Invarianten (darf sich nicht ändern)

- INV-01: Der öffentliche Endpoint `POST /twitch/api/v2/self-explainer/ask` und das Widget der Landingpage verhalten sich unverändert; Helfer aus `self_explainer.rs` dürfen nur sichtbar gemacht (`pub(crate)`) oder in ein gemeinsames Modul gezogen werden, ohne Verhaltensänderung.
- INV-02: Session-Prüfung und Identität laufen über den bestehenden Session-Extractor von tb-dashboard-api; kein zweiter Auth-Weg, kein verstecktes Feld, keine Namens-Nachschläge.
- INV-03: Keine neuen ENV-Variablen, keine neuen Secrets. Fireworks-Key und DB-Zugang kommen aus dem bestehenden Weg (Infisical über tb-llm und tb-db).
- INV-04: Die Modellwahl bleibt in `tb-llm` (`selection.rs`); der Assistent setzt kein `endpoint`, keine eigene Kette.
- INV-05: Bestehende Tests werden nicht gelöscht oder abgeschwächt. Neue Tests für Endpoint (Auth, Rate-Limit, Kontextaufbau ohne Fremddaten, Injection-Flag) und Widget (Vorschläge je Seite, Fehlerzustände).
- INV-06: Datenkarten werden aus bestehenden Lesefunktionen der Dashboard-API gebaut (dieselben Funktionen, die die Dashboard-Seiten speisen), nicht aus neuen Roh-SQL-Abfragen, wenn eine passende Funktion existiert.
- INV-07: Die Dashboard-Shell (Sidebar, Layout) wird nicht umgebaut; das Widget hängt sich als globales Element über alle Routen ein, analog zum Sprach-Provider in `App.tsx`. Der parallele Branch `feat/dashboard-shell-einheitlich` bleibt unberührt.
- INV-08: Keine Code-Kommentare, keine Gedankenstriche in Nutzertexten, Nutzersprache statt internem Vokabular in allen sichtbaren Texten.
- INV-09: Der Wissenskorpus-Loader (`tb-knowledge`) bleibt die einzige Quelle für Dokumentwissen; kein zweiter Korpus, keine Duplikate der Doku im Prompt-Code.

## Nicht-Ziele

- Kein Tool-Calling oder Agenten-Loop, in dem das Modell selbst Abfragen auslöst; die Datenkarten werden serverseitig deterministisch vor dem Modellaufruf gebaut.
- Keine Schreibaktionen aus dem Chat heraus (kein Umschalten von Einstellungen, keine Raids, kein Trennen).
- Kein Streaming der Antwort (eine Antwort je Request, wie beim Landing-Widget).
- Keine Änderung am öffentlichen Landing-Widget oder an `/streamer`.
- Keine Admin-Sicht auf fremde Streamer, auch nicht im Admin-Modus.
- Kein Persistieren des Chat-Verlaufs über den Reload hinaus (Verlauf lebt im Browser-State).

## Erlaubter Änderungsbereich

- rust/crates/tb-dashboard-api/src/handlers/dashboard_assistent.rs
- rust/crates/tb-dashboard-api/src/handlers/mod.rs
- rust/crates/tb-dashboard-api/src/lib.rs
- rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs
- rust/crates/tb-dashboard-api/tests/
- rust/crates/tb-analytics/src/
- rust/crates/tb-knowledge/src/
- rust/crates/tb-db/migrations/
- docs/knowledge/
- bot/dashboard_v2/src/components/assistent/
- bot/dashboard_v2/src/api/assistent.ts
- bot/dashboard_v2/src/App.tsx
- bot/dashboard_v2/src/i18n/
- bot/dashboard_v2/src/index.css
- bot/dashboard_v2/tests/
- bot/dashboard_v2/package.json
- .tasks/2026-09-04-dashboard-assistent/

## Verbotene Änderungen

- rust/crates/tb-llm/ (Modellwahl, Kette, Provider)
- website/ (Landingpage und ihr Widget)
- bot/dashboard_v2/src/components/layout/ (Shell, Sidebar)
- Lint-Konfiguration, CI-Workflows, Cargo-Workspace-Abhängigkeiten außer im Crate tb-dashboard-api
- Bestehende Migrationen
- Jede Datei mit Secrets oder Tokens

## Offene Produktfragen

- keine

## Amendments

- 2026-09-04, Erlaubter Änderungsbereich: `docs/knowledge/` -> `rust/knowledge/` (der produktive Korpus liegt unter rust/knowledge/bot und rust/knowledge/deadlock, docs/knowledge existiert nicht), Grund: Pfadfehler im Contract, entschieden von Orchestrator (nur technisch, reversibel)

- 2026-09-04: Erlaubter Änderungsbereich `rust/crates/tb-db/migrations/` -> `rust/migrations/` (der sqlx-Migrator zeigt auf rust/migrations, tb-db hat keinen eigenen Ordner), Grund: Pfadfehler im Contract, entschieden von Orchestrator (nur technisch, reversibel)
- 2026-09-04: Erlaubter Änderungsbereich ergänzt um `rust/crates/tb-dashboard-api/src/handlers/internal_home.rs`, `moderation_settings.rs`, `scam_guard_settings.rs`, `uplink.rs`, `ad_manager.rs`, dort ausschließlich Sichtbarkeit (`pub(crate)`) oder das Herausziehen einer Lesefunktion ohne Verhaltensänderung, Grund: die Wiederverwendung der bestehenden Lesefunktionen (Invariante zu Datenkarten) verlangt Zugriff auf diese privaten Funktionen, entschieden von Orchestrator (nur technisch, reversibel)
- 2026-09-04: Vorgehen Request-Body: zusätzlich zum vereinbarten Body das Feld `language` (de|en) aus der Dashboard-Sprachwahl, Server fällt ohne Wert auf de zurück, Grund: die gewählte Sprache lebt nur im Browser-State und ist serverseitig nicht bekannt, entschieden von Orchestrator (nur technisch, reversibel)
- 2026-09-04: Vorgehen Limits: zusätzlich zum Minutenlimit ein Tagesdeckel von 150 Fragen je Twitch-User-ID (ebenfalls 429, im Prozessspeicher), Grund: Kostendeckel für den LLM-Aufwand je Streamer, Default nach Akte, entschieden von Orchestrator (reversibel)
- 2026-09-04: Vorgehen Protokoll: neue Tabelle `twitch_dashboard_assistent_log` per Migration in `rust/migrations/` mit eigener Schreibfunktion in tb-analytics statt Wiederverwendung von `twitch_self_explainer_log` (hat keine Spalten für Twitch-User-ID und Seite; `peer` zweckzuentfremden wäre eine stille Bedeutungsänderung für list_recent), entschieden von Orchestrator (nur technisch, reversibel)
