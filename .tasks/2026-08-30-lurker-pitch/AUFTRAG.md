status: aktiv
Datum: 2026-10-01

# Auftrag: Lurker-Discord-Pitch wiederaufnehmen

Bearbeite ausschließlich den Auftrag aus `CONTRACT.md` auf dem isolierten Branch `codex/luna-dispatch/deadlock-twitch-bot/feat-lurker-discord-pitch-6ff725ee`, gestartet bei `6ff725ee6808eed2235429ed4cadddfa8f0deead`.

Der Source-Commit `6ff725ee6808eed2235429ed4cadddfa8f0deead` ist im Archiv-Tag `archiv/2026-10-01/lurker-discord-pitch-pre-main-sync-b5185e93` erhalten. `origin/main` stand beim Abgleich auf `14bc1f479e32394fe2977f8c8ef85e6c5bff66e1` und enthielt den Pitch nicht. Der Arbeitsbranch wurde auf diesen Main-Stand rebased; die aktuelle Broadcaster-ID-, Scope-, Plan- und Reward-Gates bleiben erhalten. Das Pitch-Flag bleibt standardmäßig aus.

Validiere Vertrag, Eintrittspfade, Persistenz und Fehlverhalten statisch. Die pauschalen Host-Ressourcen- und TokenDB-Live-Holds aus der früheren Fassung sind am 2026-10-01 aufgehoben. Schwere Cargo-Läufe nur mit `/tmp/deadlock-cargo-release.lock`; keine parallelen Läufe zum aktiven Hostslot. Keine Secrets/ENV-Dateien lesen. Produktions-DDL, Deploy, Restart und Live-Cutover sind nicht Teil dieser Fortsetzung.
