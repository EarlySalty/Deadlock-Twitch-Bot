# Nachprüfung der offenen PRs

Stand: 24. September 2026. Keine Produktiv-Auslieferung. Die weiter geltende Entscheidung liegt in `/home/nathanael/.worktrees/claude-config-pr-first-testbetrieb-20260924/orchestrierung/PR-FIRST-TESTBETRIEB.md` und wird über `/home/nathanael/CLAUDE.md` eingebunden.

## Erneut ausgeführte lokale Prüfungen

Cargo/Rust 1.98.0, `--locked -j 2`, bestehende Feature-Worktrees. Keine Produktionssecrets geladen, keine echte Werbung oder Veröffentlichung ausgelöst.

| Quellstand | Prüfung | Ergebnis |
|---|---|---|
| Twitch 5f92ca7c1aeb0c0ce1c841aac880bf80c4dec446 | Eigener Wegwerf-Timescale-Container ohne Activity-Schema; Statusleser, Entscheider, Sicherheitsfälle, Store | 51 bestanden, 0 Fehler, 0 ignoriert |
| Twitch gleicher Quellstand | adManager.test.ts und verwaltungTabs.test.ts | 18 bestanden, 0 Fehler, 0 übersprungen |
| Brain 11664151b73e8dd7dd73f6f84e4aed8d9aa57171 | CLI, reguläre Veröffentlichungsprüfung, Payload-Roundtrip | 72 bestanden, 0 Fehler, 0 ignoriert |
| Discord 961cf15ef50d640e7a674f1d9cd87793f3817c44 | dl-brain, gefilterte Brain-Bot-Tests, Mock-CLI, bestätigte Veröffentlichung | 26 bestanden, 0 Fehler, 0 ignoriert |

167 ausgewählte Tests erneut bestanden. Die Filter ersetzen keine vollständige Workspace-Abnahme. Der Discord-Stand enthält die fortgesetzte lokale Negationshärtung und wurde nach Formatierung getestet, committed und gepusht. Rustfmt für das geänderte neue Modul und `git diff --check` bestanden. Testjobs: j-1790229943-11476, j-1790230214-11515, j-1790230567-11558, jeweils Exit 0; zusätzliche kurze Tests liefen unmittelbar über den MCP-Aufruf.

## GitHub-Prüfungen

### Twitch PR #958

https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/958

Beobachteter Implementierungs-SHA: `5f92ca7c1aeb0c0ce1c841aac880bf80c4dec446`.

- Run https://github.com/EarlySalty/Deadlock-Twitch-Bot/actions/runs/35948870217, Versuch 1: Offline-Build erfolgreich; Schema-Gate und Uplink/OAuth-Testjob fehlgeschlagen.
- Schema-Job 107472848958: Die gepinnte Abhängigkeit dbrain-builds am Brain-Commit d8c34270868e129098e12243f53f5b52ee507b8b benötigt beim Online-SQLx-Check Brain-Tabellen, die in dieser CI-Testdatenbank nicht vorhanden sind. Erster Compilerfehler: brain.hero_catalog in dbrain-builds/src/engine.rs:103; weitere fehlende Brain-Tabellen folgen.
- Uplink/OAuth-Job 107472849035: 8 Tests bestanden, 6 fehlgeschlagen, 0 ignoriert; die sechs Callback-Tests erwarten HTTP 200 und bekommen HTTP 500.
- Manifest-Scope-Run https://github.com/EarlySalty/Deadlock-Twitch-Bot/actions/runs/35948870055 erfolgreich.
- Cargo.toml, Cargo.lock, rust-sqlx-check.yml und raid_oauth_impl.rs sind zwischen Basis 1442640c und Implementierung 5f92ca7c unverändert, mit `git diff --exit-code` bestätigt. Keine pauschale Behauptung, der gesamte Basisstand sei getestet. Der gesonderte CI-Umbau wird nicht durch Scanner-Ausnahmen, abgeschwächte Assertions oder neue Produktionsrechte ersetzt.

### Steam PR #69

https://github.com/EarlySalty/Deadlock-Steam-Bot/pull/69

SHA: `7cfacba9d7db068fccb3e05a2de3f598cc1fa244`.

Run https://github.com/EarlySalty/Deadlock-Steam-Bot/actions/runs/35946064711 wurde als Versuch 2 erneut angefordert. Jobs 107514031829 und 107514032061 wurden nicht gestartet. GitHub nennt fehlgeschlagene Kontozahlungen oder das Ausgabenlimit als Ursache. Abrechnungseinstellungen wurden nicht verändert; kein durchgelaufener CI-Test.

