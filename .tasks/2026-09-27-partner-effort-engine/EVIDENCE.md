# Prüfnachweise

## Lokal

Alle Datenbankprüfungen liefen im eigens angelegten Wegwerf-Container `tb-effort-test-20260927` mit TimescaleDB/Postgres 16. Kein Produktionszugriff für Migrationen oder Testdaten.

| Prüfung | Ergebnis |
|---|---|
| Vollständige frische Migrationen einschließlich der einen neuen Effort-Migration | Exit 0 |
| `fresh_migrations_schema`, Snapshot aus realem Schema erzeugt und erneut abgeglichen | Exit 0 |
| SQLx-Metadaten gegen migrierte Testdatenbank erzeugt | Exit 0 |
| `cargo check -p tb-effort -p tb-config -p tb-dashboard-api -p tb-bot --all-targets -j2` | Exit 0 |
| `cargo test -p tb-config` mit bestehender Testsuite | 61 bestanden |
| `cargo test -p tb-effort` | 8 Unit-Tests und 4 PostgreSQL-Integrationstests bestanden |
| `cargo test -p tb-dashboard-api handlers::challenges --lib` | 2 bestanden |
| `cargo clippy -p tb-effort --all-targets -- -D warnings` | Exit 0 |
| `cargo clippy -p tb-dashboard-api -p tb-bot --all-targets --no-deps` | Exit 0; bestehende Warnungen außerhalb der Challenge-Implementierung |
| `cargo fmt --check -p tb-effort` und `rustfmt --check` der geänderten Rust-Dateien | Exit 0 |
| `git diff --check` | Exit 0 |
| Bestehendes Security-Push-Gate einschließlich gitleaks, RustSec und weiterer Scanner | Exit 0 |

Die vier Postgres-Tests enthalten insbesondere:

- Parallele Match-Buchungen über zwölf Tasks mit nur vier bezahlten Ereignissen, Clip-Cap, alle drei Platzierungswerte, gleiche qualifizierte Join-Zeit mit verschiedenen Quellen-IDs, widersprüchliche Replays und Cross-Partner-Attribution.
- Datenbankseitiger UPDATE/DELETE/TRUNCATE-Schutz, abgewiesene inaktive Partner und Zukunftsereignisse, keine beliebig buchbare Quest-Belohnung, fehlende Quellen und geschlossene DB-Verbindungen, Monats- gegen Lifetime-Punkte und permanente Achievement-Stufen.
- Zwei aktive Streamer mit simuliertem Helix: kein Co-Stream-Bonus vor 30 Minuten, danach genau einmal; frische Party-Präsenz noch ohne Punkte, anschließend dieselbe abgeschlossene Match-ID in beiden vorhandenen GC-Historien mit genau einer Vergabe pro Partner.
- Helix-Anzeigenamen der eigenen Viewer-Werber, überschneidungsfreie Community-Stunden trotz duplizierter Mitspielerzeilen.
- Zwei kurze Streams ergeben keine qualifizierende 30-Minuten-Session; ein einzelner langer Stream zählt. Eine Änderung der Zuschauerzahl von null auf eine Million verändert keine Punkte. Das Entfernen der Roh-Snapshots entfernt keine bereits gesicherten Wochenzeitbelege.

## GitHub

PR: https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/997

Der erste automatische Semantic-Review-Lauf `36288998112` scheiterte vor der Codeprüfung mit `You have exceeded your monthly quota`. Das vorhandene Gate wurde weder abgeschwächt noch entfernt und kein anderes kostenpflichtiges Modell eingesetzt. Die übrigen CI-Ergebnisse werden im PR festgehalten.

Keine unabhängige lokale Modellprüfung behauptet: Implementierung und Eigenprüfung wurden direkt erledigt. Der bestehende automatische GitHub-Review läuft über die Repository-Konfiguration.

Kein Merge, kein Deploy, kein produktiver Datenbankeingriff und kein Neustart von Bot- oder Dashboard-Diensten.

## GitHub-Review-Korrekturen

