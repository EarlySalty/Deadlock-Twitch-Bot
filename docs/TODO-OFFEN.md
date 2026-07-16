# Offene Punkte (Stand 2026-07-16)

## 1. Bot antwortet Neulingen auf Einsteiger-Fragen und lädt in die Community ein

**Auslöser (Chat-Mitschnitt EarlySalty, 16.07.):** Ein Zuschauer mit `FIRST MESSAGE`-Badge
fragte „Huhu, ich bin noch recht neu bei Deadlock :D. Gibt es eigentlich sowas wie einen
ranked mode, oder ist es alles erstmal casual?" und danach „wo seh ich denn meinen rank?".
Der Streamer hat beides von Hand beantwortet und selbst gepitcht: „Und falls du hilfe beim
einsieg oder mitspieler suchst sag bescheid wir ham ne sehr gute Community".

**Auftrag:** Genau das soll der Bot übernehmen — freundlich im Ton des Streamers.

### Wichtigster Befund vor dem Bauen

`rust/knowledge/deadlock/` enthält **nur** `_infra-platzhalter.md`. Der Bot hat Wissen über
sich selbst (`rust/knowledge/bot/*.md`), aber **null Deadlock-Spielwissen**. Er kann
„gibt es ranked?" heute nicht faktentreu beantworten, und ein LLM frei antworten zu lassen
produziert bei einem Nischenspiel Falschaussagen vor Publikum auf fremden Partnerkanälen.

**Empfohlener Zuschnitt (kein Spielwissen nötig):** Der Bot beantwortet die Frage *nicht*
inhaltlich. Er erkennt „Neuling stellt Einsteiger-Frage" und lädt freundlich in die
Community ein — die Leute dort beantworten es. Das ist genau das, was der Streamer oben
selbst getan hat, und es kann per Konstruktion nicht halluzinieren.

Wenn der Bot doch inhaltlich antworten soll, ist das ein **eigenes Vorprojekt**:
`rust/knowledge/deadlock/` mit Einsteiger-Karten befüllen (ranked/casual, Rang finden,
Zugang, Heldenzahl) und **grounded** ausliefern. Vorbild ist der Concierge, der genau
deshalb `CONCIERGE_FREE_VOICE=false` fährt. Karten müssen nach jedem Patch nachgezogen
werden — Wartungskosten einplanen. Alternative: HTTP-Frage an Deadlock-Brain (hat echten
RAG-Korpus), kostet aber Latenz im Chat-Pfad und eine Laufzeit-Abhängigkeit.

### Bauplan — Muster steht bereits, nicht neu erfinden

Neues Modul `rust/crates/tb-chat/src/newcomer_help.rs`, **kopiert nach dem Muster von**
`invite_question.rs` (dem nächsten Nachbarn — er hat das Newcomer-Gate schon) und
`lfg_pitch.rs`. Beide fahren dieselbe Kette, die eingehalten werden muss:

1. **Billiger Regex-Vorfilter** — Fragezeichen + Einsteiger-Vokabular (neu, anfangen,
   ranked, casual, wie funktioniert, wo sehe ich). Der KI-Call bleibt strikt dahinter.
2. **Newcomer-Gate** — `invite_question.rs` liest dafür `twitch_chatter_rollup`
   (siehe `InviteQuestionRollup`, Zeile ~135). Wiederverwenden, nicht nachbauen.
3. **Cooldowns** — Vorbild `lfg_pitch.rs`: Kanal 2 Min, Nutzer 6 h, Judge 30 s.
4. **MiniMax-Judge** — `EngagementMinimaxClient`, Verdikt `yes|no|unsure` mit Confidence.
   Prompt muss gegen den LFG-Judge abgegrenzt sein: der sagt heute ausdrücklich „no" bei
   Gameplay-Fragen, sonst greifen beide Responder in dieselbe Nachricht.
5. **Antwort** — Wortlaut schreibt der Mensch, nicht Codex (Regel: user-sichtbare Texte).
   Ton: freundlich und locker wie der Streamer, kein Marketing, keine Gedankenstriche.
6. **Verdrahtung** — `pipeline.rs`, Reihenfolge gegen die anderen Responder festlegen.

**Pflicht (CLAUDE.md, Judge-Regel):** Jede Judge-Entscheidung wird geloggt — ja, nein,
unsicher, Timeout, Fehler, jeweils mit gekürzter Eingabe, Verdikt, Confidence und Grund.
Nur-Positiv-Logging ist der blinde Fleck, der am 06.07. einen echten Kampagnen-Account
verschluckt hat. Der Judge darf gaten, was eine **Aktion** auslöst, nie was ein Mensch sieht.

**Vor dem Bauen an echten Daten messen:** Wie oft fällt so eine Frage überhaupt? Query auf
die letzten 30 Tage Chat-Historie. Danach entscheidet sich, ob Cooldowns reichen.

