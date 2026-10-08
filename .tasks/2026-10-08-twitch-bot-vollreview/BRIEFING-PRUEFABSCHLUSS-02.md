# Prüfabschluss B04, B06 und B07

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min als Orientierung, keine Kompilierungs-Abbruchfrist | Worktree: je Paket unten

## Auftrag und Grenzen

Frischer Sol-Prüfworker je Paket, anschließend frischer read-only Fix-Kritiker. Grundvertrag und bestätigte Claims stehen in BRIEFING-W02-FIXGRUPPE-02.md. Anwendungscode nicht ändern. Alle vorigen Fixer dieser Pakete sind beendet. B04 erhielt bislang keine echte Commit-Diff-Kritik; B06/B07-Kritiker brachen mit API 403 ohne Urteil ab. Diese einmalige neue Kritik ersetzt fehlende Ergebnisse, nicht ein ungünstiges Urteil. Bei erneutem API-Ausfall offen zurückgeben, kein Modellwechsel und keine Endlosschleife.

Der ursprüngliche Nutzerauftrag umfasst Fix, Commit und Integration. Die hier gesicherten lokalen Commits sind ausdrücklich zur Prüfung bestimmt. Dieser Prüfauftrag erlaubt Basisabgleich eigener unveröffentlichter Commits, aber keinen Main-Merge, Push, Release, Deploy oder Restart. Bei notwendiger Quellkorrektur oder Rebase-Konflikt mit Beleg zurückgeben, damit ein frischer Fixer übernimmt. Keine Hooks, Gatezustände oder Schutzmechanismen ändern.

## Pakete

### B04: Routerverträge

- Worktree `/home/nathanael/.worktrees/tb-vollreview-router-vertraege`, Branch `fix/vollreview-router-vertraege`.
- Prüfcheckpoint c0383531 auf bd69502728861f8279e8b044592ecac9058fc732. Vollständigen SHA selbst feststellen. Genau `rust/crates/tb-dashboard-api/src/lib.rs`, 180 Einfügungen und sechs Löschungen. Astra sicherte den unveränderten Sol-Diff, ohne Anwendungscode zu editieren.
- Claims: W02-DA01-S004-correctness-1 und W02-DA01-S004-security-2. Browserkodierte Kündigungsroute sowie vorhandene interne Adminberechtigung des Partner-Linkrouters. Produktionsguards nicht abschwächen.
- Baseline 1333 bestanden, 35 fehlgeschlagen; Fix 1336 bestanden, dieselben 35 fehlgeschlagen, jeweils mit `--include-ignored --test-threads=1`. Drei neue Regressionen und sämtliche neun Routerfälle bestanden im Gesamtlauf. Logs `/tmp/tb-b04-router-pruefungen/baseline-test.log` und `/tmp/tb-b04-router-pruefungen/post-fix-test.log`.
- Eigene Datei vor und nach Fix formatiert; Paket-fmt vorher 265 Abweichungen, nachher kein Slot. Clippy offen. Früheres ALLOW: no reviewable changes war kein Patchreview.
- Erstfixer aed18ccb4194e9d4c, 162 Sol-Datensätze, SHA256 c7aff3e1231adfb8d78252566516d95155d1ee991a615d13d2ca362c7a73831e. Leerdiff-Kritiker a324f168560f348e8, 54, SHA256 03247f3f251ea53a088872eaf99dbc5ec36f8379f39f972348d56aae82cc1d51. Von Astra geprüft.

### B06: Antwortgrenze

- Worktree `/home/nathanael/.worktrees/tb-vollreview-proxy-antwort`, Branch `fix/vollreview-proxy-antwort`. Eigene Baseline `tb-vollreview-proxy-antwort-baseline`.
- Head 46c52a928b47444b10364d024a632ca00226d6c4, Basis a8b5b5e986a1de0b8e2f981651f83bda9cf400dd. Genau `rust/crates/tb-dashboard-api/src/proxy.rs`.
- Claim W02-DA01-S005-errors-1: vorhandene 16-MiB-Antwortgrenze beim Puffern tatsächlich durchsetzen. Korrekte Antworten und sonstige Proxyverträge erhalten.
- 14 gezielte Proxytests bestanden. Gesamtsuite 1334 bestanden, dieselben 31 Fehler wie Baseline mit 1331 bestandenen Tests, jeweils sechs ignoriert. Eigene Datei formatiert; Paket-fmt vor/nach Fix mit ausschließlich fremden verbleibenden Abweichungen. Clippy offen, weil kein Slot erreicht wurde.
- Sol-Gate ALLOW am genannten Head und der Basis, `/tmp/tb-b06-gate.log`, Reviewzustand 7510de26b74f4c6f.json. Nach Basiswechsel nicht unbesehen wiederverwenden. Kritiker ohne Urteil ausgefallen.
- Erstfixer a8751a0f816a56a4c, 178 Sol-Datensätze, SHA256 45e27d4b4201ea2a4bb4e1bb1428840f6e95d726406c6acd6af063ad9c62d914. Von Astra geprüft.

