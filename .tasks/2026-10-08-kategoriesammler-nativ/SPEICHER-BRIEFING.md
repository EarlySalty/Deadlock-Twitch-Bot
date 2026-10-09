# Auftrag B: Speichernachtrag nach belegtem nativen Abschluss

## Rolle, Start und Freigabe

Du bist der native Rust-Implementierer für den ausdrücklich beauftragten Nachtrag, kein weiterer Teil-Orchestrator. Auftraggeber ist Teil-Orchestrator im Intent-Thread 0712a6dd-cf2a-4a39-907a-f50b19e7930c; Hauptorchestrator 8604000d-b8ca-40d7-a1f9-8fe5fd44aa65. Keine zusätzlichen T3-Threads und keine Session-zu-Session-Koordination. Native frische Fixer in der Gate-Schleife sind erlaubt.

Eigener exklusiver Worktree `/home/nathanael/.worktrees/tb-kategoriesammler-speicher`, Branch `feat/kategoriesammler-speicher-20261009`, tatsächlicher Start HEAD `14eedf4aa602ba835b3619d9f4d8bcfd16351a53`. Der nativen Phase steht kein laufender Compiler oder Fixer mehr entgegen. Der Produktivcode e0e9fde2 ist mit eigenen Neustarts, korrekter nativer Lease, neuen Messungen und erfolgreichen Watchdog-Timerläufen live belegt. Siehe LIVE-ABSCHLUSS.md und dessen Originalbelege. Die beiden alten eigenen nativen Worktrees und deren Folgebranches sind entfernt. Das frühere Original `/home/nathanael/.worktrees/tb-kategoriesammler-nativ` existiert nicht mehr und darf nicht wiederangelegt werden.

Die Hauptsession besitzt Integration und sämtliche produktiven Schritte. Dein Freigabepunkt ist ein sauberer kohärenter Rust-Kandidat, tatsächlicher Compilerbeweis, nachgezogene vorhandene Suites mit Zahlen, synthetischer Verlustfreiheits- und Archivbeweis sowie lokaler Merge-Gate ALLOW mit gpt-6.1-sol. Kein Push nach main, Deploy, produktives SQL-Schreiben, realer Upload, realer Rückimport, reales Entfernen, Schlüsselbereitstellung oder Cleanup durch dich. Synthetische Artefakte und Test-DB-Writes sind erlaubt. Diese Briefingdatei gehört in deinen Commit.

## Verbindlicher Vertrag und Eigentum

Lies AUFTRAG.md, NACHTRAG-1-SPEICHER.md und LIVE-ABSCHLUSS.md. Implementiere den gesamten dort beauftragten Speichernachtrag als direkte Fortsetzung. Eigenes Schreiben umfasst die zusammengehörigen nativen Writer-, Schema-, Snapshot-, Report-, Manifest-, Krypto-, Konfigurations-, CLI-, Rollen-, Archiv- und technischen Dokumentationspfade für die Kategoriesammlung. Keine parallelen Implementierer für dieselben Dateien, keine zusätzlichen Refactorings und kein Umbau des VOD-Archivs. Bestehende YouTube- und fremde Featurearbeit auf main bleibt erhalten.

REGISTER.md gehört während deines Laufs der Hauptsession. Nicht dort schreiben. Führe den kleinen eigenen Stand in SPEICHER-WORKER-STATUS.md und Gate-Funde in SPEICHER-REVIEW.md; halte die nötigen Nachweise lokal. TODO.md gehört dem Aufgabenstand-Agenten und bleibt unangetastet. Die Hauptsession ändert bis zur sauberen Übergabe keinen deiner Produktivpfade.

## Snapshot-Vertrag

Normalisiere neue Writes und den gesamten Altbestand seit 2026-09-27 verlustfrei. Die zwölf tatsächlichen SQL-Spalten sind maßgeblich: snapshot_at, stream_id, user_id, user_login, viewer_count, title, language, started_at, tags, thumbnail_url, is_mature und sample_seconds. Statische Daten werden bei Änderung versioniert; pro Messung bleiben Zeit, Stream, Betrachterzahl und die nötigen Verweise beziehungsweise veränderlichen Messwerte. Login-Historie, Tagreihenfolge, NULL-Verhalten, Zeitauflösung und exakte sample_seconds dürfen nicht verloren gehen.

Der vorhandene store_snapshot und der Report müssen direkt mit dem neuen Format arbeiten. Sorge für einen wiederanlaufbaren Übergang, der auch laufende Writes vollständig erfasst. Keine dauerhafte Doppelstruktur. Die alte Form darf erst nach vollständiger zeilenweiser Rekonstruktion sowie Zeilenzahl und Prüfsumme je UTC-Tag entfernt werden. Liefere den Rust-CLI-Pfad für Backfill und Beweis, seinen synthetischen echten DB-Beweis sowie eine produktiv sichere Reihenfolge. Keine alte Tabelle im initialen Schema-Rollout ungeprüft löschen. Eine reine Stichprobe oder Größenersparnis ist kein Verlustfreiheitsbeweis.

