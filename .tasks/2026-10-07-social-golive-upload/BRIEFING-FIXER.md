# Frischer Fixer für Auftrag D nach Gate-BLOCK

## Auftrag und Grenze

Auftraggeber: Intent-Thread d3a1741e-82bc-4a48-865b-2845c663dca7. Ein frischer Fixer übernimmt genau die Mängelliste aus REVIEW.md. Kein weiterer Orchestrator, keine eigenen Review-Threads, keine zusätzlichen Worker. Routing autonom nach der Pyramide durch den Auftraggeber.

Bestehender Branch: fix/social-golive-upload-texte.
Bestehender Worktree: /home/nathanael/.worktrees/tb-social-golive-upload.
Geprüfter Codekandidat: f2b64b39706417ca63071e5fcf8cc857ab9dff01.
Basis: origin/main e0b0dbaf662d7680c4ceaa210bf15f1443693cd8.
Der Übergabecommit danach enthält ausschließlich Register, Briefing und Nachweise. Keine Behebung der Gate-Funde durch den ursprünglichen Implementierer.

## Beheben

1. Wartende Aufträge dürfen neuere ausführbare Aufträge nicht dauerhaft verdrängen. Der Gate hat das konkrete 100-Aufträge-Szenario bestätigt. Gemeinsamen Queue-Reader und begrenzten Worker-Scan zusammen korrigieren. Eine separate, faire Verbindungsprüfung mit unabhängiger Auswahl regulärer pending-Aufträge ist eine mögliche Lösung. Nicht bloß das Limit erhöhen. Nachweisen, dass neuere verbundene Aufträge erreicht werden und ältere Warteaufträge nach einer Verbindung weiterhin wiederaufgenommen werden. Keine echten Konten oder Produktionsaufträge dafür ändern.
2. Fehlende und unvollständige Verbindungen auch im englischen Dashboard verständlich anzeigen. Strukturierte Gründe verwenden; keine Tests am Wortlaut von Produkttexten. Bestehende deutsche Fehlermeldungen und gespeicherte Altaufträge berücksichtigen.
3. Dem Gate den bereits vorhandenen visuellen Nachweis zugänglich machen. Vier tatsächliche Viewportaufnahmen liegen in `/home/nathanael/.claude/sichtpruefung/social-golive-upload/`: `upload-wait-1440.png`, `upload-wait-390.png`, `upload-connections-1440.png`, `upload-connections-390.png`. `browser-state.json` und Browserlogs ergänzen DOM- und Funktionsnachweise. Bei Bedarf die relevanten Bild-/DOM-Artefakte für den nächsten Gate versionieren. Nicht behaupten, ein Screenshot existiere nur aufgrund einer Assertion.

## Bestehende Produktentscheidungen erhalten

- Trennen löscht die eigene gespeicherte Verbindung und widerruft beim Anbieter, soweit unterstützt. Nur ein serverseitig geprüfter Admin darf eine Sammelverbindung löschen. Partner bekommen den Weg zum eigenen Konto. Keine echten Streamer-Zugänge trennen.
- TikTok Direct Post und dessen Rechtstexte gehören Auftrag E; nicht selbst umstellen. Bei Integration von E das Fähigkeitsmerkmal an die tatsächlich implementierte Versandart anpassen.
- Anreicherung und Transkription sind durch integrierten Auftrag F abgeschaltet. Nicht wieder aktivieren. Das Startlog zählt nun fünf unabhängige Worker plus vier bei verfügbarer Verschlüsselung.
- Tokenablauf und DMs gehören Auftrag A. Bei frischem origin/main dessen Fehlerpropagierung im Statuspfad und stabile ID-Verträge erhalten, insbesondere beim Zusammenführen von credentials.rs.
- Produktiver Backend-Code nur Rust. Keine neuen Python-Skripte und keine Code-Kommentare. Migrationen nur neu, bestehende Migration 20261007213000 noch nicht in Produktion und nach erster Produktionsanwendung unveränderlich. SQLx-Artefakte und Snapshot mitführen.

## Nachweise und Werkzeuge

