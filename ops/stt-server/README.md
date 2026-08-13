# Lokale Transkription (Ersatz für die OpenAI-Whisper-API)

`stt_server.py` hält ein Whisper-Modell im Speicher und spricht
`POST /v1/audio/transcriptions` in genau der Form, die
`rust/crates/tb-engagement/src/transcribe.rs` schon erwartet. Der Rust-Pfad
ändert sich dadurch nur an der Basis-URL — Multipart-Aufbau, `verbose_json`,
`text` und `duration` bleiben gleich.

## Betrieb

```
STT_PORT=8791 .../stt-tools/bin/python ops/stt-server/stt_server.py
curl -s http://127.0.0.1:8791/health
```

Verdrahtet wird der Dienst über eine Env-Variable:

```
ENGAGEMENT_STT_BASE_URL=http://127.0.0.1:8791/v1/audio/transcriptions
```

Ist sie gesetzt, braucht `OpenAiTranscriber::from_env()` keinen
`OPENAI_API_KEY` mehr und schickt einen Platzhalter, den dieser Dienst ohnehin
ignoriert. Ohne die Variable geht jede Transkription an OpenAI und kostet Geld —
für den Reaktions-Lernmodus, der stundenlang durchgehend aufnimmt, ist das der
teure Weg.

Der Dienst hat **keine Authentifizierung** und gehört deshalb ausschliesslich
an `127.0.0.1`. Der Rust-Aufrufer schickt weiterhin einen `Authorization`-Header,
der hier ignoriert wird.

## Modellwahl — gemessen, nicht geraten

Gemessen auf diesem Host (EPYC 9334, 16 Kerne zugeteilt, keine GPU, AVX-512),
`int8`, deutsches Sprachmaterial:

| Modell | Fenster | Median | RTF |
|---|---|---|---|
| tiny | 20 s | 1,34 s | 0,066 |
| small | 20 s | 6,06 s | 0,318 |
| **large-v3-turbo** | **20 s** | **3,97 s** | **0,200** |
| large-v3-turbo | 5 s | 3,05 s | 0,609 |

`large-v3-turbo` ist bei 20-Sekunden-Fenstern **schneller als `small`** und
dabei deutlich genauer — turbo hat nur vier Decoder-Schichten. Bessere Qualität
kostet hier also nichts. `tiny` scheitert schon an Eigennamen und ist für
Deadlock-Vokabular unbrauchbar.

### Threads: 8, nicht 16

| Threads | 20 s | 10 s | 5 s |
|---|---|---|---|
| 16 | 3,87 s | 3,18 s | 2,91 s |
| **8** | **3,10 s** | **2,84 s** | **2,72 s** |
| 4 | 4,35 s | — | — |

Mehr Threads machen es langsamer — der Synchronisationsaufwand überwiegt. Acht
Threads sind zugleich das, was neben Bot und Postgres vertretbar ist.

### Warum kurze Fenster teuer sind

Whisper füttert seinen Encoder **immer** mit einem 30-Sekunden-Fenster, egal wie
kurz das Audio ist. Ein Aufruf kostet dadurch rund 2,8 s Grundgebühr, fast
unabhängig von der Fensterlänge (5 s → 2,72 s, 20 s → 3,10 s). Für den Durchsatz
gilt deshalb:

```
RTF = Kosten pro Aufruf ÷ Schrittweite
```

Häufigere Aufrufe kosten linear mehr. Eine Auswertung im Sekundentakt läge bei
RTF ~2,9 und ist auf dieser Maschine nicht machbar.

## Auf echtem Twitch-Audio verifiziert

Kette `streamlink --twitch-low-latency` → `ffmpeg` → turbo, gemessen an einem
laufenden Stream: **3,37 s für 20 s Audio (RTF 0,17)**, deckt sich mit dem
Benchmark. Über HTTP gegen diesen Dienst: 3,29 s — kein messbarer Aufschlag
durch Multipart.

## Negativbefunde

- **Pauschales Normalisieren schadet.** Bei einem sehr leisen Kanal (RMS 0,008)
  hob eine 12-fache Verstärkung den Rauschteppich mit an; das Modell begann zu
  halluzinieren („Ölbac", „Dö-dö-döbä") statt besser zu erkennen.
- **Stille Fenster erzeugen keinen Datensatz.** Der VAD verwirft reine
  Spielsound- oder Musikpassagen, bevor das Modell startet. Das ist gewollt und
  senkt den realen RTF unter die hier gemessenen Werte, die durchgehende Rede
  unterstellen.
