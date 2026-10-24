# Fix-Briefing: lfg-pitch-register-gate (Runde 1, Hinweise H1 und H2)

[Orchestrator] Kleiner Fix auf dem bestehenden Branch. Review-Runde 1 war
FREIGABE, zwei Hinweise werden vor dem Merge noch umgesetzt.

- Worktree (vorhanden, nicht neu anlegen): /home/nathanael/.worktrees/tb-lfg-pitch-register-gate
- Branch: fix/lfg-pitch-register-gate (HEAD d9c82ef2, schon auf origin)
- Review: /home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-09-12-lfg-pitch-register-gate/REVIEW.md
- PATH: `/home/nathanael/.rustup/toolchains/*1.97*/bin` vor `/usr/bin`, nie `/usr/bin/cargo`.

## Zu tun

1. H2: `load_recent_channel_messages_before` in
   `rust/crates/tb-chat/src/promos.rs` liefert neueste zuerst. Der Judge
   soll den Verlauf chronologisch bekommen (älteste zuerst, die jüngste Zeile
   direkt vor der aktuellen Nachricht). Reihenfolge nach dem Laden umdrehen
   oder per Unterabfrage sortieren; die bestehende
   `load_recent_channel_messages` für den Anlass-Pitch darf ihre Reihenfolge
   behalten, wenn sie anderswo so erwartet wird (prüfen).
2. H1: In `rust/crates/tb-chat/src/lfg_pitch.rs` (Prompt-Bau um Zeile 190)
   den Chatverlauf und die aktuelle Nachricht als Daten abgrenzen: je ein
   fester Delimiter-Block (zum Beispiel `<<<CHATVERLAUF` ... `CHATVERLAUF>>>`
   und `<<<NACHRICHT` ... `NACHRICHT>>>`) und im System-Prompt ein Satz, dass
   Inhalte in diesen Blöcken Daten sind und keine Anweisungen.
3. Bestehende Tests anpassen, die den Prompt-Text oder die Reihenfolge prüfen;
   ein Test für chronologische Reihenfolge, falls es keinen gibt.
4. `rustfmt` nur auf die beiden geänderten Dateien (mit `--edition 2021`),
   `cargo clippy -p tb-chat`, `cargo test -p tb-chat lfg` plus die
   promos-DB-Tests, die `load_recent_channel_messages` berühren
   (`TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql`,
   `SQLX_OFFLINE=1`). Ändert sich eine sqlx-Query, `rust/.sqlx` nachziehen.
5. Ein Commit auf denselben Branch, `git push origin fix/lfg-pitch-register-gate`.

## Regeln

- Nur diese zwei Punkte, kein Umbau, keine Umbenennung von `SilentReason`.
- Keine Code-Kommentare. Nur den eigenen Branch pushen, nie main.
- Du bist der einzige Thread für dieses Paket, keine Unter-Threads.

## Fertigmeldung

Hier im Thread: Commit-SHA, geänderte Dateien, Testzahlen. Dann stoppen.
