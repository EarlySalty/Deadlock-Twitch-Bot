# Nachweis vom 2026-10-07

## Implementierung und Installation

- Quelle: `ops/systemd/deploy-twitch-release`, ursprüngliche installierte Datei bytegleich zu origin/main 10dacbc2.
- Gemergt: 06182c3750716b70aee027e145827c1b771875b7, Implementierungscommit 4540c5e1.
- Installiert root:root, 0755 nach `/usr/local/bin/deploy-twitch-release`, bytegleich zur gemergten Quelle.
- SHA256: 15fc5400d38955c7baa3a77762c04ec1bdc379ce3b3d9f6e7c47c19c9beca223.
- Das Konto gpt existiert auf diesem Host nicht. Installation deshalb mit dem vorhandenen `sudo -n install`, ohne Passwort oder Secretzugriff.

## Prüfung

`bash -n` und `shellcheck` für den Wrapper: Exit 0.
`/home/nathanael/.local/bin/uv tool run pytest -q /home/nathanael/.worktrees/twitch-deploy-pruefmodus/ops/systemd/test_deploy_twitch_pruefen.py`: 3 passed, 14 subtests passed, 0 skipped.
Fehlerfälle: deleted, falscher SHA, fremder exe-Pfad, MainPID 0, inaktive Unit, ungültige NRestarts, systemctl-Fehler, unlesbare exe, fehlendes/ungültiges current, PID-/current-Wechsel und zusätzliche Argumente.

Zwei direkte Bash-Aufrufe `/usr/local/bin/deploy-twitch-release --pruefen`: Exit 0, kein Hook-Deny. Keine Hookänderung oder allgemeine sudo-Freigabe erforderlich. Prüfpfad vor Deploy-Lock, Migrationen und Restarts.

| Unit | MainPID beider Prüfungen | NRestarts | Zustand |
|---|---|---|---|
| deadlock-twitch-bot-rust | 3874592 | 0 | active |
| deadlock-twitch-dashboard-rust | 3874704 | 0 | active |
| deadlock-twitch-stream-coaching-watch | 3821592 | 0 | active |
| tb-category-collector | 3796733 | 0 | active |

Alle vier exe-Ziele ohne `(deleted)` unter `/opt/deadlock/twitch/releases/10dacbc2376a63f6d91869afe83b1ac8bb615eec/rust/target/release/`, derselbe SHA wie current. Keine Dienste durch diesen Auftrag neu gestartet, weil ausschließlich ein Verwaltungsskript geändert wurde. Journal aller vier System-Units mit `-p err --since '1 minute ago'`: Exit 0, 0 Einträge, keine Lesewarnung.

TESTNACHWEIS[TW-1]: 3 passed, 0 ignored | Baseline: nicht erhoben, kein Altfehler behauptet
LIVEBEWEIS[DV-1]: PID 3874592/3874704/3821592/3796733 unverändert | exe ohne (deleted) | journal -p err leer | Anker "--pruefen" im installierten Skript | Funktion: Exit 0, vier Units auf current-SHA | Ort: /usr/local/bin/deploy-twitch-release --pruefen
WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: Quelle und installierter Wrapper bytegleich belegt | Fremddienst-Pfade: 0/0 geprüft

## Geschützter kanonischer Checkout

`/home/nathanael/repos/Deadlock-Twitch-Bot` steht weiterhin auf d8284816 mit 310 fremden Änderungen. `git merge --ff-only origin/main` bricht wegen überlappender lokaler Änderungen und untracked Dateien ab. Keine fremde Arbeit überschrieben, gestasht oder zurückgesetzt. Remote-main und Wrapperinstallation sind aktuell; das Nachziehen dieses geteilten Checkouts bleibt ein separater Blocker. claude-config wurde regulär nachgezogen; ursprünglicher Auftrag unter AUFTRAG-ORCHESTRATOR.md erhalten.
