# Selbstvermarktungs-Shadow — Design

Sprachlicher Vertrag: `2026-07-27-selbstvermarktung-stilvertrag.md`. Dieses
Dokument beschreibt nur die Mechanik.

## Ziel

Der Bot beobachtet jeweils **einen** Nicht-Partner-Kanal, der gerade live
Deadlock streamt, hört über OpenAI Whisper mit, was der Streamer sagt, und
erarbeitet daraus **Anknüpfungspunkte**: konkrete Momente, an denen sich ein
Gespräch natürlich anfangen ließe, jeweils mit Beleg und dem Satz, den er
sagen würde.

Es wird nichts nach Twitch gesendet. Der gesamte Output geht in den internen
Discord-Review-Kanal. Prüfbar ist damit ohne jede Interaktion, ob der Bot die
richtigen Momente erkennt und im richtigen Ton anknüpft.

## Nicht-Ziele

- Kein Senden an Twitch, weder automatisch noch manuell freigegeben.
- Kein Eingriff in die bestehenden statischen Recruitment-Texte
  (`tb-raid/src/recruitment_messaging.rs`); die laufen unverändert weiter.
- Kein Speichern von Roh-Audio.
- Keine neue LLM-Provider-Schicht; `crew_review`-Bausteine werden benutzt.
- Keine Bewertung der Person, nur des Gesprächsanlasses.

## Sitzungsauswahl

1. Kandidaten sind Kanäle aus `twitch_partner_outreach`, die weder Partner sind
   noch in `cooldown_until` liegen.
2. Ein Kandidat kommt nur infrage, während er live ist und Deadlock spielt
   (`tb-monitoring`-Streamzustand).
3. **Global höchstens eine aktive Sitzung.** Kommen mehrere Kandidaten
   gleichzeitig infrage, gewinnt der am längsten nicht beobachtete; die
   anderen werden nicht in eine Warteschlange gestellt, sondern beim nächsten
   Durchlauf neu bewertet.
4. Eine Sitzung endet bei Streamende, nach 45 Minuten, bei Prozess-Shutdown
   oder per Kill-Switch.
5. Nach einer Sitzung bekommt der Kanal einen Cooldown, damit nicht derselbe
   Kanal die Beobachtung dauerhaft belegt.

## Audio und Transkription

Unverändert der Pfad aus dem Ricky-Shadow: `yt-dlp` löst den öffentlichen
HLS-Pfad auf, `ffmpeg` schreibt kurze 16-kHz-Mono-Segmente ausschließlich nach
stdout in einen Arbeitsspeicher-Puffer, `POST /v1/audio/transcriptions` mit
`model=whisper-1` und deutscher Sprachvorgabe transkribiert. Segmente werden
nach der Übertragung sofort verworfen; leere oder reine Musiksegmente erzeugen
keinen Datensatz.

## Modell

Provider und Aufruf wie im Ricky-Shadow (Fireworks, OpenAI-kompatible Chat
Completions, `accounts/fireworks/models/deepseek-v4-flash`).

Eingabe: Streamer-Transkripte der Sitzung, Chatverlauf des Kanals, bereits
erzeugte Anknüpfungspunkte derselben Sitzung, Kanalzustand (Partner: nein,
Spiel, Zuschauerzahl, bisherige Raids an diesen Kanal).

Ausgabe ist validierbares JSON:

```
{
  "hooks": [
    {
      "kind": "smalltalk" | "qualify" | "offer",
      "evidence": "wörtliches Zitat aus Transkript oder Chat",
      "evidence_source": "transcript" | "chat",
      "evidence_at": "2026-07-27T20:14:03Z",
      "opener": "der Satz, den der Bot sagen würde",
      "why": "kurze interne Begründung",
      "confidence": 0.0
    }
  ],
  "stage": "watch" | "smalltalk" | "qualify" | "offer",
  "silent_reason": null
}
```

Regeln der Validierung:

