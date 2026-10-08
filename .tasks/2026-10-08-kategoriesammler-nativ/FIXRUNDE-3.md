# Fixrunde 3: Ehrliche Ausfallmeldung

## Ziel und Vertrag

Du bist der frische native Fixer für die dritte Runde des bestehenden Auftrags. Lies AUFTRAG.md, REGISTER.md und REVIEW.md im selben Ordner sowie `/tmp/tb-category-gate-fix2-gate.log`. Behebe beide offenen BLOCK-Funde samt eng zugehörigen Hinweisen. Das Ziel ist eine native Sammlung ohne falsche Ausfallmeldungen. Bereits beendete Messlücken müssen als beendet erkennbar sein. Historische Lücken auf das tatsächliche Beobachtungsfenster begrenzen, erklärte Pauseintervalle weiterhin vereinigt abziehen und unerklärte Restlücken erkennen.

## Eigentum und Arbeitsstand

Worktree: `/home/nathanael/.worktrees/tb-kategoriesammler-nativ`.
Detached HEAD: `ac8c61b58e1e28dcec312b4bb4f1d0ba2da1684a`.
Der Arbeitsbaum war sauber. Der Teil-Orchestrator hat anschließend ausschließlich REGISTER.md ergänzt und REVIEW.md sowie diese Briefingdatei angelegt. Diese vorbereiteten Auftragsdateien gehören zum selben Paket und dürfen mitcommittet werden.

Du bist der einzige Produktivschreiber. Zuständig: Watchdog und eng zugehörige Prüfungen, die noch nicht produktiv angewandte Native-Migration, erforderliche Artefaktpfad-Korrektur in Deploy/Installer samt vorhandenen Prüfungen und passender Doku. Keine globale Formatierung, keine unabhängigen Refactorings, keine Cloud-Implementierung. Keine Änderung angewandter Migrationen. Produktiver Code ausschließlich Rust. Kein pauschales Rohchat-UPDATE/DELETE/TRUNCATE; enges Dirty-Queue-Spaltenrecht prüfen. Verschlüsselungsnachtrag bleibt bis nach nativem Live-Beweis ungeöffnet als Umsetzung.

Eigene Dateien gezielt committen, Git-Schritte einzeln mit literalem absolutem Pfad. Nutzertrailer: `Co-authored-by: GPT 6.1 Sol <modell@local>`. Kein Push, Merge, produktive Migration, Deploy, Dienstneustart oder Cleanup produktiver Dateien. Der Teil-Orchestrator übernimmt diese Schritte.

## Beweisziel

Vor Code-Suche Skill `code-suche` beachten und globalen Graph fragen. Lokaler Worktree-Graph fehlt; `/home/nathanael/.graphify/global-graph.json` funktioniert. Danach gefundene Stellen lesen. Prüfe die Watchdog-Fehlerfamilie zusammenhängend: Beobachtungsfenster, Pauseunion, laufender versus beendeter Vorfall, Auswahl und Meldeinhalt, Entprellung, Tagesgrenze, Wiederholungen und Zeitformat. Keine neue Review-Session, einziger Kritiker bleibt Merge-Gate.

Werkzeugkette Rust 1.97.1, Workspace-Edition 2021, Cargo über `/home/nathanael/.local/bin/cargo-slot +1.97.1`, höchstens drei Jobs. Kein globales CARGO_TARGET_DIR. SQLX_OFFLINE=true. Der Teil-Orchestrator hat bereits einen laufenden beziehungsweise slotwartenden Check für tb-bot und tb-category-collector all-targets, Task `bkwxoztyj`, Log `/tmp/tb-category-native-current-main-check-retry.log`; diesen nicht duplizieren oder stoppen. Eigene fokussierte Watchdog-Prüfung einmal passend fahren. Kein neuer Testzwang, bestehende beschädigte Suites nachziehen. Bestandene, fehlgeschlagene und ignorierte Tests ehrlich zählen. Ein wartender leerer Cargo-Log ist kein Testbeweis.

Eigener Test-Postgres, sofern noch vorhanden: Container `tb-category-wb1-db2-168485db`, Port 33100, DSN `postgres://postgres:tbtest@127.0.0.1:33100/postgres`, TB_TEST_REQUIRE_DB=1. Ausschließlich künstliche Daten. Produktionsdaten und Secrets nicht lesen oder ausgeben. Keine ENV-Dateien, Infrastruktur-Secrets nur bestehender Weg.

Rückkehr zu einer älteren Release-Version: Basis-Vite-Artefaktpfad gegen `required_artifacts` und Installerauswahl nachweisen, ohne produktive Umschaltung. Vorhandene Release-Artefakte nicht nach Alter beurteilen. Falls eine rein pfadbezogene Kompatibilitätskorrektur genügt, passend vorhandene Wrappertests nachziehen. Keine zusätzlichen T3-Threads oder fremden Sessions.

Nach den Fixes sauber committen und selbst den Gate fahren:

`python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-kategoriesammler-nativ --base origin/main --head HEAD --model gpt-6.1-sol`

Bei erneutem BLOCK nicht im selben Kontext weiterfixen. Sichere den Stand und liefere den exakten Gate-Wortlaut samt neuen Funden für den nächsten frischen Fixer. Bei ALLOW liefere SHA, Arbeitsbaumstatus und Nachweisorte. Keine Zwischenberichte je Einzelprüfung.

## Routing und Grenzen

Auftraggeber und Statusproduzent: Teil-Orchestrator im Intent-Thread `0712a6dd-cf2a-4a39-907a-f50b19e7930c`. Hauptorchestrator `8604000d-b8ca-40d7-a1f9-8fe5fd44aa65`. Berichte als natives Agent-Ergebnis an diese Hauptsession, nicht über ListAgents, SendMessage oder neue T3-Threads. TODO.md unangetastet. Das native Agent-Modell erbt `gpt-6.1-sol`.

Keine Browserarbeit erforderlich. Falls unerwartet nötig, zuerst `/home/nathanael/Documents/claude-config/wissen/agent-browser.md` lesen und ausschließlich `/home/nathanael/.local/bin/moli` benutzen. Brave weder direkt noch indirekt starten. Der alte automatisierte Browserhelfer verwendet Brave. Gate daher ausschließlich im isolierten Worktree, nie im kanonischen Checkout. Keine Hooks ändern oder umgehen. Persönliche Browser, fremde Worktrees, Builds, Locks und Dienste unangetastet. `ai-coach` nicht anfassen oder melden. Nutzertexte auf Deutsch mit echten Umlauten, keine Gedankenstriche und keine neuen Code-Kommentare.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-kategoriesammler-nativ
