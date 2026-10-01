status: aktiv
Datum: 2026-10-01

# Auftrag: Lurker-Discord-Pitch wiederaufnehmen

Bearbeite ausschließlich den Auftrag aus `CONTRACT.md` auf dem isolierten Branch `codex/luna-dispatch/deadlock-twitch-bot/feat-lurker-discord-pitch-6ff725ee`, gestartet bei `6ff725ee6808eed2235429ed4cadddfa8f0deead`.

Der Source-Commit ist ein WIP-Sicherungscommit und nicht semantisch in `origin/main` oder dem offenen PR 1035 enthalten. Übernimm keine fremden Änderungen. Der Originalauftrag wurde aus dem passenden Contract-Artefakt in Commit `0b03a9701b86ed103f8fe163bbc1e4b3b1795165` rekonstruiert. Der Branchbeitrag umfasst die own-channel-Sperre für LFG-Pitches und den optionalen Lurker-Discord-Pitch im bestehenden Lurker-Tax-Pfad. Das Betreiber-Flag bleibt standardmäßig aus.

Validiere Vertrag, Eintrittspfade, Persistenz und Fehlverhalten statisch. Vor Tests, Build oder lokalem Merge-Gate gilt der Host-Ressourcen-Hold. Kein Main-Merge, Produktions-DDL, Deploy, Restart oder Live-Cutover bis zur jeweiligen Ownerfreigabe. Für die Abschlussintegration ist PR 1035 wegen gemeinsamer Dateien mit dem Gruppenintegrator abzustimmen; keine Einzelintegration.
