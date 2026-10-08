# Fixpaket B02: Übergang zum ersten OBS-Live-Stream

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vollreview-obs-start

## Ziel und Sollvertrag

Befund `W02-DA01-S004-concurrency-1`, Klasse B. Der erste `/obs/ws`-Socket kann seinen Datenbank-Nachlauf beenden, bevor der unabhängig gestartete PostgreSQL-Listener LISTEN registriert. Ein Ereignis in diesem Fenster geht für den offenen Socket still verloren. Der Listener übernimmt beim Erststart den aktuellen maximalen Ereigniswert, ohne die fehlende Zeile nachzuliefern.

Sollbelege am Review-SHA `0ecae1370f1a80d1a101249b5c932663d69be8af`: `rust/crates/tb-dashboard-api/src/obs/ws.rs:15-19,42-53` verlangt einen lückenlosen oder ausdrücklich als Lücke gemeldeten Übergang. `obs/bus.rs:883-932` und `obs/ws.rs:1405-1438` enthalten verwandte Replay-/Live-Verträge, ersetzen in Tests aber den tatsächlichen Listener. Ursachenpfade: `obs/ws.rs:484-498,755-766`, `obs/bus.rs:393-405,440-470`. Der vorhandene Writer liegt in `rust/bin/tb-bot/src/obs_dock.rs:205-238`.

Der Fall setzt `obs_docks.enabled=true` voraus. Standardmäßig ist der Schreibpfad aus; die produktive Einstellung wurde nicht gelesen. Keine Behauptung eines Live-Vorfalls. Funktionierende Verbindungen nach wirksamem LISTEN sind nicht betroffen.

## Unabhängige Freigabe

- Reviewer `af1906e0e42b5f7a4`, Workflow `wf_cdc4c5ac-9bb`: 42 Sol-Nachrichten; SHA256 `71d7637553660d13912ab3100e3a4979b62a4f23cc23796d6288b70e3c64d36e`.
- Skeptiker 1 `a2cc748228287326d`: BESTÄTIGT, B, Soll eindeutig; 45 Sol-Nachrichten; SHA256 `6646a49fb2ccd981edebda298448a4eeef0730460ba07b3073f10bd756375976`.
- Skeptiker 2 `aaf00c9fc2e9ce972`: BESTÄTIGT, B, Soll eindeutig; 32 Sol-Nachrichten; SHA256 `065482fc7583cbb081ddab4e05a58add0ca600c680d3ba3dc6b0ced46a1c3ffc`.

Beide Skeptiker liefen in `wf_3b16d4aa-68e` mit frischem unabhängigen Kontext und erhielten nur Behauptung und Ort. Astra hat sämtliche Modellfelder der drei abgeschlossenen Transcripts geprüft. Auth, Flag, Routereinbindung, Wiederverbindung, Broadcast-Lag und bestehende Tests sind berücksichtigt. Keine Live- oder DB-Prüfung.

## Eigentum und Arbeitsstand

Frischen Worktree `/home/nathanael/.worktrees/tb-vollreview-obs-start` und Branch `fix/vollreview-obs-start` von aktuellem, frisch geholtem `origin/main` anlegen. Vorhandenen eigenen Stand bei Wiederaufnahme erst prüfen, nie doppelt anlegen. Fremde Worktrees und Branches bleiben unangetastet.

Schreibrecht ausschließlich:

- `rust/crates/tb-dashboard-api/src/obs/bus.rs`
- `rust/crates/tb-dashboard-api/src/obs/ws.rs`

Tests möglichst in bestehenden Modulen. Keine andere Datei ohne belegte notwendige Vertragserweiterung anfassen; dann an Astra zurückgeben. Keine neue Konfiguration, Abhängigkeit, Migration, Kommentare, Umbenennung, breite Formatierung oder Nebenfixes. Keine Änderungen am Feature-Schalter, Writer, Aufbewahrungsvertrag oder an intakten Live-Verbindungen. Falls eine neue Produkt- oder Timeout-Policy nötig wäre, als C zurückgeben statt sie zu erfinden.

## Nachweis und Grenzen

Vor Suche code-suche und Graphify. Referenzgraph `/home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json`, Quellcode im eigenen aktuellen Worktree. Zunächst prüfen, ob aktuelles main den Defekt bereits behoben hat.

Minimalen Fix für das belegte Erststartfenster und einen günstigen deterministischen Nachweis der betreffenden Reihenfolge anstreben. Keine echten Konten, keine Produktionsereignisse erzeugen, kein DB-Produktionszugriff. Gesunde spätere Listener-Verbindungen, Replay-Deduplizierung, Lag-/Reconnect-Wiederherstellung und bestehende Antwortverträge erhalten.

Vor Prüfungen rolle-test-waechter, vor Git rolle-merge-schleuse laden. Details in OPS-PREFLIGHT.md. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, vorhandenes `/home/nathanael/.local/bin/cargo-slot`, bei Kompilierung `--jobs 1`. Test-DB `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`; `rust/test-database.json` nur auf Existenz prüfen, keine Zugangsdaten lesen. Eigene Dateien formatieren; paketbezogen Formatprüfung, Clippy und bestehende Tests von `tb-dashboard-api`. Fremde bestehende Fehler als Baseline dokumentieren, nicht korrigieren. Normale eigene Kompilierung nicht nach wenigen Minuten abbrechen. Vorhandene Slotmechanik verwenden, keine fremden Prozesse oder Dienste verändern.

Nur eigene Dateien stagen und committen. Trailer `Co-authored-by: GPT 6.1 Sol <gpt-6.1-sol@local>`. Sicherung des eigenen Arbeitsbranches erlaubt, kein Force-Push und kein Push nach main.

Eigener finaler Gate:

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vollreview-obs-start --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080
```

Bei BLOCK Urteil und Stand zurückgeben; Astra startet einen frischen Fixer. Kein Modellrückfall, keine Hook- oder Zustandsänderung. Nach Abgabe folgt ein frischer Sol-Fix-Kritiker. Merge, Release, Deploy und Restart sind in dieser Runde nicht freigegeben. Der Deploy-Wrapper-Konflikt ist weiterhin offen.

## Rückgabe

Status, Basis-/Head-SHA, genaue geänderte Dateien, Prüfaufrufe mit echten Exit-Codes, Baseline, Gate-Urteil und Zustand des eigenen Worktrees. Keine grünen Tests behaupten, wenn sie nicht gelaufen sind. Keine Berichtdateien schreiben; Astra hält die Artefakte.

Native Workerrolle, ausschließlich `gpt-6.1-sol`. Keine eigenen Agenten oder Threads, kein ListAgents/SendMessage, keine Nutzerfragen, kein Browser, keine Secrets/ENV-Dateien, kein `ai-coach`. Auftraggeber Astra `c88f4057-c6b3-4c54-8750-addd08b24b42`, Hauptauftraggeber `819f0d87-8d3f-4fbf-8c4c-ada2bf290f5b`.
