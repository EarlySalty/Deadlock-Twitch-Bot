# Feature-Stammbaum: tatsächlicher Stand am 24.09.2026

Status: Teilimplementierung, nicht produktiv. Der aktive Generator, das Frontend und die installierten Dienste sind unverändert. Kein Merge, keine Veröffentlichung, kein Bot-Neustart.

## Implementiert und ausgeführt

`tools/roadmap-history/feature_graph.py` migriert die vorhandenen Schema-2-Daten samt abgeschlossener Twitch-Vorgeschichte in einen expliziten FeatureNode-Baum. `FeatureNode.ts` dokumentiert den Datenvertrag. Der Snapshot `legacy-history.json.gz` enthält Git-Metadaten der Twitch-Pfade, keine Quelltexte oder Zugangsdaten. Seine Prüfsumme wird beim Laden kontrolliert.

Tatsächlich ausgeführter Lauf im eigenen Worktree:

```sh
python3 tools/roadmap-history/feature_graph.py --repo /home/nathanael/repos/Deadlock-Twitch-Bot --ref origin/main --legacy-repo /home/nathanael/repos/Deadlock-Bots --legacy-ref origin/main --write-legacy-cache tools/roadmap-history/legacy-history.json.gz --output dist/roadmap-history/feature-graph.json
```

Ergebnis: 4.174 eindeutige Commits, davon 379 aus der Vorgeschichte, 907 Knoten, 52 Funktionsfamilien, 479 offene/mehrdeutige Zuordnungen. Die Ausgabe wird vor dem Schreiben gegen fehlende Eltern, Zyklen, zusätzliche Wurzeln und umgekehrte Datumsbeziehungen validiert. Der Generator prüft außerdem die vollständige Repräsentation der Commit-IDs in den Knoten.

Ursprung: `3654f6c73be53fc569da673fb307e7e0c79f2b87`, 21.09.2025, `new Twitch Bot`, Repository `EarlySalty/Deadlock-Bots`. Der Snapshot endet mit der Auslagerung am 24.02.2026. Aktueller Datenstand: `901744c609ba677558804db504a7ec79de386588`.

Kleine Änderungen werden nach Funktion, Art und Pflege-Status in festen 14-Tage-Fenstern gebündelt. Die Originalmeldungen und Hashes bleiben erhalten. Reine FeatureNode-JSON-Eingaben können ohne Zugriff auf die Legacy-Quelle validiert werden. Ausgabe-Dateien werden atomar ersetzt; eine fehlgeschlagene Validierung lässt die vorherige Ausgabe stehen.

## Testnachweise

- `python3 -m unittest discover -s tools/roadmap-history -p 'test_*.py' -v`: 47 bestanden, 0 fehlgeschlagen, 0 übersprungen. Darin 16 neue Parserprüfungen und 31 bestehende Prüfungen.
- `node --test tools/roadmap-history/model.test.mjs tools/roadmap-history/family.test.mjs`: 31 bestanden, 0 fehlgeschlagen, 0 übersprungen. Diese prüfen ausdrücklich das unveränderte bisherige Frontend-Modell, nicht die noch fehlende neue Ansicht.
- Der vollständige reale Migrationslauf war erfolgreich. Kein Beispielauszug als Ersatz verwendet.
- Keine neuen Browser- oder Live-Nachweise. Keine Produktionsdaten verändert.

## Hindernisse und fehlende Integration

Die MCP-Sicherheitsprüfung hat den Schreibaufruf für den neuen Frontend-Graphen sowie einen kombinierten Status-Leseaufruf verweigert. Die abgewiesenen Aktionen wurden nicht über andere APIs erzwungen. Die zuvor ebenfalls blockierte komprimierte Paketübertragung wurde verworfen; die unvollständige temporäre Datei wurde entfernt.

Der unabhängige Opus-4.8-Autor scheiterte vor der Bearbeitung an einer abgelaufenen Anmeldung. Der Ersatz-Thread mit GPT 5.6 Sol scheiterte ebenfalls vor der Bearbeitung am Codex-Kontingent. Beide Threads sind beendet und gesettelt. Die tatsächliche Parser-Implementierung und die Tests erfolgten in dieser Hauptsession; keine unabhängige Abnahme behauptet.

Offen: neuer Frontend-Renderer, Such- und Datumsfilter gegen den neuen Graphen, Integration in `tools/generate_roadmap.py`, Installer/Timer-Anpassung, Browserprüfung mit vollständigen Daten, unabhängige Abnahme, PR-Abnahme und Veröffentlichung.

Zusätzlich gilt die geladene zentrale Entscheidung `orchestrierung/PR-FIRST-TESTBETRIEB.md` vom 24.09.2026. Sie verlangt eine versionierte Folgeentscheidung für das Ende des Testhalts. Schutz-Hooks und diese zentrale Policy wurden nicht verändert. Die aktuelle Nutzerforderung nach Deployment ist im Register als Konflikt festgehalten.

MERGEPROTOKOLL[MS-1]: kein Merge: PR-first-Testbetrieb; Teilimplementierung
LIVEBEWEIS[DV-1]: nicht ausgeführt: PR-first-Testbetrieb; aktive Seite unverändert
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: repo-nahe Aufgabenakte
