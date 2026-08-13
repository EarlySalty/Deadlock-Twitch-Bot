# Reaktions-Lernmodus

> Stand: 2026-08-13 · Code: `rust/crates/tb-engagement/src/reaction_learning.rs`,
> `learn_irc_reader.rs`, Loops in `background.rs` · Teil der
> [Engagement-Architektur](engagement.md)

## 1. Wozu

Die Engagement-KI bezog ihren Grundton bisher aus einem handgepflegten
Gold-Register in `style_examples.rs`: vierzehn Zeilen, die als typisch für den
Owner galten. Der Lernmodus ersetzt diese Annahme durch Beobachtung. Er zeichnet
auf, **worauf** der Owner im fremden Twitch-Chat reagiert und **wie**, und
speist beides in den Prompt zurück.

Der Unterschied ist nicht kosmetisch. Stil-Zeilen zeigen nur die Antwort. Was
gefehlt hat, ist der Auslöser: welcher Moment im Stream jemanden überhaupt zum
Tippen bringt, und welche zwanzig Momente davor es nicht getan haben.

## 2. Der Weg einer Nachricht

1. **Mitlesen.** Zwei Quellen speisen `ReactionLearning::observe`:
   - der EventSub-Chat-Pfad (`chat_wiring.rs`) für Partner-Kanäle, angehängt
     **vor** dem Partner-Gate;
   - `LearnIrcReader`, eine zweite anonyme IRC-Verbindung (`justinfan`), die
     alle live Deadlock-Kanäle aus `twitch_live_state` mitliest.

   Die zweite Quelle ist nötig, weil EventSub nur Kanäle mit erteiltem
   `channel:bot` liefert, gelernt aber gerade in fremden Kanälen wird. Sie hat
   keinen Sende-Transport und keine Verbindung zur Antwort-Pipeline.

2. **Aufnehmen, mit klarer Rangfolge.** Transkription kostet dauerhaft CPU,
   also läuft sie nicht überall, sondern dort, wo sie etwas bringt:
   - **Wo der Owner ist, hat Vorrang.** Sobald er irgendwo schreibt, wird
     genau dieser Kanal aufgezeichnet (bis zu `MAX_CHANNELS`, falls er in
     mehreren unterwegs ist). Alles andere pausiert.
   - **Ist er nirgends, läuft genau ein Kanal mit**: der mit dem lebendigsten
     Chat unter den live Deadlock-Partnern. Das ist Anschauungsmaterial dafür,
     wie ein gut laufender Chat funktioniert, und Vorbereitung auf künftige
     Streamer. Alle Partner rund um die Uhr zu transkribieren würde die
     Maschine auslasten, ohne dass dabei eine einzige eigene Reaktion entsteht.
     `ENGAGEMENT_LEARN_IDLE_CHANNELS=0` schaltet auch das ab.

   „Lebendig" misst der Lern-IRC-Reader selbst: Chat-Zeilen je Kanal in den
   letzten 10 Minuten, im Speicher, ohne Datenbank. Kanäle ganz ohne Aktivität
   werden nie aufgenommen.

   Die Aufnahme endet, sobald der Stream offline geht (`twitch_live_state`),
   sonst 45 Minuten nach der letzten eigenen Nachricht. In allen aufgenommenen
   Kanälen läuft auch der Chat mit; sonst gäbe es Stream-Ton ohne den Chat, der
   dazu lief. Alle übrigen Nachrichten kosten nur einen Map-Zugriff und werden
   verworfen.

   **In fremden Kanälen** startet die Aufnahme erst mit der ersten eigenen
   Nachricht. Früher geht es dort nicht: dass der Owner zuschaut, erfährt man
   ausschließlich dadurch, dass er schreibt.

3. **Ein Task pro Kanal**, nicht eine gemeinsame Runde: `streamlink` zieht
   Blöcke von 30 s, Whisper transkribiert. Reihum wären die Lücken dazwischen
   genau die Momente, auf die reagiert wird.

4. **Koppeln.** Der Mapper verbindet jede eigene Nachricht mit dem
   Transkript-Fenster davor (-45 s bis +10 s) und den letzten acht Chat-Zeilen.
   Ergebnis ist eine Zeile in `twitch_engagement_reaction_samples`.

5. **Zurückspeisen.** Zwei Wege:
   - **Stil**: Ab vier brauchbaren gelernten Zeilen verdrängen sie das feste
     Gold-Register im Few-Shot-Block.
   - **Profil**: Alle 6 h destilliert ein Modell-Call aus bis zu 60 Samples ein
     Reaktionsprofil (worauf, worauf nicht, wie), abgelegt als Soul-Eintrag
     `reaction_profile` und an den Prompt gehängt.

