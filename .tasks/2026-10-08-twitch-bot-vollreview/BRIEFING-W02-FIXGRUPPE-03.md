# W02: Fixgruppe 03

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min als Orientierung, kein Kompilierungsabbruch | Worktrees: je Paket unten

## Gemeinsame Regeln

Frischer Sol-Fixer je Paket, danach frischer Sol-Kritiker. Auftrag AUFTRAG.md gilt. Ausschließlich eigene freigegebene Rust-Datei ändern. Keine Kommentare, Refactorings, neue Konfiguration oder Abhängigkeit, keine Migration. Keine produktiven Python-Fixes, Secrets/ENV-Dateien, Produktionsdatenbank, Browser, echte Kontoaktionen oder ai-coach. Keine Agenten/Threads/ListAgents/SendMessage oder Nutzerfragen. Zusätzlichen Schreibbedarf an Astra zurückgeben. Taskdokumentation gehört Astra.

Vor Codesuche code-suche/Graphify. Hauptgraph `/home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json`, aktuelle Quelle aus eigenem Worktree. Neuen Worktree von frisch geholtem origin/main erstellen; falls schon vorhanden, zuerst Zustand prüfen und erhalten. Fremden Hauptcheckout, Prozesse und Worktrees nicht verändern. Prüfen, ob aktuelles main den bestätigten Fehler bereits beseitigt hat.

Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, `/home/nathanael/.local/bin/cargo-slot`, Kompilierung `--jobs 1`. Bestehende Tests der Crate tb-dashboard-api sowie Paket-fmt und Clippy durchführen, fremde rote Stellen gegen passende unveränderte Baseline abgrenzen. Eigene Datei gezielt formatieren. Testdatenbank `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`. `rust/test-database.json` lediglich auf Existenz prüfen, keine Zugangsdaten lesen. Optionalen Test-Opt-in erhalten; neue Tests dürfen ohne Aktivierung nicht wegen fehlendem Pool paniken. Reguläre Kompilierung fertiglaufen lassen; fehlende Prüfungen ehrlich melden.

