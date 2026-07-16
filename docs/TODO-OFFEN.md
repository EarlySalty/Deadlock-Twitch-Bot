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

## 3. Aufräumen

- Branch `fix/commands-link-only` ist nach `main` gemergt und kann weg (lokal + remote),
  nachdem `git merge-base --is-ancestor fix/commands-link-only main` sauber durchläuft.
- Mehrere Worktrees offen (`git worktree list`): `analytics-fixes`, `botban`, `secfix`.
  Nach Merge jeweils `git worktree remove` + `git worktree prune`.
