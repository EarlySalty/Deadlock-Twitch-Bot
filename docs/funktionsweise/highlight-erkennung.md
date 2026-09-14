# Highlight-Erkennung aus VODs

Aus einem lokalen Twitch-VOD entsteht eine bewertete Liste von Highlight-Kandidaten
(Zeitfenster, Score, Begruendung je Signal) und daraus geschnittene mp4. Der Detektor
lernt vorher aus echten Zuschauer-Clips, welche Signale gute Clips ausmachen.

Der Prototyp liegt in `ops/highlight-detector/` (eigenes venv nach dem Muster von
`ops/stt-server`). Alle Schwellen und Gewichte stehen erklaerbar in den Konfigdateien
unter `config/`, nichts ist im Code hart verdrahtet.

## Einrichtung

```
cd ops/highlight-detector
bash setup.sh
```

Voraussetzungen auf dem Host: tesseract 5.3.4 (deu, eng), ffmpeg 6.1, yt-dlp, der
lokale STT-Server auf 127.0.0.1:8791.

## Signale

Drei Quellen, jede einzeln abschaltbar (`config/gewichte.toml`, Abschnitt
`[signale_aktiv]`):

- **OCR** (`config/regionen.toml`): Frame-Abtastung (Standard 2 Bilder je Sekunde).
  Je Region ein Ausschnitt aus dem 1080p-Bild, in Bruchteilen der Bildbreite/-hoehe,
  damit die Regionen mit der Aufloesung skalieren.
  - `killfeed` (oben links): der Kill-Feed. Die linke Haelfte ist der Killer, die
    rechte das Opfer. Steht der Spielername (Fuzzy-Abgleich gegen `spieler_namen`)
    links, ist es ein eigener Kill, rechts ein eigener Tod.
  - `souls` (unten links): der Soul-Zaehler. Ein Sprung ueber `soul_sprung_min`
    zwischen zwei Messungen ist ein Kill-/Objective-Hinweis.
  - `banner` (Bildmitte): Multikill- und Objective-Einblendungen ueber Wortlisten.
- **Sprache**: Lautstaerke-Huellkurve je Sekunde (ffmpeg), Spitzen gegen das
  gleitende 60-Sekunden-Mittel; ausserdem der STT-Server fuer Lachen und
  Schluesselwoerter (`config/gewichte.toml`, `[schluesselwoerter]`). Das Transkript
  wird je VOD gecacht und kann von der Untertitel-Aufbereitung wiederverwendet werden.
- **Bild**: Szenenwechsel-Dichte (`ffmpeg select=gt(scene,...)`) und Death-Screen
  (Einbruch der Farbsaettigung bei eigenem Tod).

## Score

Der Detektor schiebt ein Fenster (`fenster_s`, Standard 8 s) in Schritten
(`schritt_s`) ueber die Signal-Zeitreihe. Je Fenster wird pro Signaltyp der hoechste
Wert genommen, mit dem Gewicht aus `config/gewichte.toml` multipliziert und aufsummiert.
Fenster ueber der `schwelle` sind Kandidaten; ueberlappende Kandidaten werden per
Nicht-Maximum-Unterdrueckung (`nms_abstand_s`) ausgeduennt. Der Score jedes Kandidaten
traegt seine Einzelbeitraege, damit die Begruendung nachvollziehbar bleibt.

## Lern-Korpus (Gewichte kommen aus echten Clips, nicht geraten)

```
highlight-detector lernen --dsn "<DSN>" --limit 800
```

Laedt in Masse existierende Twitch-Clips (Partner-Clips aus `twitch_clips_social_media`,
optional Deadlock-Top-Clips ueber Helix), zieht je Clip nur das Zeitfenster per yt-dlp,
extrahiert dieselben Signale und schreibt je Signal-Treffer eine Zeile relativ zum
Clip-Ende nach `twitch_clip_merkmale`. Die Clip-Datei wird nach der Extraktion
geloescht. Aus der Verteilung leitet der Lauf die Gewichte (Anteil der Clips mit dem
Signal im Endfenster, auf das staerkste Signal normiert) und die Score-Schwelle
(25-Perzentil der Clip-Spitzenscores) ab und schreibt sie nach `config/gewichte.toml`.
Der Report `.tasks/2026-09-06-highlight-erkennung-vod/korpus-report.md` erklaert, warum
gute Clips gut sind (Verteilungen, Top-Muster, echte Beispiel-Clips).

Der Kill-Feed-Treffer haengt am Spielernamen und ist damit streamer-spezifisch; die
generischen Signale (Soul-Sprung, Lautstaerke, Szenenwechsel, Death-Screen, Lachen)
feuern ueber alle Clips.

## Analyse und Schnitt

```
highlight-detector analyse <vod.mp4> [--json] [--schneiden] [--worker 8]
```

Extrahiert die Signale (gecacht unter `~/.cache/highlight-detector/<vod_id>/`, ein
Wiederholungslauf mit anderen Gewichten braucht keine neue OCR), bildet die Kandidaten
und gibt sie als Tabelle oder JSON aus. Mit `--schneiden` wird jeder Kandidat ueber der
Schwelle per ffmpeg-Reencode (Vorlauf 20 s, Nachlauf 10 s) nach
`/home/nathanael/vod-archive/highlights/<vod_id>/` geschnitten; der Dateiname traegt
Zeitfenster und Score. Die OCR laeuft ueber bis zu acht Prozesse mit
`nice`; ein 4-Stunden-VOD bleibt damit im Rahmen.

## Messung gegen echte Clips

```
highlight-detector messen --dsn "<DSN>" --streamer earlysalty
```

Die echten Clips des Streamers werden im VOD verortet und mit den Kandidaten
verglichen. Ein Clip gilt als getroffen, wenn ein Kandidat sein Fenster um hoechstens
15 s versetzt ueberlappt; Kandidaten ohne echten Clip werden als Falsch-Positive offen
ausgewiesen. Der Streamer wird ueber die Twitch-User-ID aufgeloest, nie ueber den Login
gefiltert.

Verortung: produktiv ueber `vod_id`/`vod_offset_s` (Helix, neue Spalten in
`twitch_clips_social_media`). Ohne diesen Backfill verortet die Messung den Clip
hilfsweise ueber seine Twitch-Erstellzeit gegen das Zeitfenster aus der VOD-`.info.json`.

## Persistenz

Erkannte Kandidaten gehen nach `twitch_vod_highlights` (Streamer-ID, VOD, Fenster,
Score, Signale als JSONB, Status). Merkmale des Lern-Korpus stehen in
`twitch_clip_merkmale`. Beide Tabellen kommen aus den Migrationen unter
`rust/migrations/` und muessen einmalig als `postgres` angewandt werden.