- `hooks` darf leer sein; dann ist `silent_reason` Pflicht.
- Jeder Hook braucht ein `evidence`, das wörtlich in einem gespeicherten
  Transkript oder einer gespeicherten Chatnachricht der Sitzung vorkommt.
  Erfundene Belege verwerfen den ganzen Zyklus.
- `kind: "offer"` ist nur zulässig, wenn in derselben Sitzung bereits ein
  `qualify`-Hook mit einer erkennbaren Antwort des Streamers vorliegt. Das
  bildet den belegten Trichter ab (erst reden, dann qualifizieren, dann
  anbieten).
- `opener` unterliegt dem Stilvertrag: keine Emojis außer `:)`, keine
  Mitgliederzahlen, keine Superlative, kein Link. Verstöße verwerfen den Hook.
- Ein Link darf nie in einem `opener` stehen — im belegten Ablauf folgt er
  erst nach ausdrücklicher Zustimmung, die es im Schattenbetrieb nicht gibt.
- Parserfehler, fehlende Pflichtfelder oder ein unzulässiger Text führen zu
  `provider_error` und niemals zu einem Hook.

## Discord-Review

Zielkanal wie beim Ricky-Shadow, Components-V2 mit Gold-Akzent `0xC8A86B`.

Jede Karte zeigt: Kanal, Sitzungs-ID, Laufzeit, Stufe, und pro Hook den Beleg
mit Zeitstempel und Quelle, den Einstiegssatz, die Begründung und die
Confidence. Zusätzlich pro Zyklus, was der heutige statische Recruitment-Pfad
in derselben Lage gesendet hätte — nebeneinander, damit der Unterschied
sichtbar ist.

`allowed_mentions.parse` bleibt leer.

## Vollständige Sichtbarkeit

Jeder Zyklus erzeugt einen Eintrag, auch wenn nichts gefunden wurde. Karten
zeigen ebenso `silent` mit Grund, Parserfehler, Timeouts und Provider-Fehler.
Nur Treffer zu melden wäre ein blinder Fleck: Stille sähe dann wie „kein
Anlass" aus, wäre aber „Modell hat versagt".

Jede Logzeile enthält Sitzungs-ID, Kanal, Stufe, Anzahl Hooks und Fehlerklasse
— nie die Inhaltsfelder.

## Fehlerverhalten

- Audioquelle nicht verfügbar: Sitzung wird als `stream_unavailable`
  geschlossen, kein veralteter Kontext bleibt stehen.
- Whisper-Fehler: Fehlerereignis, kein Modellaufruf für dieses Segment.
- Fireworks-Fehler oder ungültiges JSON: Fehlerereignis, kein Hook.
- DB-Fehler: kein Discord-Post für den Zyklus, der Bot läuft weiter.
- Discord-Fehler: DB bleibt Source of Truth, begrenzter Retry.

## Konfiguration

- `OUTREACH_SHADOW_ENABLED` (Default aus) — Kill-Switch.
- `OPENAI_API_KEY`, `FIREWORKS_API_KEY` aus Infisical, wie gehabt.
- Segmentlänge und Zykluslänge als Konstanten, keine neue Config-Fläche.

## Testvertrag

- Sitzungsauswahl: nie mehr als eine aktive Sitzung, Partner werden nie
  ausgewählt, Cooldown wird eingehalten.
- Validierung: erfundener Beleg wird verworfen; `offer` ohne vorheriges
  `qualify` wird verworfen; Emoji, Mitgliederzahl, Superlativ und Link im
  `opener` werden verworfen.
- Leeres `hooks` ohne `silent_reason` ist ein Fehler.
- Jeder Ausgang — Hook, silent, Parserfehler, Timeout — erzeugt genau ein
  persistiertes Ereignis.
- Kein Codepfad ruft einen Twitch-Send auf.

## Betriebsnachweis

Nach Deploy: PID-Wechsel, `/proc/<pid>/exe` zeigt auf die neue Binary,
Journal ohne `error|panic|fatal`, und mindestens eine Review-Karte im
Discord-Kanal aus einer echten Sitzung.