## 3. Zeitversatz

Nachricht und Transkript werden über die Wall-Clock verglichen, nicht über eine
Stream-Position. Beide Seiten hängen der Realität hinterher: der Zuschauer sieht
den HLS-Stream mit rund 10-20 s Verzögerung, `streamlink` zieht ihn mit
ähnlicher Latenz. Die Versätze heben sich weitgehend auf, und das Fenster ist
mit 55 s Breite grosszügig genug, um den Rest aufzufangen. Sekundengenauigkeit
ist hier auch nicht das Ziel, der Auslöser reicht.

Damit das trägt, rechnet `run_learn_capture` die Segmentzeit aus dem
Capture-**Start**, nicht wie der operative Transkript-Loop aus dem Zeitpunkt
nach der Transkription. Sonst verschöbe die Whisper-Laufzeit jedes Segment um
ihre eigene Dauer nach hinten.

## 4. Tabellen

| Tabelle | Inhalt | Aufbewahrung |
|---|---|---|
| `twitch_engagement_learn_channels` | Wo der Owner zuletzt geschrieben hat | dauerhaft |
| `twitch_engagement_learn_timeline` | Der gebündelte Zeitstrahl: Stream-Ton, fremder Chat und eigene Zeilen | 7 Tage |
| `twitch_engagement_reaction_samples` | Das fertige Stimulus/Response-Paar | dauerhaft |

**Ein Zeitstrahl, nicht drei Töpfe.** Getrennte Tabellen für Audio und Chat
hätten bedeutet, dass die beiden Seiten erst im fertigen Sample zusammenfinden,
also nur in den Sekunden rund um eine eigene Nachricht. Der Rest der Sitzung
wäre in zwei Hälften zerfallen, die niemand mehr am Stück lesen kann. In
`twitch_engagement_learn_timeline` unterscheidet nur die Spalte `kind`
(`stream` / `chat` / `own`), was woher kam.

`ts` ist immer der maßgebliche Zeitpunkt: bei Chat die Sendezeit, bei Audio das
**Ende** des Segments, weil dann der Satz zu Ende gesprochen war. Ein einziges
`ORDER BY ts` sortiert damit die ganze Sitzung richtig. `started_at` hält
zusätzlich den Segment-Beginn.

Eigene Zeilen stehen nur einmal drin (`kind = 'own'`). Sie sind Response und
zugleich Umgebung des nächsten Turns; der Chat-Kontext liest deshalb
`kind IN ('chat','own')`. Beim Trimmen bleiben ungemappte eigene Zeilen
unabhängig vom Alter erhalten, sonst würde ein längerer Mapper-Ausfall
Reaktionen wegwerfen, bevor sie ausgewertet wurden.

Getrennt von `twitch_engagement_stream_transcripts`: die operative Tabelle ist
absichtlich flüchtig (60 min, 40 Segmente je Kanal) und läuft nur für
engagement-aktive Partner. Der Lernmodus braucht das Gegenteil.

## 5. Schalter

| Variable | Default | Wirkung |
|---|---|---|
| `ENGAGEMENT_LEARN_ENABLED` | aus | Schaltet Erfassung, Aufnahme, Mapping und Profil frei |
| `ENGAGEMENT_LEARN_LOGIN` | `earlysalty` | Wessen Reaktionen gelernt werden |
| `ENGAGEMENT_STT_BASE_URL` | `http://127.0.0.1:8791/v1/audio/transcriptions` | Whisper-Endpunkt (siehe unten) |
| `ENGAGEMENT_LEARN_HOT_MINUTES` | 45 | Nachlauf in fremden Kanälen |
| `ENGAGEMENT_LEARN_CAPTURE_SECONDS` | 30 | Länge eines Aufnahmeblocks |
| `ENGAGEMENT_LEARN_MAX_CHANNELS` | 3 | Kanäle mit Owner-Anwesenheit gleichzeitig |
| `ENGAGEMENT_LEARN_IDLE_CHANNELS` | 1 | Mitlaufende Kanäle, wenn der Owner nirgends ist (`0` = aus) |
| `ENGAGEMENT_LEARN_WINDOW_PRE_SECONDS` | 45 | Transkript-Fenster vor der Nachricht |
| `ENGAGEMENT_LEARN_WINDOW_POST_SECONDS` | 10 | Transkript-Fenster danach |
| `ENGAGEMENT_LEARN_RETENTION_HOURS` | 168 | Aufbewahrung des Zeitstrahls |
| `ENGAGEMENT_PERSONA_MODE` | `veteran` | `rookie` schaltet auf die Neuling-Persona |

