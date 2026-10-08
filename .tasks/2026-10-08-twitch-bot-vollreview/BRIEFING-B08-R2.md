# B08: zweite Runde mit erhaltenem Research-Vertrag

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min als Orientierung, keine Kompilierungs-Abbruchfrist | Worktree: /home/nathanael/.worktrees/tb-vollreview-query-grenzen

## Auftrag

Frischer Sol-Fixer für den bereits doppelt bestätigten Claim W02-DA01-S005-correctness-1. Grundvertrag und geprüfte Reviewer-/Skeptikerbelege stehen in BRIEFING-W02-FIXGRUPPE-03.md, Abschnitt B08. Die erste Rolle hat keine Quelle geändert und meldet einen begrenzten zusätzlichen Schreibbedarf. Der gemeinsame Parser erhält bei Chat-Analytics und Research identische Argumente; Research prüft anschließend den Rohwert erneut mit parse::<i64>().ok(). Ein alleiniger Überlauf-Fix im Parser würde deshalb den ausdrücklich zu erhaltenden Research-Sondervertrag lockern.

Astra erweitert den Schreibumfang um die unmittelbar betroffene Aufruferprüfung. Dies dient ausschließlich dem Erhalt des bestehenden Research-Verhaltens während desselben bestätigten Parserfixes. Es ist kein neuer Claim und kein Auftrag, andere Research-Funktionen zu verändern. Eine Produktentscheidung ist damit nicht verbunden.

## Eigentum und Startstand

- Worktree `/home/nathanael/.worktrees/tb-vollreview-query-grenzen`, Branch `fix/vollreview-query-grenzen`.
- Erhaltene saubere Basis und Head a8b5b5e986a1de0b8e2f981651f83bda9cf400dd. Eigenen tatsächlichen Stand prüfen und aktuelles origin/main holen. Keine parallele schreibende Rolle für B08; B09 läuft in einer anderen Datei weiter.
- Erlaubte Dateien: `rust/crates/tb-dashboard-api/src/query_int.rs` und `rust/crates/tb-dashboard-api/src/handlers/admin_research.rs`. Keine anderen Schreibpfade. Diese Freigabe ersetzt die Ein-Datei-Grenze des B08-Grundbriefings.
- Erster Fixer a497c9a5de2aed3bb, Workflow wf_f3187078-c1c: 35 echte Sol-Datensätze, SHA256 e29b089fcff6908dc89306096384373ae3d8d51a3a13d97f2ff943345a4e3a1c, durch Astra geprüft. Kein Commit, Test oder Gate aus dieser Rolle. Ihr Ergebnis ist ein Schreibumfangsblocker, keine fehlgeschlagene Codekorrektur.

## Fachlicher Umfang

Gültige Dezimalinteger außerhalb i64 müssen in den dafür vorgesehenen Query-Aufrufern auf deren vorhandene Grenzen begrenzt werden. Positive und negative Überläufe, gewöhnliche Werte, Defaults, fehlende Werte und echte Syntaxfehler berücksichtigen. Keine neue Zahlengrammatik. Die bisherige separate Research-Ablehnung bleibt erhalten, einschließlich ihres bisherigen Fehlervertrags. Nicht durch neue Parser-Sonderfälle anhand zufällig gleicher Default-/Grenzwerte nach Aufrufern unterscheiden.

Belege der ersten Rolle: query_int.rs:51 und handlers/admin_research.rs:241-260 am genannten Startstand. Vor Codesuche code-suche und Graphify verwenden, anschließend aktuellen Code und vorhandene Verträge lesen. Die zusätzliche Datei nur so weit ändern, wie es zum Erhalt des bereits ausdrücklich festgelegten Sondervertrags notwendig ist. Kein Research-Umbau, keine neue Policy, keine geänderten Guards oder Produkttexte nebenbei.

## Prüfungen und Abschluss

Gemeinsame Prüf-, Sicherheits- und Gitregeln des Grundbriefings gelten. Günstige Parserregressionen und den Research-Sondervertrag belegen; bestehende tb-dashboard-api-Tests, Paket-fmt und Clippy mit passender unveränderter Baseline prüfen. Toolchain 1.97.1, SQLX_OFFLINE=1, cargo-slot und bei Kompilierung --jobs 1. Synthetische Testdatenbank wie im Grundbriefing. Optionalen DB-Opt-in erhalten. Normale Kompilierungen nicht voreilig beenden und vorhandene eigene laufende Aufgaben nicht duplizieren.

Der ursprüngliche Nutzerauftrag umfasst lokale Commits und die spätere Integration. Liefere einen sauberen nichtleeren Commit-Diff für den anschließenden Kritiker. Ein Git-Schritt je Bash-Aufruf, literale absolute Pfade. Sol-only-Gate wie im Grundbriefing, kein --chain oder Modellfallback. Bei BLOCK oder weiterem Schreibbedarf mit Belegen abgeben; die nächste Fixrunde bekommt frischen Kontext. Kein Main-Merge, Release, Deploy oder Restart. Keine Secrets/ENV-Dateien, Produktionsdatenbank, Migration, Browser, Kontoaktionen, ai-coach, weiteren Agenten/Threads/ListAgents/SendMessage oder Nutzerfragen. Astra dokumentiert.

Rückgabe: tatsächliche Basis und Head, sauberer Worktree, geänderte Dateien, behobener Claim, unveränderter Research-Vertrag, Prüfungen/Exit-Codes, Baseline, Gate samt Belegen und offene Blocker. Danach prüft ein frischer read-only Sol-Kritiker den festen Gesamtdiff beider Dateien.
