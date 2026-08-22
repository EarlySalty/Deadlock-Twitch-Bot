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

Es ist gerade voll. Laufende Streams bleiben bestehen. Für diesen Abend streamst du direkt zur Plattform. Stelle OBS dafür wieder auf Twitch oder Kick.

### OBS sendet, aber auf der Plattform kommt nichts an

1. Prüfe im Dashboard, ob Uplink für dich freigeschaltet ist. Steht dort die SRT-Adresse, ist der Zugang da.
2. Der OBS-Dienst muss **Benutzerdefiniert** sein. Als Server muss die SRT-Adresse aus dem Dashboard eingetragen sein.
3. Steht in OBS noch eine alte Adresse, kopiere die SRT-Adresse im Dashboard neu und ersetze sie. Der Schlüssel steckt als `streamid` in der Adresse, es gibt keinen zweiten Wert zum Abtippen.

### Das Bild reißt in Fights

Deine Leitung oder der Encoder kommt nicht hinterher.

- Senke bei VBR das Maximum um 1000 Kbps.
- Oder nutze 30 fps statt 60.
- Ist **Skipped frames** in OBS größer als 0, setze die Auflösung eine Stufe herunter und lass den Encoder auf Hardware-HEVC.

### HEVC steht nicht in der Liste

Nutze Hardware-H.264. Der Stream läuft dann weiter, braucht aber mehr Upload. Lass Software-x265 und AV1 neben dem Spiel weg.

### Streamlabs kann die SRT-Adresse nicht eintragen

Viele Streamlabs-Versionen können kein SRT. Von deinem PC aus kommt dein Stream nur über SRT bei uns an. Einen RTMP-Eingang gibt es zusätzlich, doch der liegt auf dem Server selbst und ist nur von dort erreichbar. Wenn du die SRT-Adresse aus dem Dashboard dort nicht eintragen kannst, nimm OBS. Auch HEVC ist aus Streamlabs oft nicht nutzbar. Dann ist der Bandbreitenvorteil weg.

### Das Internet bricht weg

Bei einem kurzen Ausfall halten wir die Plattform-Seite offen und setzen fort, sobald OBS wieder da ist. Bei einem längeren Ausfall gilt der Stream als beendet, wie bei einem normalen Plattform-Abriss. Starte ihn in OBS neu.