**Tests:** `classify_*`-Funktion rein halten und wie `tests/lfg_pitch.rs` testen (Positiv-
und Negativliste). Der Screenshot-Satz „Gibt es eigentlich sowas wie einen ranked mode"
gehört als Positivfall rein, „suche einen guten build" als Negativfall.

## 2. Bug: `exp_sessions` kann keine Samples schreiben

Im Journal von `deadlock-twitch-bot-rust` laufen dauerhaft Warnungen:

```
WARN tb_monitoring::exp_sessions: exp: Konnte Sample nicht schreiben
error=... there is no unique or exclusion constraint matching the ON CONFLICT specification
```

Die exp-Snapshots wurden am 16.07. im Analytics-Fix (CHANGELOG #380) gerade erst
reaktiviert — das `ON CONFLICT`-Ziel passt aber nicht zum tatsächlichen Index der Tabelle.
Ergebnis: Es werden weiterhin **keine** Samples geschrieben, die Reaktivierung ist damit
faktisch wirkungslos. Nicht neu durch die !commands-Änderung, aber offen.

Erster Schritt: Index/Constraint der Zieltabelle gegen die `ON CONFLICT`-Spalten in
`rust/crates/tb-monitoring/src/exp_sessions.rs` vergleichen.

**Achtung beim Nachprüfen (Stand 16.07., nach dem Restart um 16:44):** Seit dem Neustart
steht **keine** exp-Warnung mehr im Journal — aber auch sonst **keine** exp-Zeile. Der
Sampler hatte schlicht noch keinen Lauf. Das ist kein Beleg dafür, dass der Bug weg ist.
Vor einer Entwarnung muss ein echter Sampler-Lauf im Journal stehen oder ein frischer
Datensatz in der Zieltabelle liegen.

## 3. Raid-Erinnerung: verbleibende Lücken

Gefixt und live seit 16.07. (CHANGELOG #382, Merge `0d88b73f`): Die Erinnerung prüfte
vorher eine Liste fester Grußwörter, wer „gg wp" schrieb galt als abwesend. Jetzt zählt
jede Nachricht des Raiders im Zielchat, Fenster 20 statt 5 Minuten. Offen bleibt:

**a) Der Bot bestraft Kanäle, die er gar nicht hören kann.** Chat empfängt er nur, wo
`channel:bot` erteilt ist (`chat-sub-reconcile` am 16.07.: 51 Kanäle, 49 ok, 2 blocked).
Raidet jemand auf ein Ziel außerhalb dieser Liste — beim Kategorie-Fallback in Deadlock-DE
durchaus normal — sieht der Bot die Antwort **nie** und whispert grundsätzlich, egal was
der Raider tut. Das ist ein falscher Vorwurf per Konstruktion.
Fix-Idee (klein): Beim Ablauf der Frist prüfen, ob aus dem Zielkanal überhaupt Chat
ankommt. Wenn nein, keinen Whisper senden, sondern die Blindheit loggen. Der Monitor sieht
alle Chat-Events selbst, er braucht dafür keine neue Abhängigkeit.

**b) Offene Fristen überleben keinen Neustart.** Die Pending-Liste lebt nur im Prozess
(`raid_greeting.rs`, `pending`-HashMap). Ein Deploy innerhalb der 20 Minuten verwirft sie,
die Erinnerung entfällt. Fehlerrichtung ist bewusst konservativ (lieber keine Erinnerung
als eine falsche), mit 20 statt 5 Minuten trifft es aber häufiger. Persistieren nur, falls
die Erinnerung wirklich Deploys überleben soll.

**c) Der Beweis am echten Raid steht aus.** Seit dem Deploy gab es keinen Raid mehr. Zu
erwarten ist bei der nächsten Begegnung die Journal-Zeile `Raider hat im Zielchat
geschrieben` statt einer Whisper-Zeile.
Nachmessen lohnt: In den 7 Tagen **vor** dem Fix stand im Journal genau **eine** erkannte
Begrüßung gegen rund **zehn** Whisper-Erinnerungen. Dreht sich das Verhältnis nicht um,
ist Ursache (a) der nächste Verdächtige.

## 4. Aufräumen

- Branch `fix/commands-link-only` ist nach `main` gemergt und kann weg (lokal + remote),
  nachdem `git merge-base --is-ancestor fix/commands-link-only main` sauber durchläuft.
- Mehrere Worktrees offen (`git worktree list`): `analytics-fixes`, `botban`, `secfix`.
  Nach Merge jeweils `git worktree remove` + `git worktree prune`.
- Das Deploy-Repo `Documents/Deadlock-Twitch-Bot` steht auf **detached HEAD**, weil der
  Worktree `Deadlock-Twitch-Bot-main` den Branch `main` belegt. Deploys dort laufen per
  `git checkout --detach origin/main`. Wer `main` auschecken will, muss vorher den
  main-Worktree entfernen. Kein Fehler, aber eine Stolperfalle bei jedem Deploy.
