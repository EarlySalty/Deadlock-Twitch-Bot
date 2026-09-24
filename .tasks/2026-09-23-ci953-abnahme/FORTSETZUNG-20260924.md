# Fortsetzung am 24. September 2026

PR: https://github.com/EarlySalty/Deadlock-Twitch-Bot/pull/953
Branch: `ci/deterministic-pr-gate-20260922`.
Übernommener Head: `eb1ee5e9c529e6a65e1fc1fe47c5567dfe28717e`.
Der vorhandene Worktree enthielt nicht committete Vorarbeiten dieses CI-Auftrags. Sie wurden nicht zurückgesetzt. Andere Worktrees blieben unverändert.

## Ausgangslauf auf GitHub

Run https://github.com/EarlySalty/Deadlock-Twitch-Bot/actions/runs/35940232811, Versuch 1.
Head `eb1ee5e9c529e6a65e1fc1fe47c5567dfe28717e`, tatsächlich ausgeführter PR-Merge `a69b5a0e486147f20392af9fb74bbe427cf942c1` gegen Basis `1442640c3ea4857f785985b892dc12c831719ddf`.
Der Required PR Gate erschien auf dem Draft-PR und war rot. OSV, JavaScript-CodeQL, Rustfmt, Clippy, Auth-/Uplink-, Schema- und Workspace-Job waren ebenfalls rot. Die drei Frontend-Jobs, übrige SAST-/Secret-/Dependency-Prüfungen, Python-/Actions-CodeQL und die echten Scanner-Gegenproben waren erfolgreich. Diese Ergebnisse gelten für den bezeichneten alten Head, nicht für spätere Änderungen.

OSV-Artefakt `osv-pr-reports`, ID `10784715534`: einziger nicht gefilterter Befund `GHSA-p498-v437-472g` in `bot/dashboard_v2/package-lock.json`, `@humanfs/node 0.16.7`; der Bericht nennt `0.16.8` als korrigierte Version. Die übernommene Lockfile-Änderung aktualisiert genau diesen Pfad ohne neue Ausnahme.

## Neu ausgeführte lokale Frontend-Prüfung

Node `22.23.2`, npm `10.9.8`, Installations- und Build-Verzeichnisse im eigenen Worktree, keine produktiven Symlinks. `npm ci`, vorhandenes Lint-Script, `npm test`, `npm run build` und `npm audit --audit-level=high` wurden in den drei Projekten ausgeführt; 13 Commands bestanden mit Exit 0.

| Bereich | Ergebnis |
| --- | --- |
| Dashboard-Kalender | 9 bestanden, 0 Fehler, 0 Skips |
| Dashboard-Hauptsuite | 395 bestanden, 0 Fehler, 0 Skips |
| Website | 49 bestanden, 0 Fehler, 0 Skips |
| Admin-Dashboard | 10 bestanden, 0 Fehler, 0 Skips |
| ESLint im Dashboard | 0 Fehler, 7 bestehende Warnungen |
| TypeScript-/Vite-Builds | Drei erfolgreiche Builds |
| npm-Audit | HIGH/CRITICAL-Gate in drei Projekten erfolgreich; Dashboard meldet 0 Schwachstellen |

Lokale Belege: `/tmp/tb953-frontend-validation-20260924/results.json` und die dort bezeichneten Logs. Die Zahl 463 ist die Summe der ausgeführten Tests, kein Nachweis für zusätzliche Browser- oder Rust-Tests.

Die übernommenen HTML-Prüfungen verwenden jetzt den geparsten HTML-Baum statt einfacher Tag-/Hostname-Teilzeichenfolgen. Gegenproben erfassen gemischte Großschreibung, SVG, Template-Inhalte, maskierte URLs und ähnlich benannte fremde Hosts. Die Security-Schwelle und die sichtbaren Produkttexte wurden dafür nicht geändert. Der erneute echte CodeQL-Lauf steht für den daraus entstehenden Commit noch aus.