Compiler nach Integration einschließlich tb-bot: Exit 0. Social-Media-Bibliothek seriell 309/309, API-Handler 48/48, Fresh-Schema 1/1, Dashboard-Verträge 45/45, Chromium 25/25. Paralleler TikTok-Timeouttest scheiterte einmal; der vollständige serielle Nachlauf bestand ohne Teständerung oder Skip. Vollständige API-Suite vor Integration: Basis und Änderungsstand jeweils 1309 bestanden und dieselben 22 Fehler. Clippy: Basis und integrierter Stand dieselben zwei Befunde in analytics.rs:321 und vocab.rs:164. EVIDENCE.md beschreibt Grenzen und Befehle.

Isolierte echte Testdatenbank: `postgresql://nathanael@127.0.0.1:55437/tb_social_upload_test`. `PATH=/home/nathanael/.cargo/bin:$PATH`, `SQLX_OFFLINE=true`, `TB_TEST_DATABASE_URL` für Bibliotheks-/API-Tests, `TEST_DATABASE_URL` für Fresh-Schema. `--include-ignored`; den gesamten Social-Media-Lauf mit `--test-threads=1` reproduzieren. Edition 2021 für rustfmt. Keine Tests gegen Produktionsdatenbanken.

Graph zuerst: kanonischer Graph unter `/home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json`. Hauptcheckout ist fremd verändert und bleibt unangetastet.

## Freigabepunkte und Abschluss

1. Vor Gate frisches origin/main integrieren. Lokaler Gate bleibt einziger Reviewer. Folgerunde mit demselben Modell wie Runde 1: gpt-6.1-sol. Kein Wechsel nach inhaltlichem BLOCK. Bei weiterem BLOCK wiederum neuen Fixer durch den Auftraggeber zuweisen lassen.
2. Nur nach ALLOW normal über `origin HEAD:main` integrieren. Kein Force-Push, kein Hook-Bypass, kein git add -A. Jeder Git-Schritt einzeln und mit literalen absoluten Pfaden.
3. Release erst aus sauberem, committed und aktuellem origin/main-Stand im eigenen Worktree bauen. Der gestoppte Rust-Release-Build ist unvollständig und darf nicht ausgeliefert werden. Cargo-Pakete für Installer: tb-bot, tb-dashboard, tb-stream-audit-bin, tb-config, tb-llm, tb-category-collector. Binaries: tb-bot, tb-dashboard, tb-stream-audit, tb-config-check, tb-llm-usage-recover, tb-category-collector, tb-twitch-watchdog, clip_context_learn. ELF-Revision für jeden auf den tatsächlichen SHA prüfen; alle drei Frontends bauen.
4. Deploy-Wrapper braucht einen eigenständigen Clone mit internem .git-Verzeichnis. Verifizierte Artefakte dorthin übertragen. `deploy-twitch-release <aktueller-40-SHA> <Clone> --restart twitch-bot --restart twitch-dashboard`. Wrapper wendet Migrationen vor Neustart an. Vorherige PIDs zuletzt 1184203 (Bot), 1184089 (Dashboard); vor eigenem Deploy erneut messen. Live-Proof einschließlich Schema, PIDs, exe, Journal und UI liefern. Kein Produktionsschreibzugriff für Testzwecke.
5. Nach geprüftem Merge und Live-Beweis Branch/Worktree entfernen. Eigene Baseline-Sicherung c336249afcfbb2f11f5b8dad6cbad3021815651f ist wiederhergestellt und committed, liegt noch als Stash; nach geprüftem Abschluss genau diese Sicherung entfernen, fremde Stashes erhalten. Pflichtnachweise ergänzen, an den bestehenden Auftraggeber melden und eigenen Thread gemäß Regel abschließen.

## Aktueller Zustand

Kein Push, kein Merge, keine Produktionsmigration, kein Deploy, kein Neustart. Keine weitere Hintergrundaufgabe des ursprünglichen Implementierers offen. Branch, Worktree, Testcluster, Screenshots und Teilcache bleiben für die Übernahme erhalten. Der Original-Worker startet keinen zusätzlichen Thread und behebt die Funde nicht selbst.
