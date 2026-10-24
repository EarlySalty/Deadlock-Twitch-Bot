# Review-Briefing: lfg-pitch-register-gate (Runde 1)

[Orchestrator] Review Runde 1 für Paket A. Du bist Reviewer, nicht Autor. Du
änderst keinen Code, du schreibst nur die Mängelliste.

- Auftrag: /home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-09-12-lfg-pitch-register-gate/AUFTRAG.md
- Worktree (nur lesen): /home/nathanael/.worktrees/tb-lfg-pitch-register-gate
- Branch: fix/lfg-pitch-register-gate, Commit d9c82ef2, Merge-Base 0941a1a1
- Diff: `git -C /home/nathanael/.worktrees/tb-lfg-pitch-register-gate diff 0941a1a1..d9c82ef2`
- Ergebnis nach: /home/nathanael/repos/Deadlock-Twitch-Bot/.tasks/2026-09-12-lfg-pitch-register-gate/REVIEW.md

## Was zu prüfen ist

1. Erfüllt der Diff jeden Punkt aus "Arbeitsschritte" und "Fertig-Kriterium"
   des Auftrags? Fehlendes ist ein Mangel.
2. Fail-closed: ohne Register, bei DB-Fehler im Gate, bei leerer Chatter-ID
   bleibt der Pfad still. Broadcaster, Mods, Partner, Streamer, Betreiber und
   Zuschauer mit Discord-Zuordnung bekommen keinen LFG-Pitch, und der Judge
   wird für sie nicht aufgerufen (keine LLM-Kosten).
3. Prod-Verdrahtung in `rust/bin/tb-bot/src/chat_wiring.rs`: bekommt der
   `LfgPitchResponder` dasselbe `ZuschauerRegister` wie die `PromoEngine`?
   Was passiert, wenn `member_relay` fehlt (Register `None`): ist der
   LFG-Pitch dann still, und ist das im Log erkennbar? Prüfe beide
   `LfgPitchResponder::new`-Stellen (Prod und Test).
4. Erkennung: `classify_lfg("Drifter und noch wer")` muss `false` sein,
   echte Suchen wie "noch wer bock auf ranked?" oder "wer zockt noch?" müssen
   `true` bleiben. Prüfe die geänderten Regexe auf Nebenwirkungen (die
   Änderung an `search_lfg_re` mit `noch\s+(jemand|wer)` ist neu).
5. Judge-Kontext: bekommt der Judge die letzten 8 Nachrichten vor der
   Nachricht (nicht danach, nicht die Nachricht selbst doppelt)? Ist die
   Prompt-Regel ("no" bei Antworten auf Fragen, Aufzählungen, Kommentaren)
   drin? Prompt-Injection über den Chatverlauf: ist der Verlauf klar als Daten
   markiert?
6. Zwilling: gibt es weitere Stellen, die diese Einladung oder einen
   Discord-Link an Chatter schicken, ohne das Register zu fragen
   (`InviteQuestionResponder` in `commands.rs`, periodische Einladung,
   `!invite`)? Nur melden, nicht als Mangel dieses Pakets werten, außer der
   Auftrag verlangt es.
7. Tests: neue Tests decken Broadcaster/Mod, Register-Reject ohne
   Judge-Aufruf, Register-Pass, Regex-Fälle und Judge-Input mit Chatverlauf ab?
   Baseline 816/4/5, Endstand 820/4/5 laut Worker: plausibel gegen den Diff?
8. Auftragsregeln: keine Code-Kommentare neu, kein Anfassen fremder Dateien,
   Formatteränderungen nur in den fünf eigenen Dateien. Der promos.rs-Diff hat
   rund 230 Zeilen; sag, wie viel davon reine Formatierung ist und ob das den
   Merge stört.

## Format von REVIEW.md

```
# Review Runde 1: lfg-pitch-register-gate

Urteil: FREIGABE | MÄNGEL

## Mängel (blockierend)
- M1: <datei:zeile> <was> <warum> <erwartet>

## Hinweise (nicht blockierend)
- H1: ...

## Zwillinge (außerhalb des Pakets)
- Z1: ...
```

Jeder Mangel mit Datei und Zeile im Worktree. Keine Code-Kommentare, keine
Em-Dashes, echte Umlaute. Nach dem Schreiben der Datei hier im Thread nur das
Urteil und die Anzahl der Mängel melden, dann stoppen.
