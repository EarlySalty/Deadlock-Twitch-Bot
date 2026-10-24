[Orchestrator] D2: Bannänderungen bei erster historischer Aggregation zuverlässig vormerken, danach Community live abschließen.

Bereichsordner `/home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-10-03-community-abschluss-neu/`. Auftraggeber D2: T3 `ea03e82a-8d90-409f-8e6d-af7e36024305`, CODEX_THREAD_ID `01a10326-ed18-7ff3-b139-e617577b1233`. Kopf T3 `92efdb66-6e7e-495c-873e-32f2912c2fa2`. Status nur `AN_D2.md`, Steuerung `VON_D2.md`; D2 schreibt Register und Hauptbericht.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90 und /home/nathanael/.worktrees/sol-abschluss-7b50cc4c87b8e9ee

Frischer Blatt-Worker für genau dieses Paket, keine weiteren Agenten oder Threads. Du bist nicht allein im Repo; fremde Änderungen erhalten. Lies `BRIEFING.md`, `BRIEFING-FIX-R1.md`, die neueste Meldung in `AN_D2.md`, `VON_D2.md` und das vollständige neue Urteil `gate-twitch-fix-r1.log`. Die Delegatorregeln unter `/home/nathanael/repos/Deadlock-Brain/.tasks/2026-10-03-brain-fertigstellung/DELEGATOR-REGELN.md` gelten. Nur das Wichtige bauen, keine Refactorings oder zusätzliche Härtung.

## Stand und Eigentum

- Twitch-Worktree `/home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90`, Branch `sol/abschluss/51eb094dc5242b90`, HEAD `b9462ca44ab458e0874d27f0f62e9b05d63f43de`, Basis `8db73ec3779239cd237262c4392a2c612f16c6e0`, regulär gepusht und sauber.
- Bots-Worktree `/home/nathanael/.worktrees/sol-abschluss-7b50cc4c87b8e9ee`, Branch `sol/abschluss/7b50cc4c87b8e9ee`, HEAD `7d64e0b96fb2917088f6049b8d71597f313966c6`, Basis `5e6598f9d82ea8250c34e854c42a41de13f06efc`, unverändert, gepusht und sauber. Vorhandene grüne Bots-Prüfungen erhalten.
- Fix R1 ist ready und angehalten. Twitch-Gate erneut BLOCK, Exit 1; Bots-Gate noch nicht gestartet. Keine produktive Migration, kein main-Push, Deploy oder Cleanup.
- Auf diesem Twitch-Stand: `cargo test -p tb-analytics --lib community_points::tests --jobs 3` Exit 0, 32 Tests; gezielter Contest-Dispatchtest Exit 0, 1 Test; Clippy tb-analytics/tb-chat und gezieltes rustfmt Exit 0.
- Contestaktion und Antwort wurden in Brave mit lokalen Fixtures sichtbar geprüft. Belege: `/home/nathanael/.claude/sichtpruefung/community-d2/contest-aktion.png` und `contest-antwort.png`. Keine neue Browserprobe nötig, solange diese Quelle unverändert bleibt.

Du besitzt die nötigen Hunkänderungen in `rust/migrations/20261003110000_community_activity_dirty_days.sql` und unmittelbar nötige bestehende Analytics-Testpfade. Community-Vertrag, Twitch-ID-Fix und vorhandene Tests erhalten. Kein Brain, Guide oder C9. Produktcode ausschließlich in den Worktrees ändern. Vor Migrationseingriff lesend bestätigen, dass die Datei weiterhin nicht angewandt ist; angewandte Migrationen niemals ändern.

## Blockierender Befund

Die Banntrigger finden historische Tage nur in veröffentlichten Viewer-Tageswerten. Eine Bannänderung nach `load_bans` während der ersten Aggregation findet noch keine solche Zeile und schreibt keinen Dirty-Marker. Danach werden veraltete Punkte veröffentlicht und bisherige Generationen bestätigt; der Tag wird nicht mehr gerechnet.

Das ist der zweite Befund derselben Fehlerfamilie. Prüfe daher systematisch die betroffenen Wege beider Banntabellen bei Insert, Update und Delete, einschließlich noch unveröffentlichter historischer Rohaktivität und der Generationserfassung der Aggregation. Tage zuverlässig vormerken, ohne von vorhandenen Tageswerten abhängig zu sein. Einen gezielten Nachweis für die erste Aggregation ergänzen: Banndaten wurden bereits gelesen, die Bannänderung wird vormarkiert, die Aggregation veröffentlicht ihren alten Stand, und der nächste Lauf muss den offenen Tag neu rechnen. Bestehende historische Bann-/Entbannungsprüfungen erhalten.

Der neue Presence-Mitternachtshinweis ist NIT und wird vorerst in SPAETER.md geführt. Nicht zusätzlich bauen. Die Screenshotbelege bestehen bereits; falls der vorhandene Gateweg Referenzen aufnehmen kann, darauf verweisen, aber weder Gatehook noch Transport erweitern.

## Weg bis fertig

Früh committen und regulär pushen. Nur nötige Regressionen und betroffene Crates nach HOSTPROBE.md prüfen, drei Build-Slots und jobs 3, fmt ohne Sperre. Keine neue Vollsuite auf unverändertem Bots-SHA. Neue Mainstände bei Bedarf regulär übernehmen und finale Quellen gegen beide exakten Basen direkt nacheinander über `gate_hook.py --review` prüfen. Einziger Reviewer ist das Gate.

BLOCK: vollständige Befunde an D2 und Turn beenden, keine eigene nächste Fixrunde. Exit 2: einmal wiederholen, dann melden und anhalten. Nach beiden ALLOW den autorisierten Abschluss aus BRIEFING.md und DEPLOYCHECKLISTE.md selbst vollenden: Migrationen und Rechte, Bots vor Twitch nach main pushen und deployen, Neustarts, echte Leaderboard- und Punkte-Sync-Belege, Sync-Timer danach, PRs schließen, Branch-/Worktreeabbau nach SHA-Backup. Keine Community-Ankündigung.

Status bei Meilensteinen und spätestens alle 20 Minuten, höchstens 5 Zeilen pro Meldung. Belege nur Befehl, Exit, Testzahl, SHA und nötige Belegpfade. Routine selbst entscheiden; Produktfrage, Geld, Datenverlust oder Bereichskonflikt an D2. Secrets nur Infisical/FD, kein Klartext und keine ENV-Konfiguration, Rust only, keine Code-Kommentare oder Botmodellwechsel. Alle eigenen Texte mit humanizer und no-em-dashes prüfen: natürliche deutsche Formulierungen, echte Umlaute, keine Gedankenstriche. D2 übernimmt den Threadabschluss.
