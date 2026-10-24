[Orchestrator] D2: Clip-Retry und Scout-Cursor beheben, anschließend beide Community-Pfade live abschließen.

Bereichsordner `/home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-10-03-community-abschluss-neu/`. Auftraggeber D2: T3 `ea03e82a-8d90-409f-8e6d-af7e36024305`, CODEX_THREAD_ID `01a10326-ed18-7ff3-b139-e617577b1233`. Kopf T3 `92efdb66-6e7e-495c-873e-32f2912c2fa2`. Status nur `AN_D2.md`, Steuerung `VON_D2.md`; D2 führt Register und Hauptbericht.

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90 und /home/nathanael/.worktrees/sol-abschluss-7b50cc4c87b8e9ee

Frischer Blatt-Worker für genau dieses Paket, keine weiteren Agenten oder Threads. Du bist nicht allein in den Repos; fremde Änderungen erhalten. Lies `BRIEFING.md`, die neueste Meldung in `AN_D2.md`, `VON_D2.md` und das vollständige Urteil `gate-twitch-fix-r2.log`. Die Regeln unter `/home/nathanael/repos/Deadlock-Brain/.tasks/2026-10-03-brain-fertigstellung/DELEGATOR-REGELN.md` gelten. Nur die beiden aktuellen blockierenden Punkte bauen, keine Extras oder Refactorings.

## Übernahme

- Twitch-Worktree `/home/nathanael/.worktrees/sol-abschluss-51eb094dc5242b90`, Branch `sol/abschluss/51eb094dc5242b90`, HEAD `d50012e318499041b154115e425a549438501d57`, Basis `6e3fbcd500394b3f9ec1d6f577f89707b36d5ec0`, regulär gepusht und sauber.
- Bots-Worktree `/home/nathanael/.worktrees/sol-abschluss-7b50cc4c87b8e9ee`, Branch `sol/abschluss/7b50cc4c87b8e9ee`, HEAD `7d64e0b96fb2917088f6049b8d71597f313966c6`, Basis `5e6598f9d82ea8250c34e854c42a41de13f06efc`, unverändert, gepusht und sauber. Bots-Prüfungen auf unverändertem SHA erhalten.
- Fix R2 ist ready und angehalten. Drittes Twitch-Gate BLOCK, Exit 1; noch kein Bots-Gate, keine produktive Migration, kein main-Push, Deploy oder Cleanup.
- Banntrigger erfassen unveröffentlichte Chat-/Präsenzdaten; 34 Analyticsfälle, Clippy und gezieltes rustfmt Exit 0. Frühere Twitch-ID- und Contest-Dispatchfixes erhalten, nicht erneut bauen.
- Bestehende Browserbelege: `/home/nathanael/.claude/sichtpruefung/community-d2/contest-aktion.png` und `contest-antwort.png`; lokale Contestaktion, Annahme und gesperrte Wiederholung geprüft. Keine neue Browserprobe bei unverändertem Frontend, keine Erweiterung des Gate-Transports. Presence-Mitternacht bleibt NIT in SPAETER.md.

Eigentum: `rust/crates/tb-chat/src/clip_contest_submit.rs`, `rust/crates/tb-scout/src/community.rs` und unmittelbar nötige bestehende Testpfade oder additive Migrationen. Produktcode nur in den Worktrees. Fachvertrag und DEPLOYCHECKLISTE.md aus dem alten Communityordner gelten; keine angewandte Migration ändern. Serverguide, Brain und C9 bleiben außerhalb des Pakets.

## Aktuelle BLOCK-Befunde

1. `clip_contest_submit.rs`, `submit_inner`: Die einzige abschließende Ergebnisspeicherung prüft beim UPDATE nur `clip_id`, ohne Claimidentität oder Versuch. Nach Ablauf der 120-Sekunden-Lease kann ein neuer Versuch erfolgreich abschließen, bevor der alte Versuch `failed` schreibt. Dadurch wird eine angenommene Einreichung wieder geöffnet und aus der Tageszählung entfernt. Abschluss nur für den tatsächlich aktuellen Claim beziehungsweise Versuch zulassen. Erste Claimzeit `submitted_at`, ursprünglichen Einreicher und Retry-/Privacyregeln erhalten. Gezielt belegen, dass ein verspäteter alter Abschluss einen neueren Erfolg nicht überschreibt.
2. `community.rs`, `vorschlag_einreichen`: `AlreadyKnown` aktualisiert den Kandidatencursor nur bei geänderter Zahl unterschiedlicher Vorschlagender. Ein neuer Schlüssel desselben Vorschlagenden mit früherem `submitted_at` kann `is_first_eligible` auf ein anderes Paar verschieben. `liste_ergebnisse` aktualisiert den neuen Vertreter, lässt aber den Cursor des verdrängten Vorschlagenden stehen. Inkrementelle Leser behalten dann dessen veraltetes `true`. Beide betroffenen Paare mit neuem Cursor sichtbar machen und die inkrementelle Abfrage gezielt prüfen. Originalzeit, Scoutepoche, Pagination und einmalige 150 Punkte je öffentlichem Kanal erhalten.

## Prüfung und Abschluss

Früh committen und regulär pushen. Nur betroffene Crates und nötige Regressionen nach HOSTPROBE.md prüfen, drei Build-Slots und jobs 3, fmt ohne Sperre, Release zusätzlich exklusiv. Keine Vollsuite auf unveränderten Quellen. Neue Mainstände bei Bedarf regulär übernehmen, finale Quellen mit exakten Basis-/HEAD-SHAs über den bestehenden Gatehook direkt nacheinander prüfen. Einziger Reviewer ist das Gate.

BLOCK: vollständige Befunde an D2 und Turn beenden, keine eigene nächste Fixrunde. Exit 2 einmal wiederholen, danach melden und anhalten. Nach beiden ALLOW den beauftragten Ablauf aus BRIEFING.md und DEPLOYCHECKLISTE.md autonom vollenden: Migrationen und Rollen, Bots vor Twitch nach main pushen und deployen, Dienste neu starten, Leaderboard und manuellen Punkte-Sync live belegen, Timer danach, PRs #472 und #1035 schließen, Worktrees und Branches nach SHA-Backup entfernen. Keine Community-Ankündigung.

Status bei Meilensteinen und spätestens alle 20 Minuten, höchstens 5 Zeilen je Meldung. Belege Befehl, Exit, Testzahl, SHA und nötige Belegpfade. Routine selbst entscheiden; Produktfrage, Geld, Datenverlust oder Bereichskonflikt an D2. Secrets nur Infisical/FD, kein Klartext und keine ENV-Konfiguration, Rust only, keine Code-Kommentare, kein Botmodellwechsel. Eigene Texte mit humanizer und no-em-dashes prüfen: natürliches Deutsch, echte Umlaute, keine Gedankenstriche. D2 übernimmt den Threadabschluss.
