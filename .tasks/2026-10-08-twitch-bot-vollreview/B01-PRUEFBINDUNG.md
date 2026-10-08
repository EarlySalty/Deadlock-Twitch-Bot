# B01: Prüfbindung geschlossen

Stand: 2026-10-08. Diese Prüfrolle ändert keinen Anwendungscode und erteilt keine neue fachliche, Gate-, Integrations- oder Deployfreigabe. Das vorhandene fachliche Kritikerurteil bleibt unverändert.

## Ergebnis und unveränderter Quellstand

Die konkrete Bindungslücke ist geschlossen: Ein neuer Regressionstest lief am sauberen, unveränderten aktuellen Head. Die historische vollständige Suite und ihre Baseline sind zusätzlich durch die damalige Werkzeugfolge und identische Quellbäume gebunden. Der alte Logname wird nicht als Quellstandsbeweis verwendet.

- Worktree: `/home/nathanael/.worktrees/tb-vollreview-idempotenz`
- Head: `c3aa3cc94fec60aa365dcc9ca9f54b4bd3f1539a`
- Basis: `a8b5b5e986a1de0b8e2f981651f83bda9cf400dd`
- Git-Baum des Heads: `3f972618cdc5389900fca5cbfcdb476532b35875`
- Rust-Quellbaum: `67c189f417131aacbb77fa899d861e91bf5e8ce8`
- SHA256 der tatsächlichen Datei `/home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/crates/tb-internal-api/src/handlers/streamers.rs`: `d4575c8f5cf501df9eaef2b250699cd4a1370e07a892e1c0136e8a2e7539595a`
- SHA256 des binärfähigen Rust-Diffs Basis gegen Head: `0b81335766d1134ffef2347c0c9ac3be4e5407710aeeaa7b7282058cc092cefa`
- SHA256 des Manifests aus Pfad und SHA256 von 1797 versionierten Rust-Baueingaben: `e08cd068b4a31af474c1e5425878d70b5218143ad434d37372ec208520ffca78`. Erfasst sind `.rs`, `.toml`, `Cargo.lock` und SQLx-Query-Caches, keine ENV-Dateien.

Vor und nach jeder neuen Prüfung waren HEAD, Git-Bäume, Datei-, Baueingabe- und Diffhash identisch. `git status --porcelain=v1 --untracked-files=all` war jeweils leer, `git diff HEAD -- rust` ebenfalls. Der Runner prüft diese Identität und würde bei Abweichung abbrechen. Die einzige Task-Schreibdatei dieser Rolle ist dieses Dokument; Prüflogs und einmalige Hilfsprogramme liegen unter `/tmp/tb-b01-pruefbindung-vFV7lm8L/`. Cargo erzeugte nur normale Buildartefakte im vorhandenen eigenen Worktree-Target.

## Neue Prüfungen am aktuellen Head

