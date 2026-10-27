# Fix-Runde für die PRs mit NACHBESSERN (2026-09-29)

Du bist der einzige Thread für deinen PR. Keine Unter-Threads oder Unter-Agenten.

## Auftrag

Die Befunde aus dem Review-Kommentar deines PRs beheben, so dass der PR mergefähig ist. Der Kommentar-Link steht in deiner ersten Nachricht. Nur die dort genannten Befunde und was zwingend daraus folgt, kein Scope-Zuwachs, keine Umbenennungen.

## Regeln

- Arbeiten im eigenen Worktree `/home/nathanael/.worktrees/tb-fix-<nr>` auf dem PR-Branch (nicht detached). Nie im geteilten Checkout `~/repos/Deadlock-Twitch-Bot`.
- Vorher `git fetch origin` und den Branch auf aktuelles `origin/main` rebasen (main hat seit gestern 22 Dependabot-Merges). Konflikte sauber lösen.
- Keine Code-Kommentare schreiben. Echte Umlaute in Nutzertexten, keine Em-Dashes. LLM-Aufrufe nur über `tb_llm`, keine Modellnamen einkompilieren.
- Migrationen: eine noch nicht auf Prod angewandte Migration dieses PRs darf geändert werden; prüfe per `psql` lesend auf `_sqlx_migrations`, ob die Version schon eingetragen ist (Zugang: Memory `twitch-db-lesezugriff-aus-session.md`). Ist sie angewandt, neue Migration nachlegen.
- Prüfungen: `cargo fmt` für geänderte Dateien, `cargo clippy` und `cargo test` für die betroffenen Crates (Toolchain 1.97 aus `~/.rustup/toolchains`, jeder Befehl mit `timeout 1200`, kein `--release`, Pipes nur mit `set -o pipefail;`). Frontend: `npm ci`, Build, Tests im betroffenen Ordner. Vorbestehende rote Tests auf origin/main sind Baseline.
- `.sqlx`-Offline-Dateien und den Schema-Snapshot mitziehen, wenn sich Queries ändern.
- Selbstprüfung vor der Fertigmeldung: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review` gegen deinen Stand (Basis origin/main) und BLOCKING-Funde in derselben Runde beheben.
- Commits auf dem PR-Branch, dann auf den PR-Branch pushen (Force-with-lease nur, wenn der Rebase das erfordert; nie auf main). Kein Merge nach main, kein Deploy, kein Neustart. Das Merge macht ein unabhängiger Review-Thread.
- Unter den PR einen Kommentar: je Befund aus dem Review-Kommentar eine Zeile "erledigt: <was>" oder "offen: <warum>". Deutsch, echte Umlaute, keine Em-Dashes.

## Schlussmeldung

```
FIX PR #<nr>: FERTIG <head-sha> | BLOCKIERT (Grund)
Befunde: <n erledigt>/<n gesamt>, offen: <Liste>
Geprüft: <Befehle und Ergebnis>
Migrationen: <Dateien, angewandt ja/nein>
Selbstprüfung: <gate_hook --review Ergebnis>
```