### B07: Plan-Testfixture

- Worktree `/home/nathanael/.worktrees/tb-vollreview-plan-fixture`, Branch `fix/vollreview-plan-fixture`.
- Head 9ec607b31a68d2721f6c8905d4268fd5d4290ca6, Basis a8b5b5e986a1de0b8e2f981651f83bda9cf400dd. Genau `rust/crates/tb-dashboard-api/tests/plan_stufen_gates.rs`.
- Claim W02-DA01-S007-correctness-1: synthetische Partneridentitäten konsistent konfigurieren, ohne Assertions oder produktive Guards abzuschwächen.
- Fokus-Suite nach Fix elf bestanden, ein Fehler; davor drei bestanden und neun Fehler. Logs `/tmp/tb-b07-plan-baseline.log` und `/tmp/tb-b07-plan-fix.log`. Geprüfter Commit c419c0d4b3c5dc8148ce2c5f4f1dc05aef0cc6b6 soll sich vom endgültigen Head lediglich in zwei Taskdokumenten unterscheiden. Identität prüfen statt Tests ohne Anlass wiederholen.
- Verbliebener TikTok-Test hat keine tiktok_options und erhält deshalb 400. Dieser unabhängige Restfehler wurde nicht geändert. Kein entsprechender Zusatzfixauftrag, keine echten Anbieteraufrufe. Prüfer trennt Restfehler, ursprünglichen Claim und neue Regressionen.
- Eigene Datei vor/nach Fix formatiert. Vollständige Paketformat-Baseline und Clippy fehlen. Sol-Gate ALLOW am genannten Head, Reviewzustand 7dbce9b52973fd1f.json; frischer Kritiker ohne Urteil ausgefallen.
- Erstfixer ad671d5f3f2756d20, 184 Sol-Datensätze, SHA256 51523fb2f574c091a99dd2374ba183e73fe9cf374d35fccb66c3b8fd96ee1641. Von Astra geprüft.

## Prüfweg

1. Eigenen Zustand und volle SHAs feststellen. Aktuelles origin/main holen, eigene unveröffentlichte Commits bei konfliktfreiem unverändertem fachlichem Diff abgleichen. Ein Git-Schritt je Bash-Aufruf, literale absolute Pfade. Fremder Hauptcheckout und fremde Arbeit bleiben unangetastet.
2. Bestehende Nachweise zuerst über exakten Rust-Baum, Kommando, Voraussetzungen und Exit-Codes binden. Dateialter reicht nicht. Passende bereits ausgeführte Nachweise müssen nicht wiederholt werden. Fehlende eigene Datei-rustfmt, Paket-fmt, Clippy oder Tests samt passender Baseline vervollständigen. Verwendete Testflags nicht still ändern oder ungleiche Fehlerzahlen vergleichen.
3. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, `/home/nathanael/.local/bin/cargo-slot`, Kompilierung `--jobs 1`. Testdatenbank `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`. Normale Kompilierung als verfolgte Hintergrundaufgabe fertiglaufen lassen. Eigene bereits laufende Aufgaben nicht duplizieren. Fremde Prozesse, Slots und Dienste nicht verändern. Testdatenbankdateien höchstens auf Existenz prüfen.
4. Sol-only-Gate auf dem tatsächlichen nichtleeren Fix-Commit: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <eigener literaler Worktree> --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080`. Kein --chain, Modellfallback oder Hook-Bypass. Bei BLOCK unveränderten Stand und konkrete Mängel zurückgeben; keine Fixrunde im selben Kontext.

Ausschließlich gpt-6.1-sol. Keine Delegation, weiteren Threads, ListAgents, SendMessage oder Nutzerfragen. Vor Codesuche Skill code-suche und Graphify. Keine Secrets/ENV-Dateien, Produktionsdatenbank, Migration, Browser oder echten Kontoaktionen; ai-coach nicht anfassen. Taskdokumente gehören Astra.

## Rückgabe

Strukturiert: tatsächliche Basis/Head-SHAs, sauberer Worktree, Quelländerung seit Prüfcheckpoint, Diffidentität, Prüfkommandos und Exit-Codes, Baseline, Gate samt Belegen, eigene laufende Hintergrundaufgaben und offene Blocker. Ein unsauberer, leerer oder unerwartet geänderter Diff startet keine Kritik. Der folgende Kritiker liest den festen echten Gesamtdiff ohne Builds, Tests oder Probes. Fachliches ALLOW ersetzt keine fehlenden Prüfungen und keine Deployfreigabe.
