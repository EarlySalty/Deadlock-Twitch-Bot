[Orchestrator] D2: Community-Gate Runde 1 beheben und beide Repos live abschließen.

Bereichsordner: `/home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-10-03-community-abschluss-neu/`. Auftraggeber D2: T3 `ea03e82a-8d90-409f-8e6d-af7e36024305`, CODEX_THREAD_ID `01a10326-ed18-7ff3-b139-e617577b1233`. Kopf T3: `92efdb66-6e7e-495c-873e-32f2912c2fa2`. Status ausschließlich `AN_D2.md`, Steuerung `VON_D2.md`; D2 schreibt `AN_HAUPT.md` und `REGISTER.md`.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90 und /home/nathanael/.worktrees/sol-abschluss-7b50cc4c87b8e9ee

Du bist frischer Blatt-Worker für genau diesen Community-Abschluss, ohne weitere Agenten oder Threads. Du bist nicht allein in den Repos; fremde Änderungen erhalten. Lies zuerst `BRIEFING.md`, `AN_D2.md`, `VON_D2.md` und das vollständige Urteil `gate-twitch.log` im Bereichsordner. Die Regeln in `/home/nathanael/repos/Deadlock-Brain/.tasks/2026-10-03-brain-fertigstellung/DELEGATOR-REGELN.md` gelten. Nur notwendige Änderungen, keine Extras oder Refactorings.

## Übernommener Stand

- Twitch: Worktree `/home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90`, Branch `sol/abschluss/51eb094dc5242b90`, HEAD `af4664fa35576397667f28f4df1529464c53a410`, Basis `aeff9c21807b64916bd5dacdb41b0569dcc4c131`.
- Bots: Worktree `/home/nathanael/.worktrees/sol-abschluss-7b50cc4c87b8e9ee`, Branch `sol/abschluss/7b50cc4c87b8e9ee`, HEAD `7d64e0b96fb2917088f6049b8d71597f313966c6`, Basis `5e6598f9d82ea8250c34e854c42a41de13f06efc`.
- Beide Köpfe regulär gepusht, getrackte Arbeitsbäume sauber; W1 ist ready und angehalten. C9 wurde getrennt, alter Bots-Kopf `499b8e4a` unter `backup/community-vor-trennung-20261003` erhalten. Nicht erneut bauen oder zurücknehmen.
- Beide Community-fmt- und Clippy-Prüfungen Exit 0, Twitch 73 unterschiedliche Tests bestanden. Bots-Kopierprobe mit verdecktem Worktree grün; Broker 52, Community 578, DB 10, Sync 13, Migrator 6, Launcher 11 bestanden. Ein fremder Reaction-Roles-Fixturefehler steht in SPAETER.md; kein Gesamt-PASS.
- Twitch-Gate Runde 1: BLOCK, Exit 1. Bots-Gate noch nicht gestartet. Keine Migrationen, kein main-Push, kein Deploy und kein Cleanup.

## Zu beheben und zu belegen

1. `rust/migrations/20261003110000_community_activity_dirty_days.sql:69`: Änderungen an `twitch_ban_events` und `twitch_chatter_global_ban` müssen die betroffenen historischen Tage neu zur Aggregation vormerken. Nach Bann und Entbannung dürfen alte Punktewerte nicht bestehen bleiben. Insert, Update und Delete berücksichtigen. Zuerst prüfen, welche Migrationen tatsächlich angewandt sind; keine angewandte Migration ändern.
2. `rust/crates/tb-analytics/src/community_points.rs`, `load_seen_before`: Entdeckerhistorie über die stabile Twitch-ID zuordnen. Kanalumbenennung muss frühere Besuche erhalten, ein neu vergebener Login darf keine fremde Historie erben.
3. Gatehinweis `rust/crates/tb-chat/src/commands.rs:541`: Erreichbarkeit von `!clipcontest` durch `command_names::resolve_command` belegen, vorhandenen passenden Test verwenden oder bei Bedarf einen gezielten Regressionstest ergänzen.
4. Gatehinweis `bot/dashboard_v2/src/pages/SocialMedia.tsx:2529`: Contestaktion und Antwortzustand im Browser prüfen, Screenshot unter `~/.claude/sichtpruefung/community-d2/` ablegen und im Status auf den Pfad verweisen. Dafür keinen neuen Produktumfang bauen.

Eigentum: die genannten Twitch-Pfade und unmittelbar nötige bestehende Testpfade; Bots-Communityquelle unverändert übernehmen, sofern sich kein nötiger gemeinsamer Anschluss ergibt. Keine Brain-, Guide- oder C9-Erweiterung. Für Prosa humanizer und no-em-dashes anwenden, echtes Deutsch mit Umlauten, keine Gedankenstriche, keine Code-Kommentare. Secrets ausschließlich Infisical/FD, kein Klartext und keine ENV-Konfiguration.

## Abschluss

Nach minimalem Fix früh committen und regulär pushen. Nur betroffene Crates und notwendige Regressionen nach HOSTPROBE.md prüfen: drei Build-Slots, jobs 3, fmt ohne Sperre, Release zusätzlich exklusiv. Vorhandene grüne Bots-Prüfungen auf unverändertem SHA erhalten, keine neue Vollsuite.

Danach die regulären Gates für beide endgültigen Quellen direkt nacheinander mit exakten origin/main- und HEAD-SHAs ausführen. Einziger Reviewer ist das Gate. BLOCK mit vollständigen Befunden an D2 melden und den Turn beenden, nicht selbst eine weitere Fixrunde beginnen. Exit 2 einmal wiederholen, danach an D2 melden und anhalten. Nach zwei ALLOW den vorhandenen Ablauf aus BRIEFING.md und DEPLOYCHECKLISTE.md autonom vollenden: nötige Migrationen und Rollen, Bots vor Twitch nach main pushen und deployen, Dienste neu starten, Leaderboard und manuellen Punkte-Sync live belegen, Timer danach aktivieren, PRs #472 und #1035 schließen, Worktrees und Arbeitsbranches nach SHA-Backup löschen. Keine Community-Ankündigung.

Bericht bei Meilensteinen und spätestens alle 20 Minuten, höchstens 5 Zeilen pro Meldung. Belege: Befehl, Exit, Testzahl, SHA, nötige Screenshotpfade. Unklarer Bereichskonflikt oder Produktentscheidung an D2; Routine selbst entscheiden. Nur dieses Paket abschließen.
