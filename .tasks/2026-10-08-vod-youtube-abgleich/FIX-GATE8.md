[Orchestrator]
# Begrenzte gemeinsame Korrektur nach Gate 8

## 1. Ziel und Vertrag

Dies ist derselbe offene YouTube-Abgleich, kein neuer Auftrag oder Neuaufbau. Hauptorchestrator hat die gemeinsame Korrektur ausdrücklich freigegeben. Alle erhaltenen Fixes und Nachweise bewahren. Ausgangsquellen und integrierter HEAD: 4de559835dcded44aaa700491910d4270f132523. Lies AUFTRAG.md, PLAN.md und den exakten Gatebeleg pruefung/gate-round8.txt. REGISTER.md und REVIEW.md enthalten die aktuelle Entscheidung; HANDOFF.md ist historisch und keine aktuelle Freigabe.

Verbindliche Kriterien:
1. Fehler oder unvollständige Suche dürfen vorhandenen weiterhin konto-/quellen-/teilgebundenen processed-Nachweis weder löschen noch als Uploadfreigabe behandeln. Geänderte Identität oder Quelle bleibt ausgeschlossen. Bestehende API-Guards, Vorbereitung und tatsächlicher Worker-Upload-Eintritt müssen denselben Vertrag durchsetzen. Den Test, der nach connection-Fehler einen Upload erwartet, korrigieren, nicht abschwächen oder entfernen. Alte Beobachtungszeiten und ursprüngliche Evidenzbindung erhalten, nicht auf einen neuen Gesamtstand umhängen.
2. Nach vollständig belegtem Abschluss verlässt die Wiederherstellung den Arbeitszustand dauerhaft. Kein downloaded-Zyklus. Den vorhandenen terminalen Abschlussweg verwenden, keine historische Uploadzeit, Video-ID oder lokale Teilhistorie erfinden. Aufräumen bleibt nur mit dem bestehenden gültigen vollständigen Zielnachweis erlaubt. Der reine Leseloop und Prüfen dürfen keine Uploadwirkung bekommen.
3. Letzten Prüfversuch und Fehler in der API unabhängig von der Gültigkeit eines früheren Erfolgs sichtbar machen, einschließlich fehlendem Zugang, fehlendem beobachtetem Kanal und geänderter Teileliste. Erhaltener Erfolg darf aktuellen Fehler nicht verstecken. Liste und POST bleiben an dieselbe echte aktuelle Identität gebunden, innerhalb bestehender einheitlicher Sperrreihenfolge. Ein Fehler ist kein neuer Erfolgsnachweis.
4. Fehlende englische Wiederholungserklärung eng ergänzen. Keine UI-Politur oder Browserrunde.

Keine neue OAuth-/Providerstrecke, Tabelle, Migration, Queue-Neugestaltung oder fremde Bereinigung. Nur vorhandene Zustandslogik korrigieren. Keine produktiven YouTube-Aufrufe oder Produktionseingriffe durch den Fixer.

## 2. Eigentum

Du bist genau ein frischer nativer Fixer im gleichen Worktree. Keine Unteragenten, zusätzlichen T3-Threads oder fremde Sessionkoordination. Alleinige Quellen-Schreibhoheit während deiner Arbeit für:
- rust/crates/tb-vod-archive/src/youtube_check.rs
- rust/crates/tb-vod-archive/src/youtube_check_tests.rs
- rust/crates/tb-vod-archive/src/worker.rs samt vorhandenen Inline-Tests
- rust/crates/tb-vod-archive/src/store.rs samt vorhandenen Inline-Tests
- rust/crates/tb-dashboard-api/src/handlers/social_media_vod_archive.rs
- rust/crates/tb-dashboard-api/src/handlers/social_media_vod_archive_tests.rs
- bot/dashboard_v2/src/i18n/vodArchive.ts

Eltern besitzt Register, Statusereignisse und Berichte. Diese nicht editieren/stagen/committen. Eigene Prüf- und Gate-Logs unter .tasks/2026-10-08-vod-youtube-abgleich/fixer-r8-*.log erlaubt. Keine neuen Code-Kommentare, keine globale Formatierung oder Nachbaränderungen. Weitere nötige Quellpfade zuerst als tatsächlichen engen Blocker melden.

## 3. Arbeitsstand und Git

Worktree: /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008
Branch: feat/vod-youtube-abgleich-20261008
Eltern sichert gerade ausschließlich vorhandene Gate-8-Akten und dieses Briefing. Vor eigener Bearbeitung git status und log -1 ansehen. Quellen sind committed, keine aktive eigene Cargo-Queue und kein anderer Quellenwriter. Eltern ist alleiniger Integrations- und Liveverantwortlicher.

