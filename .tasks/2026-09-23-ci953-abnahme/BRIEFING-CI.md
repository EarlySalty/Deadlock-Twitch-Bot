# PR #953: kanonischer Brain-Schema-Verbrauch und CI-Verknüpfung

Auftraggeber ist der laufende ChatGPT-Auftrag über codex-mcp, kein T3-Intent-Thread. Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

Worktree: `/home/nathanael/.worktrees/tb-deterministic-pr-gate-20260922`
Branch: `ci/deterministic-pr-gate-20260922`
Übernommener HEAD: `eb1ee5e9c529e6a65e1fc1fe47c5567dfe28717e`
PR: https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/953

Lies die geltenden Regeln einschließlich PR-FIRST-TESTBETRIEB. Kein Merge, main-Push, Auto-Merge, Deploy, Dienst-Neustart, Cleanup oder Hook-Bypass. Keine Produktionsdatenbank, keine geladenen Geheimnisse. Keine anderen Repositories verändern. Builds höchstens zwei Jobs. Keine Git-Indexänderung, kein Commit/Push: Der Orchestrator prüft den Diff, committet einzelne Dateigruppen und pusht sofort. Er darf währenddessen geprüfte Frontend-Dateien committen.

## Dateibesitz

Du besitzt `.github/ci/brain_schema.rs`, `.github/workflows/rust-sqlx-check.yml` und bei belegtem Bedarf eng angrenzende `.github/ci/`-Tests sowie deinen Bericht `CI-NACHWEIS.md` in diesem Task-Verzeichnis. Keine Rust-Produktdateien unter `rust/`, keine Frontends oder deren Lockfiles, keine SECURITY-CI.md oder allgemeinen Statusdateien verändern. Ein anderer Worker bearbeitet ausschließlich `rust/` und RUST-NACHWEIS.md. Die vorhandene uncommittete Workflowzeile ergänzt SQLx `--no-dotenv --all-targets --locked`; diese Vorarbeit prüfen und erhalten, nicht zurücksetzen.

## Neuer, vom Orchestrator geprüfter Schema-Vertrag

Brain-PR #10 ist offen, Commit `72d1ae10d32d40d7d37e58777f7182cf528e105b`. Anonym erfolgreich gelesen:
`https://raw.githubusercontent.com/EarlySalty/Deadlock-Brain/72d1ae10d32d40d7d37e58777f7182cf528e105b/schema/README.md`.
Dort stehen der echte öffentliche Export und `scripts/ci/bootstrap-brain-schema.sql`. Sechs byteidentische Originalmigrationen unter `schema/vendor/dl-central-db/`, Ursprung Deadlock-Bots `ff635f7b354cb09909c01ddd6f773d0682dd89c9`; `schema/SHA256SUMS` umfasst diese und zwei Brain-Migrationen. PostgreSQL 16, keine zusätzlichen Extensions. Atomarer Bootstrap verweigert vorhandene Brain-Relationen sowie Datenbanknamen ohne eigenes `ci`- oder `test`-Segment. Der Twitch-Dependency-Pin bleibt zunächst `d8c34270868e129098e12243f53f5b52ee507b8b`; ändere ihn nicht beiläufig. Prüfe echte Dateien, Reihenfolge, Prüfsummen und Abwärtskompatibilität zu diesem Pin, statt Aussagen aus der README blind zu übernehmen.

## Aufgabe

1. Ersetze den bisher unvermeidlich roten anonymen Abruf aus dem privaten Deadlock-Bots-Repo durch den tatsächlich freigegebenen öffentlichen kanonischen Export mit vollem SHA und geprüften Hashes. Keine privaten DDL-Inhalte neu veröffentlichen, keine erfundenen Ersatzschemas, keine Offline-Ausnahme. Prüfe, ob die originale fünfteilige Sequenz byteidentisch aus dem neuen Export bezogen werden kann oder der vollständige neue Einstieg notwendig ist. Keine willkürliche Änderung der Produkt-Dependency. Herkunft und Entscheidung belegen.
2. Provisioniere vor den SQLx-Compile-Prüfungen in den tatsächlich relevanten Jobs, auch Workspace-/Integrationstests. Falls der originale Bootstrap einen neuen DB-Namen verlangt, CI-Wegwerfnamen konsistent anpassen. Die lokalen Schutzprüfungen gegen Produktion und Verbindungsumleitung behalten beziehungsweise stärken. Datenbankausgabe stumm halten.
3. Reproduziere mit frischer isolierter PostgreSQL-16-Testinstanz ohne Produktionsdaten oder Geheimnisse. Nutze keinen von anderen Tests gleichzeitig verwendeten Container oder Datenbanknamen. Positive Gegenprobe sowie mindestens falscher Herkunfts-/Dateihash, fehlender Download und verbotener Datenbank-/Verbindungsname müssen Fehler bleiben. Upstream-Bootstrap-Gegenprobe bestehendes Brain-Schema erhalten, sofern dieser Einstieg verwendet wird.
4. actionlint und gezielte Rust-Tests der CI-Werkzeuge ausführen. Required PR Gate und Deep-Scans nicht abschwächen. Keine Scanner-Ausnahmen ergänzen. Der Orchestrator untersucht Frontend-/OSV-/CodeQL-Findings separat.

## Nachweis

`CI-NACHWEIS.md`: Dateiliste, konkrete Commands, Exitcodes, positive/negative Zahlen, Herkunftsshas und Hashes, noch offene Punkte. Ein nicht abgeschlossener Online-SQLx-/GitHub-Lauf ist kein Erfolg. GitHub-Run 35940232811 ist die rote Ausgangsbasis; Logs nötigenfalls über `gh api repos/EarlySalty/Deadlock-Twitch-Bot/actions/jobs/<id>/logs` lesen, da `gh run view --log` hier leere Ausgabe lieferte.
