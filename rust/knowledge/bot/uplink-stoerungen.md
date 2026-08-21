---
title: Uplink: Häufige Störungen
namespace: bot
category: faq
audience: streamer
last_updated: 2026-08-21
source: rs-relay/docs/stoerungen.html
tip_eligible: false
---
Am Anfang zählt die sichtbare Lage, danach der nächste Schritt. Bei einem kurzen Netzabriss gibt es ein kurzes Wiederverbindungsfenster. Ist die Verbindung länger weg, ist der Stream beendet.

### Der Start wird abgelehnt, weil Plätze belegt sind

Es ist gerade voll. Laufende Streams bleiben bestehen. Für diesen Slot streamst du direkt zur Plattform. Stelle OBS dafür wieder auf Twitch oder Kick.

### OBS sendet, aber auf der Plattform kommt nichts an

1. Prüfe im Dashboard, ob Uplink freigeschaltet ist, das Ziel verbunden ist und der Schlüssel stimmt.
2. Der OBS-Dienst muss **Benutzerdefiniert** sein. Als Server muss die SRT-Adresse aus dem Dashboard eingetragen sein.
3. Bei einem falschen oder alten Schlüssel holst du im Dashboard einen neuen und setzt ihn in OBS ein.

### Das Bild reißt in Fights

Deine Leitung oder der Encoder kommt nicht hinterher.

- Senke bei VBR das Maximum um 1000 Kbps.
- Oder nutze 30 fps statt 60.
- Ist **Skipped frames** in OBS größer als 0, setze die Auflösung eine Stufe herunter und lass den Encoder auf Hardware-HEVC.

### HEVC steht nicht in der Liste

Nutze Hardware-H.264. Der Stream läuft dann weiter, braucht aber mehr Upload. Lass Software-x265 und AV1 neben dem Spiel weg.

### Streamlabs kann die SRT-Adresse nicht eintragen

Viele Streamlabs-Versionen können kein SRT. Nur über SRT kommt dein Stream bei uns an. Wenn du die SRT-Adresse aus dem Dashboard dort nicht eintragen kannst, nimm OBS. Auch HEVC ist aus Streamlabs oft nicht nutzbar. Dann ist der Bandbreitenvorteil weg.

### Das Internet bricht weg

Bei einem kurzen Ausfall halten wir die Plattform-Seite offen und setzen fort, sobald OBS wieder da ist. Bei einem längeren Ausfall gilt der Stream als beendet, wie bei einem normalen Plattform-Abriss. Starte ihn in OBS neu.