Die drei konkreten Review-Fundstellen wurden direkt behoben: persistierte begrenzte Quellenscans mit separatem Pfad für neue Bestätigungen und zyklischem historischem Abgleich; Löschung temporärer Party-Belege nach sieben Tagen; fehlender Helix-Client macht die Quelle auch ohne laufenden Stream ungesund und entfernt Stream Together aus dem erreichbaren Aufgabenpool. Regressionstests prüfen Fortsetzung nach Engine-Neustart, neue Ereignisse während des Backfills, verspätete ältere IDs in beiden Quellen, die Aufbewahrungsgrenze sowie den Fehlerzustand im Leerlauf.

## Fix-Runde vom 29. September 2026

Sieben Review-Befunde aus https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/997#issuecomment-5881532663 wurden auf dem rebasten PR-Branch bearbeitet. Die Quellverträge für #999 und Deadlock-Bots #466 sowie der Monatsabschlussvertrag für #996 stehen in `docs/partner-effort-engine.md` und in `REVIEW.md`. Die Empfehlungs-Testmigration ist bytegleich mit dem Stand von #999, SHA-256 `b77b09a09b18f5bcd99da686fe2875cc7acf793ae35912c7e6fb04fcfd15c285`; Tabelle und Trigger des Discord-Tests entsprechen dem Stand von #466.

| Prüfung | Ergebnis |
|---|---|
| `cargo test --locked -j2 -p tb-effort --all-targets --quiet -- --test-threads=1` mit `TB_TEST_DATABASE_URL` auf isoliertem TimescaleDB 2.17.2/Postgres 16 | 8 Unit-Tests, 7 echte PostgreSQL-Integrationstests bestanden, 0 ignoriert |
| `cargo test --locked -j2 -p tb-db --all-targets --quiet -- --test-threads=1` auf eigener Wegwerf-Datenbank | 36 Tests bestanden, 0 ignoriert, darunter frische Migrationen und Schema-Snapshot |
| `cargo fmt --check -p tb-effort` und `cargo clippy --locked -j2 -p tb-effort -p tb-db --all-targets -- -D warnings` | Exit 0 |
| Lesende Abfrage von `_sqlx_migrations` in `twitch_analytics` | Versionen `20260927010000` und `20260927110000` noch nicht angewandt |

Ein erster Testversuch scheiterte an der automatisch auf 12 GB Shared Buffers abgestimmten Wegwerf-Datenbank; der nächste Lauf nutzte einen auf 128 MB begrenzten eigenen Container. Fachliche Regressionen im ersten lauffähigen Testlauf wurden korrigiert und die vollständigen Suites anschließend grün ausgeführt. Es gab keinen produktiven Schreibzugriff, keinen Merge und keinen Deploy.

Der erste lokale Review-Gate-Lauf auf `4443f463` meldete zwei BLOCKING-Befunde: ein qualifizierter Join ohne gültige Partner-ID blockierte die Einladungsquelle, und neue Stream-Zeit wurde nach der Löschung älterer Snapshots nicht zum gesicherten Wochenstand addiert. Der Folge-Fix quittiert unveränderlich nicht zuordenbare Joins ohne Punkte und verarbeitet bei einer ungültigen optionalen Werber-ID die Einladung ohne Werber-Zuordnung. Der Zeitbeleg addiert ausschließlich neue, entdoppelte Intervalle nach dem zuletzt verarbeiteten Snapshot. Zwei PostgreSQL-Regressionen prüfen die Quellen-Gesundheit und zusätzliche Stream-Zeit nach Rohdatenlöschung sowie Wiederholungssicherheit.

Nach dem Folge-Fix: `cargo test --locked -j2 -p tb-effort --all-targets --quiet -- --test-threads=1` mit `TB_TEST_DATABASE_URL` auf dem isolierten TimescaleDB-Container: 8 Unit-Tests und 8 PostgreSQL-Integrationstests bestanden, 0 ignoriert. `cargo fmt --check -p tb-effort`, striktes Clippy für `tb-effort` und `tb-db` sowie `git diff --check` bestanden. Der abschließende Review-Gate-Lauf wird nach dem Commit eingetragen.