Eigene Quelländerungen einzeln stagen und committen ist freigegeben, keine Elternakten einschließen. Trailer: Co-Authored-By: Claude Code <noreply@anthropic.com>.
Nach erfolgreicher gezielter Verifikation aktuelles origin/main regulär im eigenen Worktree integrieren. Git-Schritte einzeln mit absoluten literalen Pfaden, kein add -A, force, stash, reset oder Eingriff in Kanon/fremde Bäume. Nur normale Hooks. Kein Push, Deploy oder Cleanup durch dich. Regulärer Gate erst auf committed integriertem Stand. Eltern beendet nach ALLOW Main/Release/Live/Cleanup.

## 4. Beweisziel

Bestehende SQL-/Workerproben entlang der vollständigen Sequenz Teilbestätigung, ausdrücklicher Retry, fehlgeschlagene Folgeprüfung, Abschluss und erneuter Poll gemeinsam prüfen. Beide bestehenden Vorbereitungspfade, tatsächlichen Upload-Eintritt, Wiederholung ohne zusätzliche Uploads, API GET/POST, Fehleranzeige und unveränderte historische Felder berücksichtigen. Geänderte Authrevision, Quelle und Einzelteile bleiben negative Gegenproben. Keine bloßen Bool-Mocks für den riskanten DB-/Queue-Pfad.

Eigene vorhandene synthetische PostgreSQL: Port 55683, DB token_db_youtube, DSN postgresql://nathanael@127.0.0.1:55683/token_db_youtube. Bereits laufend, fremde Prozesse nicht verändern. Archiv-Testhelper verlangt die reine DSN-Zeile in rust/token-db-tests.conf. Temporär nur hierfür per Write herstellen und nach echten DB-Läufen entfernen. API-Proben verwenden TB_TEST_DATABASE_URL; bisherigen tatsächlichen Aufruf in fixer-r7-api.log beziehungsweise r7-api.txt nachlesen. Keine Secretdateien lesen.

Alle Cargo-Prüfungen ausschließlich /home/nathanael/.local/bin/cargo-slot mit --jobs 3. Genau ein regulärer Warteaufruf zugleich, ohne selbst gesetzte 30-Minuten-Abbruchgrenze; keine eigenen Locks, Wrapperänderung oder alternative Cargo-Strecke. Echtes Werkzeuglimit melden, kein Null-Lauf als Erfolg. Wrapper-Konfiguration kann aktuell anders sein, keine Dreislot-Annahme.

Archiv: test -p tb-vod-archive -- --include-ignored --nocapture. API: bestehender fokussierter VOD-Archiv-Testfilter. Archiv-Clippy --all-targets --no-deps -- -D warnings. API-Clippy --all-targets --no-deps ohne -D warnings, Warnungen transparent; keine fremde Baselinebereinigung. Gezielt rustfmt und diff --check.

Erhaltene Nachweise: 66 Archiv-, 13 API-, neun Frontend- und zwei Clientproben, verschiedene tatsächliche Quellstände. Keine erneute Provider-/UI-Vorprobe. Neue Counts samt passed/failed/ignored/filtered, exaktem Befehl und Exit getrennt melden. Quellenänderungen brauchen passende fehlende Rust-Nachweise. UI-/Client-Quellen werden nicht umgebaut.

Gate ausschließlich:
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008 --base origin/main --head HEAD --model gpt-6.1-sol
Ausgabe in fixer-r8-gate.log, echten Exit erhalten. Kein Reviewerwechsel oder Override. Bei erneutem inhaltlichem BLOCK stoppen, Ursache und kleinsten Lösungsweg an Eltern melden. Keine zusätzliche unbesprochene Fixrunde.

## 5. Routing und Sicherheitsgrenzen

Elternsession c5d0a48e-64b8-4232-9b24-fa23316782b7 ist alleiniger Statusproduzent, Paket youtube Versuch 3, Statuspfad status/youtube/3/. Eigener T3-Thread bleibt 022314b5-fac1-41c2-abe7-e6c7673b0762, Hauptorchestrator d264f838-4a47-4b9e-9bf2-12efa37223f7. Keine neuen Threads. Rückfragen ausschließlich an Eltern per eigener nativer Agentennachricht; keine ListAgents oder fremde SendMessage-Aufrufe.

Graphify vor jeder Codesuche, Skill code-suche lesen/laden und query/affected/explain nutzen, dann gefundenen Code lesen. Geheimnisse niemals lesen/ausgeben/ablegen, keine ENV-Dateien, /proc/cmdline oder Prozess-ENV. Alten widerrufenen Harness-Zugang nicht prüfen. Keine Nutzer-/Community-Daten an Modelle. Kein Modell-/Providerwechsel, kein Sonnet oder Fable-Unteragent. Produktiver Code Rust. Texte mit echten Umlauten, keine Em-Dashes. Moli ist der einzige freigegebene Browser; Brave niemals starten/übernehmen/Fallback. Vor Browserarbeit wäre /home/nathanael/Documents/claude-config/wissen/agent-browser.md Pflicht, aber hier ist keinerlei Browserarbeit autorisiert.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008
