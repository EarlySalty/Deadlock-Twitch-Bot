# Vorprüfung von Gate, Build und Deploy

Stand: 2026-10-08. Quelle: Sol-Workflow `wf_8b989b1c-1e8`, Agent `a1f8aeed6c7779c6b`. Modellnachweis: 123 Nachrichten mit `message.model = gpt-6.1-sol`, kein anderes Modell. Ergebnisse aus der strukturierten Rückgabe durch Astra gesichert. Der Worker schrieb wegen seiner Workflow-Ausgabevorgabe keine eigene Markdown-Datei.

Die Prüfung war lesend. Keine Tests, Builds, Gate-Modellaufrufe, Git-Mutationen, Restarts, Deploys oder Datenbankaufrufe wurden ausgeführt.

## Deploy-Blocker

Der vorgeschriebene Wrapper `/usr/local/bin/deploy-twitch-release` führt zusätzlich zum Releasewechsel produktive Änderungen aus:

- Zeile 366: startet die Migrationseinheit.
- Zeile 387: installiert PostgreSQL-Peerregeln.
- Zeilen 611 bis 618: installiert Konfiguration und aktiviert einen Timer.

`--restart` begrenzt nur die Dienstneustarts. Es unterdrückt diese anderen Schritte nicht. Das widerspricht der harten Auftragsgrenze gegen Migrationen und schreibende Produktionsdatenbank-Befehle. Astra hat die Frage zur Freigabe des regulären Wrappers an den Auftraggeber gestellt. Bis zur Klärung kein Deploy, kein Skip-Schalter, kein alternativer Installationsweg und keine Hookänderung. Read-only-Reviews und die Vorbereitung bestätigter Fixes können weiterlaufen.

Installierter Wrapper und Installer sind laut `cmp` mit Exit 0 identisch zu den versionierten Dateien `ops/systemd/deploy-twitch-release` und `ops/systemd/install-twitch-release.sh` im Review-Worktree.

## Sol-only-Gate

Aktiver Hook: `/home/nathanael/Documents/.claude/gpt-workers/gate_hook.py`. `agent_hook.py` und `review_gate.py` sind Symlinks darauf. Ab Zeile 5439 unterstützt der Hook `--review`, `--repo`, `--base`, `--head`, `--model`, `--effort` und `--timeout`.