Zeitraum: 10:03:47 bis 10:06:05 UTC. Vorprüfung `/tmp/tb-b01-pruefbindung-vFV7lm8L/own-process-preflight.json`: keine laufende eigene Cargo-, Rustc-, Clippy- oder Rustfmt-Prüfung gefunden. Zusätzlich wurde vorher nach Cargo-Slot-Aufrufen für diesen Worktree gesucht; kein eigener Aufruf war aktiv. Der Regressionstest und Clippy erhielten tatsächlich Slot 2. Kein Erfolg wurde aus Wartezeit abgeleitet, keine Kompilierung pauschal abgebrochen und kein fremder Prozess oder Slot verändert.

Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)`.

Alle vier Befehle wurden ohne Pipe mit vollständiger Ausgabe in die jeweils genannte Logdatei ausgeführt. Ihr tatsächlicher Prozess-Exitcode wurde in `/tmp/tb-b01-pruefbindung-vFV7lm8L/checks.json` gespeichert. Arbeitsverzeichnis war `/home/nathanael/.worktrees/tb-vollreview-idempotenz/rust`. Der Runner übergab ausschließlich folgende Umgebung, ohne geerbte Produktionszugänge:

```text
HOME=/home/nathanael
PATH=/home/nathanael/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin:/home/nathanael/.cargo/bin:/home/nathanael/.local/bin:/usr/local/bin:/usr/bin:/bin
RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu
SQLX_OFFLINE=1
TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql
TB_TEST_REQUIRE_DB=1
LANG=C.UTF-8
```

### 1. Regression

Der exakte Filter stammt aus der eigenen Quelle, `/home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/crates/tb-internal-api/src/handlers/streamers.rs:1616-1692`.

```bash
/home/nathanael/.local/bin/cargo-slot test --manifest-path /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/Cargo.toml --target-dir /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/target --locked -p tb-internal-api --jobs 1 --no-fail-fast --lib handlers::streamers::tests::idempotency_waiter_erhaelt_originalen_fehlerbody_ohne_caching -- --exact --include-ignored
```

Exit **0**: **1 passed, 0 failed, 0 ignored, 359 filtered**. Tatsächliche Kompilierung, danach der namentlich bestätigte Test. Der Test erzwingt Owner/Waiter-Überlappung durch Polling und One-Shot-Freigabe. Er prüft 404, dynamischen 400-Body und 500, Status, beide Antwortbodys, Replay-Markierung und erneute Ausführung nach einem ungecachten Fehler. Der echte gemeinsame Wrapper und die echte Idempotenzkomponente laufen; diese Teile sind nicht durch Fakes ersetzt. Der Test braucht keine Datenbank, keinen Browser und kein Konto. Seine kurze Testdauer ist deshalb kein DB-Skip.

### 2. Eigene Formatierung

```bash
/home/nathanael/.cargo/bin/rustfmt --edition 2021 --check /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/crates/tb-internal-api/src/handlers/streamers.rs
```

Exit **0**. Die leere Ausgabe ist zusammen mit dem aufgezeichneten Prozess-Exitcode ein gültiger Formatnachweis, nicht allein wegen eines leeren Logfiles.

### 3. Paketformatierung

```bash
/home/nathanael/.local/bin/cargo-slot fmt --manifest-path /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/Cargo.toml --package tb-internal-api -- --check
```

Exit **1**, **34** `Diff in`-Blöcke, keiner in der eigenen Datei. Nach Entfernung ausschließlich der historischen `cargo-slot:`-Statuszeilen ist die gesamte neue Formatausgabe bytegenau identisch zur gebundenen Baseline. Die historischen Fix- und Baseline-Formatlogs sind bereits untereinander bytegleich. Kein mutierendes `cargo fmt` ausgeführt.

### 4. Clippy

```bash
/home/nathanael/.local/bin/cargo-slot clippy --manifest-path /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/Cargo.toml --target-dir /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/target --locked -p tb-internal-api --jobs 1 --all-targets -- -D warnings
```

Exit **101**, Abbruch in `/home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/crates/tb-chat/src/scam_pitch.rs:1502:29`, `clippy::needless_borrows_for_generic_args`. Diese Datei ist zwischen B01-Basis und B01-Head identisch, entsprechend `git diff --exit-code a8b5b5e986a1de0b8e2f981651f83bda9cf400dd c3aa3cc94fec60aa365dcc9ca9f54b4bd3f1539a -- rust/crates/tb-chat/src/scam_pitch.rs`, Exit **0**. Kein grüner Clippy-Nachweis für das gesamte Zielpaket; der Abbruch in der Abhängigkeit bleibt offen.

## Historische Bindung: tatsächlicher Fixhead statt Logname

Gezielt geprüft wurde `/home/nathanael/.claude/projects/-home-nathanael-repos-Deadlock-Twitch-Bot/f61905e7-f7ff-405b-a6d7-090dec371fcb/subagents/workflows/wf_45aca23b-0b9/agent-a85cf61377bbe9481.jsonl`, SHA256 `6771ffa6431310267c216744e629be6d3bd6839068972fa0c030f11d659efee8`. Auch der Vorgänger a50c2d6b8221a18b2 und die Wiederaufnahme ac4d984dbc328a0cd wurden gezielt gesichtet. Keine vollständigen Transcripts werden hier wiedergegeben.

Die belastbare Werkzeugfolge im Abgleich-Transcript:

1. Zeilen 58-62 bestätigen den vollständigen damaligen Head `089a9cbe8fddeff1bb1731b0f0b15afc3ae70cb7` und den sauberen Arbeitsbaum. Die anschließende Folge enthält keine Quelledits. Quellstandswechsel sind explizite Git-Switches und erfolgreiche Rebases, keine stillen oder uncommitteten Fixes.
2. Zeilen 290-294: erfolgreicher Rebase, danach `rev-parse origin/main HEAD` mit Basis `3341098f3a953e231c0ec5731141996b10f1b9b6`, aber tatsächlichem **Fixhead `307c09e64528699364036780eb9f6623192be1d8`**. Dieser enthält Fix und Regression. `3341098f` im Lognamen bezeichnet also nicht den getesteten Head.
3. Zeile 297 startet die historische Regression, Zeile 326 die vollständige Fixsuite. Zwischen Start und Fertigmeldung gibt es keine Quelländerung. Zeilen 329-330 bestätigen Regression Exit 0; Zeilen 352-353 bestätigen die beendete Suite mit 336/24 und Exit 101. Erst danach folgt der nächste Rebase in Zeile 358.
4. Zeilen 368-369 wechseln ausdrücklich auf Baseline `bd69502728861f8279e8b044592ecac9058fc732`. Zeile 371 startet die Baselinesuite, Zeile 377 die Formatprüfung. Zeilen 407-408 bestätigen beide vollständigen Ergebnisse. Erst Zeilen 429-430 wechseln zurück zum Fixbranch.
5. Zeilen 442-456 bestätigen den heutigen Head und die heutige Basis, erfolgreiche Rust-Quellgleichheit zum tatsächlich geprüften Fixhead beziehungsweise zur geprüften Baseline und wieder einen sauberen Arbeitsbaum.

Zusätzlich jetzt nachgerechnet:

| Historischer Stand | Heutiger entsprechender Stand | Gemeinsamer Git-Baum `:rust` |
| --- | --- | --- |
| `307c09e64528699364036780eb9f6623192be1d8` | `c3aa3cc94fec60aa365dcc9ca9f54b4bd3f1539a` | `67c189f417131aacbb77fa899d861e91bf5e8ce8` |
| `bd69502728861f8279e8b044592ecac9058fc732` | `a8b5b5e986a1de0b8e2f981651f83bda9cf400dd` | `da48b906659042b04e9166f184868bd85e72bb2e` |

Auch der vollständige jeweilige Repo-Diff mit ausschließlich `.tasks` ausgeschlossen ist leer, jeweils Exit **0**:

```bash
git -C /home/nathanael/.worktrees/tb-vollreview-idempotenz diff --exit-code 307c09e64528699364036780eb9f6623192be1d8 c3aa3cc94fec60aa365dcc9ca9f54b4bd3f1539a -- . ':(exclude).tasks'
git -C /home/nathanael/.worktrees/tb-vollreview-idempotenz diff --exit-code bd69502728861f8279e8b044592ecac9058fc732 a8b5b5e986a1de0b8e2f981651f83bda9cf400dd -- . ':(exclude).tasks'
```

Diese jetzige Quellgleichheit ist nur der letzte Teil des Beweises. Entscheidend ist zusätzlich die damalige belegte Werkzeugfolge mit tatsächlichem Head und beendeten Läufen vor dem nächsten Quellstandswechsel.

### Wiederverwendete vollständige Suite

Fix und Baseline führten denselben konkreten Befehl aus, jeweils mit dem oben belegten Quellstand und eigener Logumleitung:

```bash
RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu SQLX_OFFLINE=1 TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql' TB_TEST_REQUIRE_DB=1 /home/nathanael/.local/bin/cargo-slot test --manifest-path /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/Cargo.toml --target-dir /home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/target --locked -p tb-internal-api --jobs 1 --no-fail-fast -- --include-ignored
```

- Fix: Exit **101**, **336 passed, 24 failed, 0 ignored, 0 filtered**; Doc-tests 0/0.
- Baseline: Exit **101**, **335 passed, 24 failed, 0 ignored, 0 filtered**; Doc-tests 0/0.
- Die sortierten Listen der 24 fehlgeschlagenen Tests sind exakt gleich, nicht nur ihre Anzahl. Unter anderem fehlt in vorhandenen Testaufbauten `twitch_analytics`, SQLSTATE `3D000`.
- Kein neuer vollständiger Suite-Lauf wurde behauptet oder benötigt. Die Wiederverwendung beruht auf dem rekonstruierten damaligen Quellstandsbeweis.

TESTNACHWEIS[TW-1]: 336 passed, 0 ignored | Baseline: 24 rot

## Logs und SHA256

Historische Logs wurden unverändert ins eigene Prüfverzeichnis kopiert. Herkunft und vollständige Befehlsfolge stehen in `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-proof.json`; darin sind auch die 24 Testnamen enthalten.

| Prüfung | Logpfad | SHA256 |
| --- | --- | --- |
| Neue Regression, Exit 0 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/regression.log` | `0e500b9842a52cb841036f999a0a3efb1911100ce85a9e6809884ad25e31b1bb` |
| Neue eigene Formatprüfung, Exit 0 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/own-fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Neue Paketformatprüfung, Exit 1 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/package-fmt.log` | `c52f2d1ec0765720eb3254426b9c6ae77cb00263ff56d2e5a4cfacabca5d8303` |
| Neues Clippy, Exit 101 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/clippy.log` | `a327f530ad5fa949666448269cdf9151ac1db08a2affc6b3ea5623d9dccb8688` |
| Historische Fixsuite, Exit 101 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-fix-suite.log` | `4a91dbed58aa7fe1e0ab981c78dd26e8f060281d41b0a4b7f9909db151b85f00` |
| Historische Baselinesuite, Exit 101 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-baseline-suite.log` | `e3209530bf701b3b1bee4836e04dfdf928ffe7bcb3dfc76736bc9cedb6c3728a` |
| Historische Regression, Exit 0 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-regression.log` | `527ee64606703554163d036b5210754c29f96267d42a584327f96e799ca399d9` |
| Historische Fixformatierung, Exit 1 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-fix-fmt.log` | `68fa3fae4a7dca2c36766556f88605557903e5a084f3cb2ec9abaf24e7359257` |
| Historische Baselineformatierung, Exit 1 | `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-baseline-fmt.log` | `68fa3fae4a7dca2c36766556f88605557903e5a084f3cb2ec9abaf24e7359257` |