## Archiv- und Partnervertrag

Partner-Chat und Partner-Snapshots bleiben lokal. Maßgeblich sind stabile Twitch-User-IDs aus dem vorhandenen aktiven Partnermodell, nicht Logins. Die produktive View verwendet derzeit das ganzzahlige is_partner=1. Diese Eigenschaft und der genaue vorhandene Vertrag sind vor dem Neubau anhand des Codes zu prüfen. Partnerstatus unmittelbar vor einer lokalen Entfernung erneut prüfen; Wechsel und Wiederanlauf dürfen keinen Partnerbestand verlieren.

Nichtpartner-Chat und Nichtpartner-Snapshots werden pro abgeschlossenem UTC-Tag und Datenart komprimiert und clientseitig verschlüsselt nach dem bestehenden rclone-Remote gdrive: ausgelagert. Für twitchbot wurde der Remote bereits metadata-only bestätigt. Kein zweiter Google-Connector und keine kopierten Zugangsdaten. Der Schlüssel kommt aus Infisical über den vorhandenen Bot-Secret-Weg; keine Schlüssel- oder Token-Dateien. Google erhält Chiffretext. Lokale temporäre Dateien und Speicherverbrauch bleiben begrenzt und geschützt; keine ganze Mehrmillionen-Tagestabelle im RAM materialisieren.

Dauerhaftes PostgreSQL-Manifest: Tag, Datenart, objektbezogener Dateipfad, Zeilen, Prüfsumme und wiederanlaufbarer Zustand. Erst vollständigen Export herstellen und tatsächlichen Upload prüfen, danach eng begrenzt lokal entfernen. Bei Fehlern bleibt der lokale Bestand liegen. Entprellte Meldung höchstens einmal täglich. Keine pauschalen Rohchat-DELETE-Rechte für den Bot. Das lokale Entfernen braucht einen eigenen engen, wiederanlauffähigen und auf bestätigte Manifeste beschränkten Pfad. Kein unbestätigtes Näherungsprädikat und keine Alterslöschung außerhalb verifizierter Auslagerung.

Die IRC-Tags zuerst über sämtliche bestehenden Auswertungspfade bestimmen, dann auf die ausgewerteten Felder reduzieren. Der Tages-Rückholweg ist ein Rust-CLI-Unterbefehl und überprüft Integrität. Bestehende CLEARMSG, zielbezogene CLEARCHAT, Shared-Chat-Kopien, Raumsperren und Zielmarken bleiben wirksam. Verspätete Zustellung und Rückholung dürfen bereits gelöschte Inhalte nicht wiederherstellen.

Stundenaggregate und Kennzahlen bleiben für Partner und Nichtpartner lokal. Das heutige flush_rollups rekonstruiert aus lokalen Rohzeilen und darf nach Auslagerung keine erhaltenen Aggregate auf Null oder Teilmengen reduzieren. Das heutige report berechnet Sprach- und Kanalmetriken aus vollständigen Snapshots. Deren Entfernung benötigt einen gleichwertigen lokalen Kennzahlenpfad. Belege denselben Report vor und nach einem synthetischen Archivlauf.

Vor dem ersten echten lokalen Entfernen muss die Hauptsession den Trockenlauf des fertigen Rust-Programms mit aktuellen Zeilen und Bytes je UTC-Tag, getrennt nach Partnerstatus und Datenart, vorlegen. Implementiere einen nachweislich nicht schreibenden Trockenlauf. Die früheren vorläufigen SQL-Zahlen sind keine Freigabe für echtes Entfernen. Die geteilte Google-Client-ID bleibt als offenes Risiko im Abschlussbericht; ihr Ersatz gehört nicht zu diesem Auftrag.

## Vorhandene Referenzen

Vor Bestandssuche code-suche und Graphify. Vorhandener Twitch-Graph `/home/nathanael/.graphify/projects/twitch-bot/graphify-out/graph.json`; keine volle Extraktion. Grep erst nach Graphify und zum Nachlesen der Fundstelle.