**Transkription läuft lokal, und zwar per Default.** `ops/stt-server/stt_server.py`
hält ein `large-v3-turbo`-Modell im Speicher und spricht dieselbe Schnittstelle
wie OpenAI; der eingebaute Endpunkt zeigt auf ihn. Es geht also kein
Stream-Audio an einen Fremdanbieter, auch nicht versehentlich, nur weil ein
`OPENAI_API_KEY` in der Umgebung steht. Wer OpenAI trotzdem will, setzt
`ENGAGEMENT_STT_BASE_URL` explizit auf deren Endpunkt.

Der Dienst hat keine Authentifizierung und bindet deshalb ausschließlich ans
Loopback. Er läuft als User-Unit `deadlock-stt-server.service`
(venv `~/stt-tools`, Port 8791, 8 Threads); Gesundheitscheck:
`curl -s http://127.0.0.1:8791/health`. Läuft er nicht, schlägt die
Transkription fehl und der Zeitstrahl bekommt für diese Minuten keine
Stream-Zeilen — er weicht bewusst nicht nach außen aus.

## 6. Sichtung

`ops/learn-samples.sh` zeigt die Samples und nimmt das Urteil auf:

```
DATABASE_URL=... ops/learn-samples.sh stats           # Fortschritt
DATABASE_URL=... ops/learn-samples.sh list 20         # letzte Samples
DATABASE_URL=... ops/learn-samples.sh show 42         # Audio + Chat + Reaktion
DATABASE_URL=... ops/learn-samples.sh bad 42 43       # aus dem Lernmaterial nehmen
DATABASE_URL=... ops/learn-samples.sh timeline nani 30  # Verlauf am Stück
DATABASE_URL=... ops/learn-samples.sh profile         # destilliertes Profil
```

`timeline` liest den gebündelten Zeitstrahl, eigene Zeilen mit `>>` markiert:

```
17:51:58 [STREAM] okay ich geh jetzt einfach mal rein da
17:52:08    chatterA: nicht machen
17:52:28 [STREAM] boah nee das war komplett dumm von mir
17:52:33 >> earlysalty: wilder take
17:52:38    chatterB: lol
```

Als `bad` markierte Samples fallen aus Few-Shot und Destillation heraus. Das ist
der vorgesehene Weg, den Grundgeschmack zu korrigieren, bevor die KI live geht.

## 7. Persona-Modi

`PersonaMode::Veteran` (Default) ist der bisherige Charakter: Daily-Spieler mit
Meinung zur Meta. `PersonaMode::Rookie` ist der Gegenentwurf: neu im Spiel,
fragt statt zu erklären, hat keine Meta-Meinung.

Der Neuling löst nebenbei das teuerste Problem des Veteranen. Der darf bei einer
Wissenslücke keinen Disclaimer aussprechen, weil ihn genau das als Bot enttarnt,
und muss sich stattdessen herauswinden. Ein Neuling sagt einfach „keine ahnung
was das macht" — im Chat die normalste Zeile der Welt. Entsprechend hebt der
Rookie-Modus dieses Verbot gezielt auf, behält aber das Erfindungsverbot und das
Schweigen über Quellen bei. Zusätzlich fallen die kuratierten Hero-Takes aus dem
Soul-Fragment, und der Bot gibt keine Tipps und korrigiert niemanden.

## 8. Grenzen

- **In fremden Kanälen fehlt der erste Moment.** Dort startet die Aufnahme
  erst mit der ersten eigenen Nachricht; für genau diese gibt es kein Audio,
  sie wird als Sample ohne Stream-Kontext abgelegt (`has_stream_context = false`).
  In Partner-Kanälen tritt das nicht auf, dort läuft die Aufnahme ab
  Stream-Beginn.
- **Nur deutsche Deadlock-Streams.** Die Kanalquelle des Lern-Readers hängt am
  Scout, der auf Kategorie Deadlock und Sprache `de` filtert. In einem Kanal
  außerhalb dieser Menge wird nur mitgelesen, wenn er ohnehin Partner ist.
- **Fremde Chat-Zeilen werden gespeichert**, aber nur in lern-heißen Kanälen und
  nur 48 Stunden lang. Der Reader liest zwar überall mit, verwirft aber alles
  Übrige, ohne es je in die Datenbank zu schreiben.