Weitere Bindungsdateien:

- `/tmp/tb-b01-pruefbindung-vFV7lm8L/checks.json`, SHA256 `da4bd31c3fddaf200eef814408e12d9b3e46ad55c1f074eb48d7a48f6bf018f0`: konkrete Befehle, Umgebung, Start/Ende, tatsächliche Exitcodes, Loghashes und Bindungsvergleiche je neuer Prüfung.
- `/tmp/tb-b01-pruefbindung-vFV7lm8L/initial.json`, SHA256 `1e03aa1836b4b5964568b65d0bd3bbbe0a25f2e6ee1d2abbc7bc3c15e0e913a4`.
- `/tmp/tb-b01-pruefbindung-vFV7lm8L/final.json`, SHA256 `00bec9802e188304eaa952359bcd81875ec2505058b176354c8275450da7ff35`.
- `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-proof.json`, SHA256 `ae805f87bff68906f26a016377c666f279ee61de3bb1427cf5d13bf24ab76ea3`.
- `/tmp/tb-b01-pruefbindung-vFV7lm8L/check-current.py`, SHA256 `2771af69f0b30d2598bef64e9aef65a84aac0f2484f0d43feb2c07e9888967c7`: ausführbarer einmaliger Prüfrunner, keine Produktivfunktion.
- `/tmp/tb-b01-pruefbindung-vFV7lm8L/diff-check.log`, `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-fix-source-equality.log` und `/tmp/tb-b01-pruefbindung-vFV7lm8L/historical-baseline-source-equality.log`: jeweils Exit 0, leerer Inhalt, SHA256 jeweils `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

## Tatsächlich offene Grenzen

1. Vollsuite weiterhin 24 rote, exakt gleiche Baselinefehler. Das ist kein grüner Gesamtlauf und kein Nachweis sämtlicher DB-Pfade. Ohne nachgewiesenen DB-Zugriff gilt ein still zurückkehrender Test nicht als DB-Beleg. Der neue Regressionstest enthält bewusst keine DB-Prüfung.
2. Paketformatierung bleibt mit 34 belegten Bestandsabweichungen rot. Keine fremden Dateien formatiert.
3. Clippy bleibt im unveränderten `tb-chat`-Abhängigkeitscode blockiert. Das ältere Baseline-Clippy auf `51c8a674a371d0e623687940e6b8ca3492f96c92` hat einen anderen Rust-Gesamtbaum und ist deshalb kein unveränderter vollständiger Baseline-Nachweis. Der jüngste Baseline-Clippy-Log ist leer und wird ebenfalls nicht übernommen. Keine vollständige Zielpaket-Clippy-Freigabe behauptet.
4. `/home/nathanael/.worktrees/tb-vollreview-idempotenz/rust/test-database.json` fehlt; ausschließlich Existenz geprüft. Keine Zugangsdaten oder ENV-Dateien gelesen. Keine Datenbank, Migration, Dienste, echten Konten oder Browser verändert.
5. Integration auf fortgeschrittenes main einschließlich B02 bleibt außerhalb dieser Rolle. Kein Fetch, Rebase, Commit, Push, Merge, Release, Deploy oder Restart durchgeführt; kein Gate erneut aufgerufen und kein neues ALLOW abgeleitet.