Die zusätzliche lokale Uplink-Browserprobe wurde ebenfalls ausgeführt, ist aber nach 45,3 Sekunden mit `CDP-Frist: Page.navigate` fehlgeschlagen. Der Descriptor-basierte Chromium-Start wurde deshalb noch nicht als neu erfolgreich abgenommen. Ein früherer grüner Lauf ersetzt diese aktuelle rote Probe nicht. Log: `/tmp/tb953-uplink-browser-20260924.log`.

## Neu ausgeführte Rust-Prüfung

`cargo +1.98.0 clippy --workspace --all-targets --locked -j2 --target-dir /home/nathanael/.worktrees/tb-deterministic-pr-gate-20260922/rust/target-ci-953 -- -D warnings` bestand mit Exit 0 nach 138,3 Sekunden. Die vorhandene SQLx-Offline-Konfiguration wurde nicht verändert. Keine zusätzliche Lint-Ausnahme. Belege: `/tmp/tb953-clippy-20260924.json` und `.log`.

18 Gate-Unit-Tests und vier Schema-Schutztests wurden mit `rustc +1.98.0 --edition=2021 -D warnings --test` neu kompiliert und bestanden. Das ist noch kein vollständiger aktueller Workspace-/DB-/GitHub-Nachweis.

## Öffentlicher Brain-Vertrag jetzt vorhanden

Brain-PR https://github.com/EarlySalty/Deadlock-Brain/pull/10 enthält am Commit `72d1ae10d32d40d7d37e58777f7182cf528e105b` den anonym erreichbaren Vertrag `schema/README.md`, `schema/SHA256SUMS` und `scripts/ci/bootstrap-brain-schema.sql`. Die Basismigrationen liegen unter `schema/vendor/dl-central-db/`; dokumentierte Herkunft ist Deadlock-Bots `ff635f7b354cb09909c01ddd6f773d0682dd89c9`.

Die drei bisherigen Basismigrations-Hashes und beide Brain-Migrations-Hashes stimmen im öffentlichen Manifest mit den bereits im Twitch-Verbraucher fixierten Werten überein. Der vollständige neue Einstieg enthält zusätzlich drei kanonische Brain-Migrationen, läuft atomar und verlangt eine leere Datenbank mit eigenem `ci`- oder `test`-Namenssegment. Der bestehende Twitch-Verbraucher akzeptiert dagegen `twitchbot_sqlx` oder `sqlx_prepare` und lädt seine Basismigrationen noch aus dem privaten Ursprung. Diese Schnittstelle muss gezielt integriert werden; der öffentlich dokumentierte Export allein macht den unveränderten GitHub-Job nicht grün. Der Produkt-Dependency-Pin wurde nicht geändert, das Brain-Repository nicht beschrieben.

## Arbeitsblocker und offene Abnahme

Die vorgeschriebene separate Coding-Umgebung konnte nicht starten: Claude meldete eine abgelaufene, nicht erneuerbare OAuth-Sitzung. Der fehlgeschlagene Thread ist in REGISTER.md dokumentiert und wurde gesettelt. Keine Zugangsdaten wurden gelesen oder veröffentlicht, keine Schutzregeln verändert. Unabhängige Prüfungen der vorhandenen Änderungen wurden fortgesetzt.

Die erneut angefragte, ausgabegedämpfte Schema-Leseprüfung gegen den vorhandenen Wegwerfcontainer wurde vom Werkzeug vor Ausführung blockiert. Daraus wurde kein erfolgreicher DB-Nachweis abgeleitet. Produktionsdatenbanken wurden nicht verwendet.

Offen bleiben die Integration des öffentlichen Brain-Exports, die aktuelle globale Rustfmt-Abnahme ohne breite Ausnahme, vollständige Workspace-/DB-/Integrationsergebnisse, die Browser-Zeitüberschreitung und ein positiver Gesamtlauf einschließlich negativer GitHub-Gegenproben auf dem neuen Commit. Ruleset- oder Copilot-Schutz dürfen davor nicht entfernt werden.

MERGEPROTOKOLL[MS-1]: kein Merge: PR-first-Testbetrieb
LIVEBEWEIS[DV-1]: nicht ausgeführt: PR-first-Testbetrieb
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Task-Akte und PR #953