Ein Git-Schritt je Bash-Aufruf mit literalen absoluten Pfaden. Eigener Commit und Sicherung des Arbeitsbranches erlaubt, kein Force-Push und kein Push nach main. Trailer `Co-authored-by: GPT 6.1 Sol <gpt-6.1-sol@local>`. Abschließender Gate: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <eigener literaler Worktree> --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080`. Bei BLOCK abgeben, keine weitere Runde im eigenen Kontext. Kein Merge, Release, Deploy oder Restart.

## B08: numerische Query-Grenzen

- Claim `W02-DA01-S005-correctness-1`, B bestätigt, Soll zweimal belegt. Ort `rust/crates/tb-dashboard-api/src/query_int.rs:51` am festen Review-SHA `0ecae1370f1a80d1a101249b5c932663d69be8af`.
- Worktree `/home/nathanael/.worktrees/tb-vollreview-query-grenzen`, Branch `fix/vollreview-query-grenzen`.
- Einziger Schreibpfad `rust/crates/tb-dashboard-api/src/query_int.rs` samt dessen Tests.
- Gültiger Dezimalinteger außerhalb i64 wird vor der dokumentierten Begrenzung als Nichtinteger mit HTTP 400 abgewiesen. Beispiele: `days=10000000000000000000` bei Chat-Analytics, das auf 3650 begrenzt werden soll; `months=9223372036854775808`, das auf 120 begrenzt werden soll. Positive und negative Überläufe berücksichtigen. Bestehende Behandlung fehlender Werte, Defaults, gewöhnlicher Ganzzahlen und tatsächlich ungültiger Syntax erhalten. Keine neue numerische Grammatik oder Einschränkung anderer Aufrufer einführen.
- Sollbelege: `query_int.rs:3-21,35-39,81-85`; erreichbare Rohstring-Aufrufer `handlers/chat_analytics.rs:24-31,45-48` und `handlers/performance.rs:36-41,69-81`. Gewollte separate Ablehnung im Research-Aufrufer nicht lockern.
- Reviewer `a5e91d4552542451e`, Workflow `wf_cdc4c5ac-9bb`: 41 Sol-Datensätze, SHA256 `534535d9bd27129d6364ad522166e38200286efb066e1e7de9261d6cb9a25e76`.
- Skeptiker `a8c034f8694dc60c2`: BESTÄTIGT-B, Soll true, 59 Sol-Datensätze, SHA256 `d0d6c2409f2eb220145650574a8f8f9e8a76b05b26053b81679c686a65f672e2`.
- Skeptiker `a0d051344e2216e5f`: BESTÄTIGT-B, Soll true, 36 Sol-Datensätze, SHA256 `f9a428e4903a4428fe5bbb4dc0b67462756d9a5764b147874f9d4e298732edbf`.

## B09: Operator-ID der IDOR-Testfixtures

- Claim `W02-DA02-S001-correctness-2`, B bestätigt, Soll zweimal belegt. Ort `rust/crates/tb-dashboard-api/src/auth/idor_e2e_tests.rs:233` am festen Review-SHA.
- Worktree `/home/nathanael/.worktrees/tb-vollreview-idor-fixture`, Branch `fix/vollreview-idor-fixture`.
- Einziger Schreibpfad `rust/crates/tb-dashboard-api/src/auth/idor_e2e_tests.rs`.
- Zwei Tests erwarten öffentliche Partneridentität und adminEligible=true, konfigurieren aber keine Operator-Twitch-ID. Bei erfolgreicher Datenbankeinrichtung scheitert der Discord-Test an level=none statt partner, der Twitch-Test an adminEligible=false. Beide erreichen die späteren IDOR-/CSRF-Assertions nicht. Bestehende synthetische Fixtures passend zu ihren vorhandenen Konto-IDs konfigurieren. Assertions nicht abschwächen, Guards und produktiven Auth-Code nicht ändern. Kein produktiver IDOR-Bypass ist mit diesem Claim belegt.
- Sollbelege: `idor_e2e_tests.rs:160-186,215-259`; vergleichbare Fixtures `auth/level.rs:1151-1177,1190-1211`; produktive Konfiguration `rust/bin/tb-dashboard/src/main.rs:517-524`. Fehlende Operator-ID soll weiterhin fail-closed bleiben.
- Reviewer `acb36cc102f02f5e5`, Workflow `wf_cdc4c5ac-9bb`: 68 Sol-Datensätze, SHA256 `3e99bce3b5977fa62a7b273b95b5f9cb2ea7572f36b79d0917ec46807bb33080`.
- Skeptiker `ab73546171685e847`: BESTÄTIGT-B, Soll true, 30 Sol-Datensätze, SHA256 `7aa87004683b9f6183f0fa682f0355b5a0e8c2aaf3992027d636f2172fb76c49`.
- Skeptiker `a85d07f5cc2d7b856`: BESTÄTIGT-B, Soll true, 26 Sol-Datensätze, SHA256 `13238b09a1540a8b1d23cd7628f53a462c91d9be834d205cbd44284455c48625`.

## Herkunft und Rückgabe

Skeptikerquelle für beide Pakete: `wf_f9b737d9-fe8/journal.jsonl` in der nativen Session f61905e7-f7ff-405b-a6d7-090dec371fcb. Beide Paare sind abgeschlossen; Astra hat Modelle, Hashes und Übereinstimmung des letzten StructuredOutput mit dem Journal geprüft. Reviewerbelege stehen in W02-KANDIDATEN.json und wurden zuvor vollständig geprüft. Originalurteile enthalten weiter die Grenzen rein statischer Prüfung und fehlender Live-Proben.

Je Paket strukturiert zurückgeben: Basis-/Head-SHA, geänderte Dateien, tatsächlich behobenes Szenario, Prüfungen mit Exit-Codes, Baselinevergleich, Gate-Urteil, eigener Worktreezustand und Blocker. Kein Markdown-Bericht des Workers. Der frische Kritiker prüft den festen Gesamtdiff gegen genau den zugewiesenen Claim und mögliche Regressionen, ohne selbst Builds oder Tests auszuführen.
