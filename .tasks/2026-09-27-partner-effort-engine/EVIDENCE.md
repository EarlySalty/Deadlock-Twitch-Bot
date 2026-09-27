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
| `cargo test -p tb-effort` | 8 Unit-Tests und 3 PostgreSQL-Integrationstests bestanden |
| `cargo test -p tb-dashboard-api handlers::challenges --lib` | 2 bestanden |
| `cargo clippy -p tb-effort --all-targets -- -D warnings` | Exit 0 |
| `cargo clippy -p tb-dashboard-api -p tb-bot --all-targets --no-deps` | Exit 0; bestehende Warnungen außerhalb der Challenge-Implementierung |
| `cargo fmt --check -p tb-effort` und `rustfmt --check` der geänderten Rust-Dateien | Exit 0 |
| `git diff --check` | Exit 0 |
| Bestehendes Security-Push-Gate einschließlich gitleaks, RustSec und weiterer Scanner | Exit 0 |

Die drei Postgres-Tests enthalten insbesondere:

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
