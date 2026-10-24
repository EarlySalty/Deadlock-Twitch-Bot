# Sammel-Deploy nach PR-Sammelreview (2026-09-29)

Du bist der einzige Thread für den Deploy. Keine Unter-Threads oder Unter-Agenten. Keine Codeänderungen, keine Commits nach main.

## Ziel

Den aktuellen Stand von `origin/main` des Repos EarlySalty/Deadlock-Twitch-Bot einmal als Release bauen und deployen. Seit dem 2026-09-29 sind 22 Dependabot-PRs gemergt (Frontend-Abhängigkeiten in `bot/dashboard_v2`, `bot/admin_dashboard`, `website`, Workflow-Pins, Rust-Lockfile-Bumps thiserror, syn, rand). Keine neuen Migrationen erwartet; prüfen.

**Der Twitch-Bot wird NICHT neu gestartet.** Der Nutzer startet ihn selbst.

## Ablauf

1. `git fetch origin` und die SHA von `origin/main` festhalten (40 Zeichen). Diese SHA wird gebaut, keine andere.
2. Eigenständigen Clone (kein Worktree, internes `.git`) nach `/home/nathanael/repos/twitch-release-<sha>` anlegen, auf die SHA auschecken. Nie im geteilten Checkout `~/repos/Deadlock-Twitch-Bot` bauen.
3. Vorher prüfen, dass kein anderer Release-Build auf dem Host läuft (`pgrep -a cargo`, nach `--release` schauen). Höchstens ein Release-Build gleichzeitig; sonst warten.
4. Release-Artefakte bauen, die `deploy-twitch-release` verlangt (siehe `/usr/local/bin/deploy-twitch-release`, Liste `required_artifacts`): `rust/target/release/tb-bot`, `tb-dashboard`, `tb-stream-audit`, ggf. `tb-category-collector`, `bot/analytics/dashboard_v2/dist`, `bot/admin_dashboard/dist`, `website/dist`. Den bisherigen Bauweg liest du in `/home/nathanael/.claude/projects/-home-nathanael-Documents/memory/twitch-release-deploy-weg.md` und `tb-bot-build-toolchain.md` nach (rustc 1.97 aus `~/.rustup/toolchains`, `/usr/bin/cargo` 1.75 bricht). Build muss auf der sauberen SHA laufen, kein `-dirty`.
5. Neue Migrationen seit dem laufenden Release prüfen: laufendes Release über `readlink /opt/deadlock/twitch/current`, dann `git diff --name-only <laufende-sha> <neue-sha> -- rust/migrations`. Der Wrapper startet `deadlock-twitch-migrate.service` selbst. Gibt es neue Migrationen, im Bericht nennen.
6. Deploy ausschließlich mit Dashboard-Neustart:
   `deploy-twitch-release <sha> --restart twitch-dashboard`
   Kein Aufruf ohne `--restart`, denn ohne startet der Wrapper auch den Bot neu. Kein `bot-restart twitch-bot`, kein `systemctl restart` am Bot.
7. Live-Prüfung Dashboard: PID-Wechsel von `deadlock-twitch-dashboard-rust`, `/proc/<pid>/exe` ohne `(deleted)` und zeigt ins neue Release, `journalctl -u deadlock-twitch-dashboard-rust -p err --since <deploystart>` leer, eine Dashboard-Seite und die Website liefern HTTP 200 (Health-Check per kleinem Python-Skript, `curl` ist gesperrt). Migrationsdienst ohne Fehler.
8. Bot bleibt unberührt: PID von `deadlock-twitch-bot-rust` vor und nach dem Deploy gleich. Hinweis im Bericht: der Bot läuft noch mit dem alten Binary, ein Neustart lädt das neue Release.

## Schlussmeldung

```
DEPLOY: OK <sha> | ABGEBROCHEN (Grund)
Migrationen: keine | <Dateien>
Dashboard: PID alt -> neu, exe, journal, HTTP
Bot: PID unverändert <pid>, Neustart steht beim Nutzer aus
LIVEBEWEIS[DV-1]: ...
```