- rust/crates/tb-analytics/src/category.rs: Rohchat, Snapshot-Writer, Rollups, Report und Redaktionen.
- rust/bin/tb-category-collector/src/lib.rs: native Sammlung, Shutdown und Speichergrenzen.
- rust/bin/tb-bot/src/user_id_backfill.rs: bestehende stabile Partner-ID-Zuordnung.
- rust/bin/tb-stream-audit/src/main.rs: nach_drive_archivieren_mit, drive_archiv_durchfuehren und bestehender rclone-Argumentbuilder. Referenz, keine Freigabe für VOD-Änderungen.
- rust/crates/tb-crypto/src/field.rs: AES-256-GCM, AAD, Schlüssel-ID und Zeroisierung. Die öffentliche Feldschnittstelle arbeitet bisher mit UTF-8 und ist nicht automatisch ein geeigneter komprimierter Streaming-Archivpfad.
- rust/crates/tb-config/src/global.rs und rust/bin/tb-bot/src/main.rs: öffentliche Betriebskonfiguration und bestehender benannter Secret-Getter beziehungsweise runtime_cipher. Vorhandene Infisical-Hydration wiederverwenden. Eine ungeprüfte Option<Arc<_>> nicht als vorhandenen Schlüssel behandeln.
- /tmp/tb-category-storage-preparation-168485db.md und /tmp/tb-category-storage-vorlaeufiger-trockenlauf-168485db.md: Vorbereitungsfakten, keine produktive Umwandlung oder Uploadfreigabe. Pfade und Eigenschaften vor Verwendung gegen aktuellen Code prüfen.

## Prüfung, Sicherheitsgrenzen und Übergabe

Produktiver Laufzeitcode ist Rust. Keine neuen Python-Features, keine Code-Kommentare, ENV-Dateien oder ENV-Konfiguration. Vorhandene kleine Verwaltungs- und Testskripte sind nur dort zulässig. Geheimnisse nicht lesen, ausgeben oder an Modelle senden; Nutzer- und Community-Inhalte nie in Modellaufrufe oder an externe Anbieter geben. Kein Bot-Konto im anonymen IRC-Lesepfad, keine Sende- oder Moderationsaktionen daraus. Speicherbudget und Plattenreserve bleiben wirksam.

Rust 1.97.1 über /home/nathanael/.local/bin/cargo-slot +1.97.1, SQLX_OFFLINE=true, --jobs 3. Kein globales CARGO_TARGET_DIR. Releases nur in diesem eigenen sauberen Worktree und keine Quellen während eines aktiven Builds ändern. Der isolierte eigene Testcontainer tb-category-wb1-db2-168485db läuft nach bestätigter metadata-only-Prüfung auf 127.0.0.1:33100. Synthetische DSN postgres://postgres:tbtest@127.0.0.1:33100/postgres; tbtest ist ein Testkennwort. Ausschließlich dieser Testbestand für schreibende DB-Prüfungen, weder Produktiv-DB noch Ports 5433 oder 5434. Container nicht entfernen, er bleibt beim Auftrag.

Neue additive Migrationen erst nach frischem origin/main nummerieren. Produktiv angewandte Dateien, insbesondere 20261008003000, 20261008160000 und 20261008161000, sind eingefroren. .sqlx und den vorhandenen Schema-Snapshot passend pflegen. Vorhandene Suites nachziehen, Zahlen einschließlich ignored und failed nennen. Keine pauschale grüne Workspace-, Bot-Clippy- oder Dashboard-Gesamtsuite behaupten. Die bekannten roten fremden Baselines sind dokumentiert und keine Freigabe, weitere Fehler zu erzeugen.

Einziger Reviewer ist /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <dieser Worktree> --base origin/main --head HEAD --model gpt-6.1-sol. Kein eigener Review-Thread. Bei BLOCK je Runde frischen nativen Fixer starten und denselben Kritiker behalten; keine Zwischenberichte pro Runde. Wenn nach fünf erfolglosen Runden tatsächlich festgefahren, eng begrenzten Blocker melden. Gate-Diff unter ungefähr 150 KB halten; bei nötiger Paketgrenze kohärenten additiven Zwischenstand melden, nichts ungeprüft produktiv schalten.

Moli ist der einzige Agentenbrowser; Brave auch indirekt nicht starten. Vor Browserarbeit /home/nathanael/Documents/claude-config/wissen/agent-browser.md lesen. Browserarbeit ist für diesen Backend-Vertrag nicht grundsätzlich nötig. Persönlicher Browser, fremde Worktrees und Dienste, ai-coach und laufende Streams bleiben unangetastet. Kein Force-Push, Hook-Umweg, geteiltes stash oder git add -A. Git-Schreibschritte einzeln, nur eigene Dateien. Kein Sonnet, kein Fable-Subagent und keine neue Orchestrierungsebene. Texte auf Deutsch mit echten Umlauten und ohne Gedankenstriche; Commit-Trailer nach den Nutzerregeln und zuletzt Co-Authored-By: Claude Code <noreply@anthropic.com>.

Übergabe: sauberer SHA und tatsächliche Prüfzahlen; eng begründete Schema-, Partner-, Aggregat-, Redaktions-, Manifest- und Krypto-Verträge; Gate-Wortlaut; synthetische Verlustfreiheits- und Wiederanlaufnachweise; sichere produktive Reihenfolge und benannte nötige Schlüsselbereitstellung. Echte produktive Rekonstruktion, Tageszahlen, Uploadprüfung und Entfernung übernimmt die Hauptsession nach Übergabe. Eigene Prüfprozesse müssen bei der Übergabe beendet sein.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-kategoriesammler-speicher
