# Fixpaket B01: identischer Fehlerbody für wartende Anfragen

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vollreview-idempotenz

## Ziel und Vertrag

Befund `W01-R09-errors-1`, Klasse B. Der gemeinsame Streamer-Wrapper übermittelt bereits wartenden identischen Anfragen bei einem Fehler den Body `internal_error` statt des ursprünglichen Fehlerbodys. Status und Nicht-Caching sind bereits korrekt. Nur diesen Defekt beseitigen, sonstiges Verhalten erhalten.

Szenario: Zwei berechtigte lokale Anfragen mit demselben gültigen Idempotency-Key überlappen sich. Ein unbekannter gültiger Streamer führt beim Owner zu HTTP 404 mit `not_found` und Nachricht. Der Waiter erhält heute HTTP 404 mit `internal_error` ohne Nachricht. Keine Live-Reproduktion mit echten Konten.

## Unabhängige Freigabe

- Reviewer `a669cb6a850016ec9`, Workflow `wf_b0f0e2fe-347`, Ergebnis `W01:R09:errors`, ausschließlich Sol.
- Skeptiker 1 `aa07cc50d45eddf83`, Workflow `wf_c2fafb5b-bac`: `BESTÄTIGT`, Klasse B, Sollverhalten eindeutig belegt. Transcript-SHA256 `25e50ecfa543b3c78a16046f4b27c1370152a353ccdf9cb523035cf0749cdf79`.
- Skeptiker 2 `a6a25344ecabeb67f`, derselbe Workflow, frischer unabhängiger Kontext: `BESTÄTIGT`, Klasse B, Sollverhalten eindeutig belegt. Transcript-SHA256 `881d15b1acbea750a19b6059d079ad46f9cdb4f9e9165b78d70c2306ffe60ec1`.
- Modellbelege aus `message.model` sind durch Astra geprüft. Beide Skeptiker erhielten nur Behauptung und Ort, nicht die Reviewerbegründung.

Sollbelege am Review-SHA `0ecae1370f1a80d1a101249b5c932663d69be8af`:

1. `rust/crates/tb-http-core/src/error.rs:205-223`: `payload_json` ist ausdrücklich der effektive Fehlerbody zur Weitergabe an Idempotenz-Waiter und entspricht `IntoResponse`.
2. `rust/crates/tb-internal-api/src/handlers/telemetry_routes.rs:384-388`: derselbe Layer gibt Fehlerbody an Waiter weiter, ohne Fehler zu cachen.
3. `rust/crates/tb-internal-api/src/handlers/raid_oauth.rs:664-668,780-782`: beide Vergleichspfade verwenden den ursprünglichen Fehlerstatus und `payload_json()`.
4. `rust/crates/tb-internal-api/src/idempotency.rs:315-333,508-533`: `cacheable=false` verhindert Speicherung, nicht die Übermittlung an bereits Wartende; der Parallelvertrag enthält den ursprünglichen Body.

Beide Skeptiker haben Loopback, Origin, gültige Zugangsdaten, Headername, Fingerprint, Timeout, tatsächliche Routereinbindung und Aufrufer geprüft. Betroffen sind sieben Nutzer desselben Wrappers, keine sieben unabhängigen Fehlerkopien. Die genannten Vergleichspfade sind bereits korrekt und nicht zu ändern.

## Eigentum und Arbeitsstand

Eigener neuer Worktree `/home/nathanael/.worktrees/tb-vollreview-idempotenz`, Branch `fix/vollreview-idempotenz`, von frisch geholtem aktuellem origin/main. Der zuletzt beobachtete Main-SHA ist `6937e4a61f43a9c08174fa95c96f49da149ca859`, dessen Anwendungscode mit der Review-Basis übereinstimmt. Frischen Stand selbst prüfen.

Schreibrecht ausschließlich für `rust/crates/tb-internal-api/src/handlers/streamers.rs`, einschließlich eines günstigen gezielten Regressionstests im bestehenden Testmodul. Kein zweiter Fixer besitzt diese Datei. Falls der aktuelle Main den Defekt bereits beseitigt hat, nichts erneut ändern, mit Beleg zurückmelden.

Astra hält die Taskdokumente. Andere Dateien, fremde Worktrees und Branches bleiben unverändert. Keine Umbenennung, kein Refactoring, keine neuen Kommentare, Konfigurationen, Abhängigkeiten, Migrationen oder Nebenfixes. Keine Python-Änderung.

## Prüf- und Git-Freigabe

Vor Code-Suche `code-suche` und Graphify. Große Referenzen mit Context-Mode gezielt untersuchen, nicht ganze Manifeste und Router in den Kontext kopieren. Zum Editieren die eigene Datei mit Read laden.

Vor Tests `rolle-test-waechter`, vor Commit und Gate `rolle-merge-schleuse` laden. Aktuelle Tool-/Gate-Details stehen in OPS-PREFLIGHT.md im Artefakt-Worktree. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, `/home/nathanael/.local/bin/cargo-slot`. Paketbezogene Formatprüfung, Clippy und bestehende Tests von `tb-internal-api`; billigen Regressionstest ergänzen, wenn ohne zusätzlichen Aufbau möglich. Vorbestehende Fehler vor Änderungen als Baseline erfassen. Testdatenbank gemäß Auftrag: `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`. Zuvor nur Existenz von `rust/test-database.json` prüfen, keine Zugangsdaten lesen. Keine Prod-DB verwenden. Kein globales mutierendes cargo fmt; nur eigene Datei formatieren, Paket mit `fmt -- --check` prüfen.

Einzeln committen, nur die eigene Datei stagen. Commit-Trailer `Co-authored-by: GPT 6.1 Sol <gpt-6.1-sol@local>`. Der eigene Arbeitsbranch darf zur Sicherung gepusht werden. Kein Push nach main, kein Merge, Release, Deploy oder Restart in dieser Fixrunde.

Eigene Gate-Prüfung am endgültigen Commit ausdrücklich ohne Modellrückfall:

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vollreview-idempotenz --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080
```

Keine Pyramiden-, Hook- oder Zustandsänderung, kein `--chain`. Bei BLOCK nicht im selben Kontext nachbessern; Befund, SHA und Worktree an Astra zurückgeben. Astra startet den vorgeschriebenen frischen Sol-Fixer. Ein Standardreview mit anderer Modellkette ist verboten.

## Beweisziel und Rückgabe

Minimaler Fix-Commit, gezeigter Regressionseffekt, Baseline und tatsächliche Exit-Codes von Formatprüfung, Clippy und Tests. Sollzustand: Owner und bereits wartendes Duplikat erhalten denselben Fehlerbody; Fehler bleiben ungecacht; erfolgreiche Anfragen bleiben unverändert.

Frischer Fix-Kritiker wird anschließend separat durch Astra gestartet. Erst dessen Abnahme und das gültige Sol-Gate erlauben Integration. Deploy ist wegen des gemeldeten Wrapper-Migrationskonflikts bis zur Freigabe des Auftraggebers gesperrt.

Native Workerrolle, ausschließlich `gpt-6.1-sol`. Keine weiteren Agenten oder T3-Threads, kein ListAgents/SendMessage, keine Nutzerfragen. Bei echten Blockern strukturierte Rückgabe an Astra, T3 `c88f4057-c6b3-4c54-8750-addd08b24b42`. Kein Secretzugriff, keine ENV-Dateien, keine Browser- oder Kontenaktionen, kein `ai-coach`.
