# Research: KI-Hilfe-Assistent im Streamer-Dashboard

status: aktiv
datum: 2026-09-04
quelle: Explore-Agent (read-only) auf Worktree feat/dashboard-assistent, HEAD 1540aebb

Fundstellen mit Zeilen stehen in EVIDENCE.md. Hier: Befunde, Widersprüche, Entscheidungen.

## Befunde

1. Es gibt genau eine Chat-Vorlage: das Landing-Widget `SiteChatbot.tsx` gegen den öffentlichen `self_explainer`-Endpoint. Alles andere im Dashboard (AIAnalysis-Folgechat mit analysis_id und Plan-Gate, Coaching regelbasiert, Uplink-Hilfe als statische HTML-Fragmente) ist kein freier Assistent.
2. Der `self_explainer` hat alle Bausteine, die der Assistent braucht (Prompt-Bau, Injection-Erkennung, History-Parsing, Antwort-Zerlegung, Fallbacks, Rate-Limiter, Korpus-Singleton), aber alles ist modul-privat. Ein Quelltext-lesender Test verbietet das Verschieben in ein anderes Modul; `pub(crate)` heben ist der sichere Weg.
3. Der LLM-Hub kennt kein Tool-Calling. Personalisierung geht nur über serverseitig vorab gebaute Datenkarten im System-Prompt. Das deckt sich mit dem Contract (kein Agenten-Loop).
4. Die Datenkarten existieren bereits als private Funktionen in `internal_home.rs` (Partner-Status, OAuth/Scopes, KPIs, Raids, Bans, letzter Stream), als Inline-SQL in den GET-Handlern für Moderation und Scam-Guard und als Helfer in `uplink.rs`.
5. Der Korpus (21 Bot-Docs) trägt durchgehend `audience: streamer`; `select(.., Some("streamer"), ..)` liefert heute dieselbe Menge wie der öffentliche Pfad. Der Assistent bekommt also keine geheimen Docs, aber der Weg ist offen für spätere Docs mit eigener Zielgruppe.
6. CSRF: der Write-Schutz akzeptiert entweder das Session-CSRF-Token im Header `x-csrf-token` oder same-origin plus gültige DB-Session. Das Widget schickt das Token aus dem Auth-Status, wenn vorhanden, und funktioniert per same-origin auch ohne.
7. Log-Tabelle des öffentlichen Endpoints hat keine Spalten für Twitch-User-ID und Seite. Neue Tabelle per Migration; prod migriert nicht selbst (Migration als postgres einspielen, Rechte an twitchbot und twitchdash, Version in `_sqlx_migrations`).
8. Frontend: kein Router-Paket, Routing über `window.location.pathname` in `App.tsx`; globales Element gehört in den `LanguageProvider` neben die Routen-Ternär. Farbtest erzwingt die Gold-Palette. Testliste in `package.json` ist explizit.

## Widersprüche und ihre Auflösung

- Rate-Limit-Schlüssel: Bestand ist Peer-IP mit 10/60 s. Auflösung: den bestehenden In-Memory-Limiter parametrisierbar machen (Fenster, Maximum) und mit der Twitch-User-ID als Schlüssel zweimal instanziieren (20/60 s und 150/24 h). Kein DB-Limiter, keine Klartext-IDs in der DB.
- REQ-04 "nur für eingeloggte Streamer bestimmte Dokumente": derzeit ohne Effekt (siehe 5). Keine neuen Docs in diesem Task; Zielgruppe `streamer` wird explizit übergeben.
- Raids: die einzige streamergebundene Quelle ist `raid_events_block` in internal_home.rs, deshalb Scope-Amendment für Sichtbarkeitsänderungen.
- Kosten: kein Plan-Gate, dafür Minuten- und Tagesdeckel (Amendment).
- `/social-media-admin` läuft im selben Bundle: Widget erscheint auch dort (gilt als Route der SPA).

## Entscheidungen (Orchestrator, im Contract als Amendments)

- Body-Feld `language`, Tagesdeckel 150, neue Log-Tabelle, Scope-Erweiterung um fünf Handler-Dateien für Sichtbarkeit.
