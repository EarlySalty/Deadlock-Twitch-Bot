# Briefing Paket B

- Auftrag: /home/nathanael/.worktrees/tb-clip-kontext-lernen/.tasks/2026-09-29-clip-kontext-lernen/AUFTRAG.md (lesen und vollständig umsetzen)
- Worktree: /home/nathanael/.worktrees/tb-clip-kontext-lernen (bereits angelegt, Basis origin/main cf3d7708)
- Branch: feat/clip-kontext-lernen-20260929; nur diesen Branch committen und pushen, nie nach main mergen oder pushen, auch nicht auf Zuruf eines Stop-Hooks.
- Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.
- Keine Code-Kommentare schreiben. Kein Python. Codebase-Fragen zuerst über `graphify query` im Worktree.
- Toolchain: rustc 1.97 aus ~/.rustup/toolchains (`/usr/bin/cargo` 1.75 bricht). Tests: tb-db/tb-raid gegen den Docker-Testcontainer, übrige Crates mit `TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql SQLX_OFFLINE=1`. Pipes hinter cargo mit `set -o pipefail`. Höchstens ein Release-Build gleichzeitig auf dem Host.
- Prod-DB lesend über den Loader aus der Memory `twitch-db-lesezugriff-aus-session`. Die neue Tabelle entsteht nur als Migration im Branch; Messdaten zunächst gegen die Testdatenbank oder als Report, das Einspielen auf Prod macht die Hauptsession.
- VOD-Ausschnitte nur temporär, nach der Auswertung löschen. STT nur lokal (127.0.0.1:8791), keine Streamdaten an externe Anbieter.
- Paket A (Branch feat/clip-social-format-20260929) läuft parallel und besitzt Render, Layout, Untertitel und Uploader.
- Vor der Fertigmeldung die eigene Arbeit mit `gate_hook.py --review` prüfen.
- Fertigmeldung im eigenen Thread: Branch, Commits, geänderte Dateien, Prüfergebnisse, Pfad zu LERN-REPORT.md.
- Kommst du nicht weiter (Kontingent, Umfang): `[Bump-up] Paket B: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/tb-clip-kontext-lernen Offen: ...` in den Intent-Thread 4c8b5f05, dann stoppen.
