# Fixrunde 7: Präzision des nativen Messbeweises

## Ziel und belegter Fehler

Derselbe native Kategoriesammlerauftrag. Frischer nativer Fixer, keine eigene Orchestrierungsebene und keine weiteren T3-Threads. Eigener Worktree `/home/nathanael/.worktrees/tb-kategoriesammler-legacy-merge`, eigener Branch `feat/kategoriesammler-messvertrag`, Start HEAD `c7440400ce566393b812738e53bc5d7e56153637`. Die Hauptsession besitzt Integration und Produktion.

Während der finalen lesenden Probe wurde ein tatsächlicher Präzisionsfehler des Messvertrags sichtbar. Der native Status enthielt `last_discovery_snapshot_at=2026-10-09T00:19:40.978276730Z`, die über SQLx gespeicherte category_collection_runs-Zeile `2026-10-09 00:19:40.978276+00`. PostgreSQL rundet die neunstellige JSON-Zeit beim Textcast auf Mikrosekunden, SQLx speichert die abgeschnittene Mikrosekundenauflösung. Das ursprüngliche exakte Casting-Prädikat war trotz aktiver richtiger PID, passender Lease, frischem Heartbeat und 152 Messläufen falsch. Es steht ebenfalls im produktiven native_cutover-Messbeweis des Deploy-Wrappers und kann die erfolgreiche Umschaltung zufällig nicht bestätigen.

Der rein lesende Konstantenbeweis `/tmp/tb-category-native-snapshot-precision-probe-168485db.sql` und dessen `.log` zeigt ursprünglicher Castvergleich false, vorher auf sechs Nachkommastellen abgeschnittener Vergleich true. Dies ist keine Sammlerstörung. Der Scratch-Live-Probe der Hauptsession ist bereits korrigiert, der produktive Wrapper noch nicht. Keine Community-Daten in Modellaufrufe übernehmen.

## Eigentum und Auftrag

Enger Fix des Messprädikats in `ops/systemd/deploy-twitch-release` und seiner vorhandenen Regression in `ops/systemd/test_deploy_twitch_pruefen.py`. Prädikat an die tatsächliche gespeicherte Zeitauflösung anpassen, ohne PID, Lease, Freshness, native_lease_active, laufende Lease oder Messzeitpunkt nach dem Cutover zu schwächen. Es muss weiterhin genau die zu diesem Status gehörige Messung nachgewiesen werden; keine bloße ungeprüfte Nähe oder pauschaler EXISTS-Ersatz. Keine native Bibliothek, Bot-Runtime oder Speicherarchitektur ändern.

Das Prädikat an der echten PostgreSQL-Konvertierung mit synthetischen Zeitwerten nachziehen, einschließlich Nanosekunden oberhalb der Rundungsgrenze. Der isolierte eigene Testcontainer `tb-category-wb1-db2-168485db` läuft auf Port 33100. Synthetische Test-DSN `postgres://postgres:tbtest@127.0.0.1:33100/postgres`. Nur diese Test-DB für schreibende Fixtures. Bestehende Wrapper-Suite vollständig ausführen; Bash-Syntax prüfen. Produktiver Rust-Code bleibt unverändert, kein unnötiger konkurrierender Release-Build. Bestehende Python-Tests sind Verwaltungsprüfungen, keine neue Python-Anwendung.

Diese Briefingdatei mitcommitten; REGISTER und REVIEW im eigenen Worktree knapp und wahrheitsgemäß nachziehen. Eigentum ist dieser eigene Worktree. Original `/home/nathanael/.worktrees/tb-kategoriesammler-nativ` nicht anfassen: Dort baut die Hauptsession gerade unveränderten c7440400 und wird den Watchdog-Start produktiv abschließen. Native Agenten teilen das Dateisystem, deshalb keine dortigen Schreibschritte.

## Grenzen und Gate

Keine neuen Migrationen und keine Änderungen an angewandten Migrationen. Kein Push, Deploy, Restart, produktives Schreiben, Upload, Entfernen oder Cleanup durch den Fixer. Kein Speichernachtrag in diesem Paket. Keine Secret-Inhalte lesen oder ausgeben; keine ENV-Konfiguration oder neuen Token-Dateien. Kein Force-Push oder Hook-Bypass.

Vor Bestandssuche code-suche und Graphify, vorhandener Graph `/home/nathanael/.graphify/projects/twitch-bot/graphify-out/graph.json`. Moli ist der einzige Agentenbrowser, Brave auch indirekt nicht starten; Browserarbeit ist nicht erforderlich. Persönlichen Browser und fremde Dienste unangetastet lassen. Keine neuen Kommentare in Code. Texte mit echten Umlauten und ohne Gedankenstriche.

Einziger Reviewer ist `/home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review`, Basis origin/main, HEAD, Modell gpt-6.1-sol. Gate nur in diesem isolierten Worktree. Bei BLOCK frischen nativen Fixer einsetzen und denselben Kritiker behalten, kein neues Modell für ein anderes Urteil. Keine Zwischenberichte pro Runde, Bericht bei Ende oder echtem Blocker. Commit-Trailer gemäß Nutzerregeln und zusätzlich die aktuell geltende Claude-Code-Attribution bei eigener Commit-Erstellung.

## Übergabe und Routing

Auftraggeber und Statusproduzent ist Teil-Orchestrator im Intent-Thread `0712a6dd-cf2a-4a39-907a-f50b19e7930c`. Hauptorchestrator `8604000d-b8ca-40d7-a1f9-8fe5fd44aa65`. Paket A, Fixrunde 7. Rückfragen an diese Hauptsession, nicht an den Nutzer.

Freigabepunkt: sauberer eigener Commit, tatsächlicher synthetischer PG-Beweis, bestehende Wrapper-Suite mit Zahlen, Bash-Syntax und lokaler Merge-Gate ALLOW mit gpt-6.1-sol; Prüfprozesse beendet. Danach Ursache, enger Fix, SHA, Zahlen, Gate-Wortlaut und Nachweisorte melden. Kein eigener produktiver Abschluss. Die Hauptsession integriert nach Übergabe und beweist den vollständigen finalen Stand live.
