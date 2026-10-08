[Orchestrator]
# Enger Restfix nach Gate 9

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008

## Ziel und Vertrag

Hauptorchestrator autorisiert ausdrücklich genau den verbleibenden Fund aus fixer-r8-gate.log. Kein Neuaufbau, keine Planung oder weitere interne Freigaberückfrage. Lies AUFTRAG.md, den aktuellen Gatebeleg pruefung/gate-round9.txt und die erhaltenen Quellen. FIX-GATE8.md enthält fortgeltende Build-/Sicherheitsgrenzen, ist aber keine Aufforderung, dessen abgeschlossenen Fix neu zu bauen. Alle bisherigen Fixes erhalten.

Bei ID-loser Zuordnung muss der bestehende Teile-/Vollständigkeitsvertrag auch den Einzelteilschutz begrenzen. youtube_part_confirmed und part_processed brauchen dieselbe sichere Quellen-, Dauer- und Gesamtteilbindung. Ein gleicher Index bei widersprechender Gesamtteilzahl oder unzureichender Dauer ist kein bestätigtes Backup. Vorhandenen gemeinsamen Vertrag verwenden beziehungsweise beide bestehenden Prädikate entsprechend angleichen, keine neue Architektur. Tatsächlich gespeicherte Video-IDs weiterhin nach bestehendem Identitätsvertrag behandeln. Den korrekten Erhalt gültiger processed-Evidenz nach bloßem Prüfungsfehler oder unvollständiger Suche keinesfalls verlieren.

Der Fix muss beide API-Resetzweige, Anzeige/Aktion, Vorbereitung und echten Worker-Upload-Eintritt erreichen. Bestehende reale SQL-/Workerproben gezielt um falsche Gesamtteilzahl und unzureichende Dauer bei ID-losen Teilen ergänzen, gültige Zuordnungen und Fehlerfolge weiterhin schützen. Bestehender terminaler Abschluss/Cleanup ohne erfundene historische Felder bleibt erhalten. Keine neue Funktion, Tabelle/Migration, OAuth-/Providerstrecke, Queue-Neugestaltung oder UI-Änderung. Keine weiteren Provider-/UI-Läufe.

## Eigentum

Genau ein frischer nativer Fixer, keine Unteragenten oder zusätzlichen T3-Threads. Alleinige Quellschreibhoheit für die vorhandenen Rust-Pfade, soweit für diesen Fund nötig:
- rust/crates/tb-vod-archive/src/youtube_check.rs und youtube_check_tests.rs
- rust/crates/tb-vod-archive/src/worker.rs und store.rs samt bestehenden Inline-Tests
- rust/crates/tb-dashboard-api/src/handlers/social_media_vod_archive.rs und social_media_vod_archive_tests.rs

Keine neuen Kommentare oder globale Formatierung. Eltern besitzt Taskakten, Status, Register und Live-Nachweis-SQL. Die beiden dortigen Erfolgs-Joins sind bereits durch Eltern an dieselben source_twitch_id-/source_duration_sec-Prüfungen wie die API gebunden; aktuelle Fehlversuche separat authgebunden sichtbar. Nur synthetisch ausgeführt. Nicht editieren oder erneut beauftragen. Eigene Logs ausschließlich fixer-r9-*.log in dieser Task-Akte.

## Arbeitsstand

Worktree /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008, Branch feat/vod-youtube-abgleich-20261008. Erhaltene Quellen: Commit 61c57d33d4d82d9bcdf4ccce49e4f90578c6dbe1, Integration 91fccc695b7e5821bfa92186f7a551d8a9e632c3. Sieben Quellenhashes von Eltern bestätigt. Eltern sichert jetzt Gate-9-Belege, neue Freigabe und Live-SQL in einem Checkpoint. Bei Start tatsächlichen Status und HEAD prüfen. Voriger Fixer a6b10cb55b192ca0f ist vollständig beendet, keine eigene Cargo-/Gate-Queue. Nicht wiederaufnehmen oder doppeln.