Beispiel für einen späteren tatsächlich vorhandenen Fix-Worktree:

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vollreview-fixpaket --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080
```

Der Einzelmodelllauf ohne Rückfall ist in Zeilen 5482 bis 5499 belegt; der Codex-Treiber erhält den Modellnamen unverändert (5198 bis 5207). Noch kein echter Gate-Lauf als Laufzeitbeweis.

Vor einem automatischen Merge-/Push-Hook muss der explizite Sol-Lauf am endgültigen Commit ALLOW ergeben und dessen unverändert gespeicherter Zustand lesend geprüft werden: `reviewer_model=gpt-6.1-sol`, `allow_sha=HEAD`, richtige Basis und Branchbindung, `allow_phase=phase1|phase2`, frischer Zeitstempel. Bindung und Modellübernahme: Zeilen 2217, 4363 bis 4410 und 5518 bis 5529. Ein fehlgeschlagenes Speichern entwertet den Nachweis (5668 bis 5692).

Ohne passenden Zustand startet der automatische Hook bei reviewbarem Code die globale Kette Sol, Opus, Grok. Dafür besteht in diesem Auftrag keine Freigabe. Bei geändertem SHA, Drift, fehlendem oder altem Zustand wieder ausdrücklich Sol prüfen; keinen Standardlauf, keinen `--chain`-Aufruf und keinen ungeprüften Code-Push nach main. Hookzustand und Pyramide nicht von Hand ändern. Der aktuelle Hook begrenzt erfolglose Runden auf vier.

Ausnahme für reine Markdown-Taskdokumente: Der native Gate kehrt mit `ALLOW: no reviewable changes` vor einem Modellaufruf und vor Zustandsspeicherung zurück (5540 bis 5542). Der automatische Push-Pfad tut dies ebenfalls (1458 bis 1468, vor der Kette ab 1487; Filter 2008 und 2063 bis 2072). Dafür ist kein nicht vorhandener Sol-Modellaufruf zu erfinden. Der Doku-Checkpoint 6937e4a6 wurde am 2026-10-08 über den normalen Bash-Push-Hook erfolgreich nach main gebracht.

Eine manuelle Simulation des PreToolUse-Hooks innerhalb eines Bash-Prozesses wurde zuvor wegen des dort vorhandenen Variablennamens `GIT_EDITOR` abgewiesen (945 bis 953). Es wurden ausschließlich Variablennamen geprüft, keine Werte oder Secrets. Der normale, unverändert geschützte Bash-Werkzeugaufruf war erfolgreich. Keine Umgebung, Hookdatei oder Gatezustandsdatei wurde dafür verändert. Solche manuellen Simulationen ersetzen nicht den tatsächlichen Werkzeug-Hook.

## Isolierte Integration

Der fremde lokale `main` liegt auf `d8284816`; `origin/main` lag bei der Prüfung auf `0ecae137`. Den fremden Checkout und dessen Branchzeiger nicht verändern.

Der Hook unterstützt den Fast-forward-Push vom eigenen Arbeitsbranch nach main ausdrücklich (1005 bis 1044), mit sauberem Baum, aktuellem Remote und Vorfahrenprüfung sowie aktivem Test- und Review-Gate (1783 bis 1849). Im eigenen Fix-Worktree origin aktualisieren und integrieren, Prüfungen und Commit abschließen, Sol-Gate durchführen, Zustand prüfen, dann `git push origin HEAD:main`. Jeder Git-Schritt als eigener Bash-Aufruf mit literalem absolutem Pfad. Bei Drift neu integrieren und prüfen.

## Toolchain und Tests

- Rust und Cargo 1.97.1 wurden per Versionsaufruf bestätigt. `/usr/bin/cargo` ist 1.75.0 und ungeeignet.
- `/home/nathanael/.local/bin/cargo-slot` verwendet `/home/nathanael/.cargo/bin/cargo`, drei Buildslots und zusätzlich den Release-Lock. Toolchain über `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu` wählen, nicht als eingeschobenes `+1.97.1` hinter cargo-slot, weil der Test-Gate-Matcher die direkte Form `cargo-slot test|clippy|nextest` erwartet.
- `tb-db` und `tb-raid`: bestehendes `rust/scripts/test_db.sh`, eindeutig eigener Containername. `up` entfernt einen gleichnamigen Container. Keine fremden Container oder festen Produktionsports verwenden.
- Übrige betroffene Crates: `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`, `SQLX_OFFLINE=1`, paketgezieltes `cargo-slot test --locked -p <crate> --no-fail-fast`. Vorher nur die Existenz von `rust/test-database.json` prüfen: Diese Datei hätte laut `rust/test-support/database.rs:9-35` Vorrang. Keine Zugangsdaten daraus lesen.
- Baseline auf unverändertem origin/main separat messen. Ohne DSN können bestehende Tests überspringen; das zählt nicht als grüner Funktionsbeweis.
- Paketgezieltes Clippy und Formatprüfung. Mutierendes `cargo fmt` ist durch `guardrails.py:170-174` gesperrt; nur eigene Dateien gezielt formatieren und `cargo-slot fmt ... --package <crate> -- --check` ausführen.

## Release nach Klärung des Blockers

Release in eigenem sauberen Worktree am frisch ermittelten aktuellen origin/main-SHA bauen. Heute werden acht Binaries benötigt: `tb-bot`, `tb-dashboard`, `tb-stream-audit`, `tb-config-check`, `tb-llm-usage-recover`, `tb-category-collector`, `tb-twitch-watchdog`, `clip_context_learn`. `cargo-slot build` mit `--locked --release -j 2` und eigenem `--target-dir` verwenden. Kein Build im fremden Hauptcheckout.

Die drei Frontend-Builds sind `bot/dashboard_v2`, `bot/admin_dashboard` und `website`. Das Hauptdashboard schreibt nach `bot/analytics/dashboard_v2/dist`, die anderen jeweils nach `dist`.

Der Wrapper verlangt ein internes `.git`-Verzeichnis und verschiebt seine Quelle nach `/opt/deadlock/twitch/builds/<SHA>` (Zeile 257). Deshalb keinen Git-Worktree direkt übergeben. Einen eigenen sauberen Release-Clone am selben SHA für die Übergabe verwenden; geprüfte eigene Artefakte als reguläre Dateien hinein kopieren und Herkunft erneut prüfen. Unmittelbar vor dem Deploy origin/main erneut vergleichen. Der Wrapper erzwingt diese Aktualität nicht selbst.

Belegte Aufrufform nach Freigabe: `/usr/local/bin/deploy-twitch-release <voller-SHA> <absoluter-Clonepfad> --restart twitch-bot --restart twitch-dashboard`. Ohne diese Auswahl kann der Standard weitere Dienste neu starten.

`readelf --string-dump=.twitch_build` muss für jedes erforderliche Binary exakt den Release-SHA ohne `-dirty` ergeben. Herkunftsprüfung: `rust/bin/build_revision.rs:19-38`; Installer-Prüfungen: 124 bis 140 und 281 bis 293. Datei-Alter ist kein Beleg.

## Ausgangszustand der Dienste

Lesend beobachtet: Bot und Dashboard active/running, PIDs 3022811 beziehungsweise 3022680, jeweils NRestarts=0. `current` zeigte auf `/opt/deadlock/twitch/releases/b0bd68248c3accc1771e938e6166c3a122ac154e`. Die Zugehörigkeit dieses SHA zur origin/main-Historie wurde mit `merge-base --is-ancestor` und Exit 0 belegt. Dies ist kein Funktionsnachweis und kein Deploy dieses Auftrags.

Nach einem erlaubten Deploy: PID-Wechsel, tatsächliche `/proc/<PID>/exe`, SHA ohne deleted-Markierung, Neustartzähler, aggregierte Fehlerlogs und betroffene kontofreie Read-only-Pfade prüfen. Keine Rohlogs, echten Kontenaktionen oder Browser ohne Moli-Freigabeweg. `deploy-twitch-release --pruefen` ist laut Code lesend, kontrolliert aber vier Dienste und kann nach zwei absichtlichen Restarts abweichende SHAs der übrigen Dienste melden.

## Cleanup

Vor Löschen eines eigenen Branches `git merge-base --is-ancestor <eigener-branch> origin/main` mit Exit 0 belegen. Eigene ignorierte und wertvolle Dateien prüfen, Worktree entfernen, Branch löschen. Wegen des alten lokalen main kann `branch -d` trotz Remote-Integration ablehnen; ein `-D` ist nur mit belegter origin/main-Vorfahrenbeziehung vertretbar. Fremde Branches und Worktrees bleiben unangetastet.
