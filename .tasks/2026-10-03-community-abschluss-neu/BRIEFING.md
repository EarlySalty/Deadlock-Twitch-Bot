[Orchestrator] D2: Community-Streamer-Brücke live bringen (Twitch #1035 und Bots #472)

Bereichsordner und Quelle dieses Briefings: `/home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-10-03-community-abschluss-neu/BRIEFING.md`. Alle Status- und Steuerungsdateien neben diesem absoluten Pfad, außerhalb der Worktrees.

Rolle: Blatt-Worker, ein Agent für beide Repos, weil Schreib- und Lesepfad zusammengehören. Auftraggeber: D2, T3 `ea03e82a-8d90-409f-8e6d-af7e36024305`, CODEX_THREAD_ID `01a10326-ed18-7ff3-b139-e617577b1233`. Kopf: Hauptsession T3 `92efdb66-6e7e-495c-873e-32f2912c2fa2`. Steuerung in `VON_D2.md` neben dieser Datei, Status ausschließlich in `AN_D2.md`; `AN_HAUPT.md` und `REGISTER.md` schreibt D2.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90 und /home/nathanael/.worktrees/sol-abschluss-7b50cc4c87b8e9ee

Du bist nicht allein in den Repos. Nur die Community-Anteile beider vorhandenen Arbeitsbranches und die nötigen Deploymentdateien gehören dir. Fremde Änderungen erhalten; Serverguide, Brain und C9 weder ausbauen noch ohne klare Zuordnung entfernen. Produktcode ausschließlich in den Worktrees ändern. Der Abschnitt „Feste Regeln“ in `/home/nathanael/repos/Deadlock-Brain/.tasks/2026-10-03-brain-fertigstellung/DELEGATOR-REGELN.md` gilt auch für dich.

## Ziel

Zuschauer-Punkte, Clip-Einreichung, Community-Scouting (Twitch) und Twitch-Verknüpfung, Punkte-Leaderboard, Clip-Contest (Discord) laufen live, gemergt auf beiden main.

## Ausgangslage

- Twitch: `~/.worktrees/sol-abschluss-51eb094dc5242b90`, HEAD `0e2dc5cc`, 26 Commits auf aktuellem origin/main. Viele nicht committete Dateien unter `.tasks/2026-10-02-community-abschluss/` sind alte Prüfakten, kein Produktcode.
- Bots: `~/.worktrees/sol-abschluss-7b50cc4c87b8e9ee`, HEAD `499b8e4a`, 21 Commits auf origin/main, sauber.
- Fachvertrag: `.tasks/2026-10-02-community-abschluss/CONTRACT.md` und `DEPLOYCHECKLISTE.md` im Twitch-Worktree. Die fachlichen Regeln dort gelten (Sessiongrenzen, Wiedereinwilligung, Scout einmal 150 je Kanal, Clip-Einreicher-Privacy). Die alten Wrapper-, Manifest-, Hash- und FD-Vorgaben gelten nicht mehr.
- Alte Twitch-Übergabe: `/home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90/.tasks/2026-10-02-community-abschluss/STAND.md`.
- Alte Bots-Übergabe: `/home/nathanael/Documents/.tasks/2026-10-02-offene-branches/status/sol-7b50cc4c87b8e9ee-haltepunkt/STAND.md`.
- Früherer Hauptabschluss: `/home/nathanael/Documents/.tasks/2026-10-03-branch-restbefunde/STAND.md`. Alle drei alten Threads sind jetzt ready. Twitch-Branch `sol/abschluss/51eb094dc5242b90`, Bots-Branch `sol/abschluss/7b50cc4c87b8e9ee`.
- Bots enthält laut STAND.md selektive C9-Anteile. Die zurückgestellten Brain-/Guide-/C9-Erweiterungen gezielt aus der auszuliefernden Community-Quelle trennen; den bisherigen Kopf vorher über eine Git-Referenz erhalten. Nur belegte Hunks entfernen, bei unklarer Bereichskopplung an D2 melden.
- Offen war: Kopierprobe im Bots-Assetlauf scheiterte an `bwrap: Cannot allocate memory`, Clippy danach nicht erreicht; Gate für beide Köpfe ohne gültiges Urteil. Twitch meldete außerdem zwei Gitleaks-Funde im Push-Hook. Befunde ohne Klartextausgabe prüfen, echte Secrets über den bestehenden Weg behandeln und den Hook nicht umgehen.
- Serverguide, Brain-Anteile und C9-Erweiterungen sind nicht Teil von W5.

## Weg

1. Beide Branches auf frischen origin/main rebasen, falls nötig.
2. Prüfen nach `/home/nathanael/Documents/.tasks/2026-10-02-offene-branches/HOSTPROBE.md`: fmt, clippy und Tests der betroffenen Crates je Repo. Die Bots-Kopierprobe einmal wiederholen; scheitert sie wieder an Speicher, ohne Sandbox-Namespace prüfen oder überspringen und das im Bericht sagen.
3. Gate für beide gemeinsam: je Repo `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo <worktree> --base <origin/main-SHA> --head <HEAD-SHA>`, direkt nacheinander. Einziger Reviewer ist das Gate. BLOCK: Befunde in `AN_D2.md` melden und den Turn beenden; D2 startet dafür einen frischen Fixer. Kein Urteil (Exit 2): einmal wiederholen, dann in `AN_D2.md` melden und anhalten. Ohne gültiges ALLOW keine Migration und kein Merge oder Deploy.
4. Reihenfolge laut `DEPLOYCHECKLISTE.md`: Migrationen und Rollen zuerst (Twitch: neue Migration als `postgres` anwenden, Rechte an `twitchbot`/`twitchdash`, Version in `_sqlx_migrations`; Bots: zentral über `dl-central-db`), dann Bots vor Twitch mergen und deployen, Sync-Timer erst nach beiden Seiten und einem erfolgreichen manuellen Sync.
5. Merge je Repo, `git push origin HEAD:main`. Deploy Twitch über `/usr/local/bin/deploy-twitch-release <sha>` (Skill `deploy-restart-selbstdienst`), Bots über den bestehenden Bots-Weg, Dienste neu starten.
6. Live prüfen: Dashboard-Route und Discord-Leaderboard liefern echte Daten, ein manueller Punkte-Sync läuft durch, Journale ohne Fehler.
7. In `AN_D2.md` melden: main-SHAs, angewandte Migrationen, Live-Belege. Beide PRs nach dem Push nach main schließen. Beide übernommenen Arbeitsbranches vor dem Löschen über eine reguläre Git-Referenz sichern; dann deren Worktrees und Branches entfernen. Fremde Branches und Worktrees erhalten.

## Arbeitsregeln

Ein Auftrag, keine Unter-Orchestratoren, keine Subagenten, keine neuen T3-Threads. Früh committen und pushen. Status bei Meilensteinen und spätestens alle 20 Minuten, je Meldung höchstens 5 Zeilen: erledigt, als Nächstes, Blocker; Belege nur Befehl, Exit, Testzahl, SHA. Keine PIDs, keine Beweisprosa, keine eigenen Wrapper oder Manifeste. Selbst entscheiden, nur bei Produktfragen, Geld, Datenverlust oder Konflikt mit einem anderen Bereich an D2 melden. Secrets nur über Infisical/FD, keine ENV-Konfiguration, Rust only, keine Code-Kommentare, keine angewandte Migration ändern, keine fremden Prozesse beenden, kein Botmodellwechsel, keine Community-Ankündigung ohne Go des Nutzers. Für eigene Texte und Auftragsdateien humanizer und no-em-dashes lesen und anwenden: natürliches Deutsch, echte Umlaute, keine Gedankenstriche oder Ersatz-Gedankenstriche. Keine Extras, Refactorings, zusätzlichen Tests oder Dokumentationsausbauten. Fremde Funde nur in `SPAETER.md` melden. Als letzten Schritt nach Fertigmeldung den eigenen Thread mit `t3-thread.py settle --selbst` abschließen.