Eigene geprüfte Rust-Pfade einzeln stagen und committen erlaubt, keine Elternakten einschließen. Trailer Co-Authored-By: Claude Code <noreply@anthropic.com>. Frisches origin/main regulär nur hier integrieren, Git-Schritte einzeln mit literalen absoluten Pfaden. Keine fremden Bäume, kein stash, force, add -A, Push, Main-Merge oder Deploy. Eltern übernimmt Abschluss nach ALLOW.

## Beweisziel und Gate

Erhalten: 67 Archiv- und 14 API-Proben grün, beide Clippy-Läufe erfolgreich auf vorherigem Quellenstand. Testfilter für API ausdrücklich vod_archive_management, NICHT social_media_vod_archive; dieser führte einen Null-Lauf aus. Bestehende DB token_db_youtube auf 127.0.0.1:55683, synthetische DSN postgresql://nathanael@127.0.0.1:55683/token_db_youtube. Archiv benötigt temporär reine DSN-Zeile in rust/token-db-tests.conf, API TB_TEST_DATABASE_URL; nach Lauf Datei entfernen. Keine Secrets lesen.

Prüfungen ausschließlich /home/nathanael/.local/bin/cargo-slot mit --jobs 3, jeweils ein normaler Aufruf ohne selbst gesetzte Slot-Abbruchgrenze. Archiv test -p tb-vod-archive -- --include-ignored --nocapture; API test -p tb-dashboard-api vod_archive_management -- --include-ignored --nocapture. Gezieltes rustfmt/diff --check. Archiv-Clippy --all-targets --no-deps -- -D warnings, API-Clippy --all-targets --no-deps ohne -D warnings mit ehrlicher Warnungsmeldung. Keine fremde Bereinigung, keine zusätzlichen Baselines oder Provider-/UI-Prüfungen. Exakte Befehle/Counts/Exits melden, kein Null-Lauf oder Werkzeuglimit als Erfolg.

Nach eigenem Quellencommit und regulärer aktueller Main-Integration derselbe Gate:
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008 --base origin/main --head HEAD --model gpt-6.1-sol
Log fixer-r9-gate.log, tatsächlichen Exit erhalten. Bei neuem inhaltlichem BLOCK stoppen und kurze Ursache samt kleinstem Lösungsweg melden, keine weitere Runde oder neuer Fixer. Nach ALLOW unmittelbar an Eltern, Main-/Release-/Liveabschluss einschließlich fünf Altfällen ist bereits autorisiert.

## Routing und Sicherheit

Eltern c5d0a48e-64b8-4232-9b24-fa23316782b7 bleibt alleiniger Register-/Statusproduzent, Paket youtube Versuch 3. Eigener T3-Thread 022314b5-fac1-41c2-abe7-e6c7673b0762, Auftraggeber d264f838-4a47-4b9e-9bf2-12efa37223f7. Rückfragen nur an Eltern über eigenen nativen Agentennachrichtenweg. Kein ListAgents, fremde SendMessage oder neuer T3-Thread.

Graphify vor Codesuchen, dann gefundene Stellen lesen. Keine Geheimnisse, ENV-Dateien, /proc/cmdline oder Prozess-ENV lesen/ausgeben. Kein alter widerrufener Zugang, Modell-/Providerwechsel oder Sonnet. Nur eigener bestehender synthetischer DB-Bereich, keine Produktionseingriffe. Keine fremden Prozesse/Locks oder Wrapper ändern. Rust für produktive Verarbeitung. Moli ist einzig freigegebener Browser, Brave niemals starten/übernehmen/Fallback; keinerlei Browserarbeit hier erlaubt. Keine Em-Dashes, echte Umlaute. Abschlussmeldung nennt genaue Quell-/Integrations-SHAs, Quellenhashes, Prüfungen, Gate und zurückgelassene eigene Ressourcen.
