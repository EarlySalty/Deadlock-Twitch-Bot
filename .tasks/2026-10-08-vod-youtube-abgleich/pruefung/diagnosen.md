# Gesicherte Prüfausgaben

Code-SHA: 76332e3d26e80e48e013e92ddddb429acf068652. Aus tatsächlich ausgeführten Logs abgeleitet, keine vollständige Workspace-Suite.

```text
archive-gatefix2.log
test result: ok. 57 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.82s
cargo-slot: EXIT=0

api-gatefix.log
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 1336 filtered out; finished in 2.85s
cargo-slot: EXIT=0

youtube-client-tests.log
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 339 filtered out; finished in 0.01s
cargo-slot: EXIT=0

frontend-tests.log
1..6
# tests 6
# pass 6
# fail 0
# cancelled 0
# skipped 0
# todo 0
```

Die API- und Client-Binärtests mit null passenden Tests sind keine zusätzlichen bestandenen Proben. Der frühere API-Null-Lauf zählt nicht.

## Tatsächlich gemessene Lintbaseline

Ausgangscommit: 0ecae1370f1a80d1a101249b5c932663d69be8af, eigener detached Worktree.

| Umfang | Ausgangsstand | Aktuell |
| --- | --- | --- |
| Strikt, mit Dependencies | Exit 101, eine eindeutige Fundstelle | Exit 101, dieselbe eine Fundstelle |
| Strikt, ausgewählte Ziele mit --no-deps | Exit 101, vier eindeutige Fundstellen | Exit 101, dieselben vier Fundstellen |
| Ausgewählte Ziele ohne -D warnings | nicht zusätzlich gemessen | Exit 0, vier bestehende Warnungen |

Dependencyfund: tb-raid/src/signup_denylist.rs:71, result_unit_err.

Vier identische Fundstellen bei --no-deps: analytics.rs:321, too_many_arguments; credentials.rs:214, manual_map; upload_worker.rs:822, type_complexity; vocab.rs:164, needless_borrows_for_generic_args. Lib und lib-test melden teilweise dieselben Funde doppelt.

```text
clippy-warnings.log
warning: tb-social-media (lib) generated 4 warnings
warning: tb-social-media (lib test) generated 4 warnings (4 duplicates)
Finished dev profile [unoptimized + debuginfo] target(s) in 34.49s
cargo-slot: EXIT=0
```

Der genaue vollständige Gate-Runde-2-Output liegt in gate-round2.txt. Formatvergleich und Moli-Messwerte liegen neben dieser Datei als JSON. Die wörtlichen Prüfbefehle stehen in ../PRUEFUNG.md.