### Brain PR #11

https://github.com/EarlySalty/Deadlock-Brain/pull/11

SHA: `11664151b73e8dd7dd73f6f84e4aed8d9aa57171`. Im untersuchten Feature-Stand ist kein `.github/workflows`-Verzeichnis vorhanden. Der sichtbare GitGuardian-Check ist erfolgreich, aber kein funktionaler CI-Nachweis. Der separate CI-Auftrag bleibt davon getrennt.

### Discord PR #451

https://github.com/EarlySalty/Deadlock-Bots/pull/451

Der ursprüngliche SHA 1de9176434b0025fc53e20b8e9a210c6f568a528 wurde mit den Runs 35948523256, 35948523480 und 35948523406 jeweils als Versuch 2 erneut angefordert. Auch diese Jobs 107514068677, 107514071091 und 107514074126 wurden wegen desselben GitHub-Abrechnungs-/Ausgabenlimit-Problems nicht gestartet. Anschließend wurde die geprüfte Negationshärtung als `961cf15ef50d640e7a674f1d9cd87793f3817c44` gepusht. Die alten Runs sind keine Abnahme des neuen SHA; dessen Read-back wird im PR festgehalten.

## Unabhängiger Review

Die Review-Threads 709f5813-a8d4-4dac-ace2-051a68fb5507 und 2da9b7e2-865e-477d-aad9-bce32cee9aa3 konnten sich nicht anmelden: abgelaufene Claude-OAuth-Session, Erneuerung fehlgeschlagen. Der Rollenresolver meldete GLM und Grok bis 12:48 sowie Astra bis 09:53 am 24. September gesperrt; dies ist die Werkzeugmeldung, keine Garantie der späteren Verfügbarkeit. Beide erfolglosen Threads wurden aus der Seitenleiste genommen und dürfen nicht als fertiger Review zählen. Es gibt kein unabhängiges Modellurteil und keine Freigabe zum Merge.

## Verbleibende Abnahme

GitHub-Abrechnungs-/Ausgabenlimit-Blocker klären; die belegten CI-Schema-/OAuth-Fehler im bestehenden CI-Auftrag prüfen; funktionalen Brain-CI-Pfad bereitstellen; unabhängige Review-Anmeldung erneuern. Danach die aktuellen PR-SHAs erneut testen. Produktions-Auslieferung und echter In-Game-Funktionsbeweis bleiben zusätzlich durch den PR-first-Testbetrieb gesperrt.

MERGEPROTOKOLL[MS-1]: kein Merge: PR-first-Testbetrieb
LIVEBEWEIS[DV-1]: nicht ausgeführt: PR-first-Testbetrieb

## Nachtrag 2026-09-24 (CI-Fix-Session)

- Schema-Gate: brain-Schema-Fixture `rust/schema-gate/brain-schema.sql` plus
  Workflow-Schritt in `rust-sqlx-check.yml`. `.sqlx`-Cache final per
  `cargo sqlx prepare --workspace -- --all-targets` (online, Wegwerf-DB mit
  Twitch-Migrationen + Fixture) neu erzeugt: 17 fehlende Queries ergänzt,
  Bestand sonst unverändert. Prüfung: `prepare --workspace --check` bestanden
  (Hinweis auf test-target-Queries ist erwartbar), Offline-Build und
  Offline-Testkompilierung (tb-chat Titelbefehl) bestanden.
- Uplink/OAuth-Job: Ursache der sechs 500er war die fehlende Spalte
  `twitch_partners.raid_admin_enabled` im callback_tests-Fixture (Prod-
  Migration 20260913153000). Fixture ergänzt; 14/14 Callback-Tests lokal grün,
  übrige Suiten des Jobs (tb-raid, tb-transport-twitch, tb-internal-api)
  ebenfalls grün.
- Details: `.tasks/2026-09-24-werbemanager-ci-fix/AUFTRAG.md`.
- Unverändert blockiert: GitHub-Actions-Billing für Steam #69 und Bots #451;
  unabhängiges Review steht noch aus.
- Zweiter CI-Lauf (dd98827a): Schema-Gate kompiliert mit Fixture sauber,
  scheiterte nur noch am Cache-Vergleich; Uplink-Job lief bis zum Schritt
  „Titelbefehl und Dashboard-Schalter" durch und scheiterte an Offline-Queries
  ohne Cache-Eintrag. Ursache beider: `prepare --check` erfasst alle Targets,
  das Write-Modus-Flag `-- --workspace` nur Nicht-Test-Ziele — der Cache muss
  daher per `--all-targets` erzeugt werden. Behoben in dd98827a-Folgecommit.
