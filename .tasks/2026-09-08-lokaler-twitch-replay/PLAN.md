status: abgeschlossen
datum: 2026-09-08

# Umsetzung des genehmigten Versuchs

ORCHESTRIERUNG[OR-1]: Klasse mittel | Phase abgeschlossen | Artefakt: .tasks/2026-09-08-lokaler-twitch-replay

## Aktueller verbindlicher Stand nach Nutzer-Go

Der Coding-Blocker ist durch die aufgabenbezogene Codex-Ausnahme aufgelöst. Der Runner wird ausschließlich für die zwei lokalen Kandidaten gebaut und ausgeführt. Kein Cloud- oder Infisical-Code, kein Datenexport. Der Root hat den lokalen Versuch innerhalb des ausdrücklich genannten „oder“ gewählt. Historische Blocker und vorläufige Cloudpläne sind überholt. Pro Modell: 50 Fälle plus dieselben zehn Baselines, 160 Ausgabetokens, Temperatur 0,4 und 600 Sekunden Frist. Fälle werden vor dem ersten Aufruf vollständig auf Identität, Zeiten, Wissensdatum, Kanaltrennung und Prüfsumme geprüft. Referenzen und Bewertungsmetadaten kommen nicht in die Generatorfunktion. Technische Fehler bleiben im Nenner und werden vollständig protokolliert.

Der erste Testlauf vor Implementierung war rot: `cargo test -p tb-llm --features local-eval --test local_eval`, fehlender Bin und `unresolved import tb_llm::local_eval`. Nach der Implementierung bestanden die gezielten Prüfungen. Unabhängige Rust-/Sicherheitsreviews erfolgten vor den echten Läufen und gaben den Code frei.

## M1: Lokaler Connector

- Standardmäßig ausgeschaltetes Feature local-eval und typisierter Loopback-Endpunkt in tb-llm; zentrale HTTP-Funktion wiederverwenden, kein Cloud-Fallback, kein Ledger, Proxy und Redirect aus.
- Zuerst aussagekräftige Tests für unzulässige Ziele, Modell-/Scopegrenze, leere Antwort und Fail-closed schreiben.
- Prüfung: cargo test -p tb-llm --features local-eval; bestehende Produktions-Guards bleiben grün.
- Stop-Regel: Ein nichtlokales Ziel oder ein Produktionsmodellwechsel ist möglich.
- Status: implementiert. Typisierter Loopback-Client im bestehenden Hub, Feature standardmäßig aus. Alter Coding-Blocker durch Nutzer-Go aufgelöst. Produktionsanbieter und bestehende Guards unverändert.

## M2: Privater Replay und Bericht

- Strikte Schema-/Zeit-/Identitätsprüfung vor jedem Modellaufruf. Referenzen werden nie in den Prompt serialisiert. Baseline und Variante erhalten identischen vorherigen Kontext.
- Config-Datei mit Laufpfaden, Modellalias, Zeit-/Tokenbudget und Umfang. Private Outputs mit create_new und 0600, Verzeichnis 0700, keine sensiblen Logs.
- Bericht mit separater Modellantwort und Referenz, HTML-Escaping, keine externen Assets. Manifest und Messwerte reproduzierbar ablegen.
- Prüfung: zielgenaue Fixtures für Zukunft, falsche Autoren-ID, Referenzleck, Concierge-Scope, Outputrechte und HTML-Injektion; fmt und clippy im eigenen Package.
- Stop-Regel: Rohchats verlassen den Host oder Referenz/Future-Kontext erscheint im Generatorinput.
- Status: implementiert und synthetisch geprüft. Private Prüfsummen, Datenvalidierung, atomare Zwischenberichte, Telemetrie auch bei verworfenen Antworten und direkte Vergleichsansicht für beide lokalen Modelle. Ein ungültiger Vorlauf blockiert vor dem ersten Modellaufruf. Referenzen und Bewertungsmetadaten beeinflussen den Prompt nicht.

## M3: Ausführung und Gegenprüfung

- Nach den synthetischen Sanity-Läufen je Modell 50 gleiche Fälle und zehn Baselines vollständig durchführen. Ein Modell zur Zeit; Produktionslast beobachten.
- Durchsatz, Median/P95, Fehler, Kontext und Runtime-Ressourcen festhalten. Keine erfundene oder automatisch behauptete Sprachfreigabe.
- Eigenprüfung mit gate_hook.py --review gegen den tatsächlichen eigenen Commit; unabhängige Rust-/Security-Gegenprüfung vor Root-Merge.
- Stop-Regel: Produktionslast gefährdet den Betrieb oder wiederholte technische Fehler machen die Messung unbrauchbar.
- Status: vollständig abgeschlossen. Beide Modelle lieferten jeweils 50 personalisierte Antworten plus zehn Baselines, ohne Fehler oder Abschneidungen. Privater Gesamtvergleich und Nachweise sind erstellt; Ergebnis in ERGEBNIS.md. Kein fester Sampling-Seed: Code, Inputs, Parameter und Binärhash sind nachprüfbar, einzelne Antworten können zwischen Wiederholungen variieren. Der getestete Binärstand ist außerhalb des Worktrees mit Hash im Dateinamen gesichert.
