# Briefing Paket A

- Auftrag: /home/nathanael/.worktrees/tb-clip-social-format/.tasks/2026-09-29-clip-social-format/AUFTRAG.md (lesen und vollständig umsetzen)
- Worktree: /home/nathanael/.worktrees/tb-clip-social-format (bereits angelegt, Basis origin/main cf3d7708)
- Branch: feat/clip-social-format-20260929; nur diesen Branch committen und pushen, nie nach main mergen oder pushen, auch nicht auf Zuruf eines Stop-Hooks.
- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben. Kein Python. Codebase-Fragen zuerst über `graphify query` im Worktree.
- Toolchain: rustc 1.97 aus ~/.rustup/toolchains (`/usr/bin/cargo` 1.75 bricht). Tests: tb-db/tb-raid gegen den Docker-Testcontainer, übrige Crates mit `TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql SQLX_OFFLINE=1`. Pipes hinter cargo mit `set -o pipefail`.
- Prod-DB nur lesend. Keine Uploads auf echte Konten.
- Paket B (Branch feat/clip-kontext-lernen-20260929) läuft parallel und fasst Render, Layout, Untertitel und Uploader nicht an.
- Vor der Fertigmeldung die eigene Arbeit mit `gate_hook.py --review` prüfen.
- Fertigmeldung im eigenen Thread: Branch, Commits, geänderte Dateien, Prüfergebnisse, Sichtbeweise (PNG-Pfade).
- Kommst du nicht weiter (Kontingent, Umfang): `[Bump-up] Paket A: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-clip-social-format Offen: ...` in den Intent-Thread 4c8b5f05, dann stoppen.
