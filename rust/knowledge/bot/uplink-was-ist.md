---
title: Uplink: Was ist das?
namespace: bot
category: feature
audience: streamer
last_updated: 2026-08-21
source: rs-relay/docs/was-ist.html
tip_eligible: false
---
Uplink nimmt deinen OBS-Stream entgegen und schickt ihn in der passenden Qualität an die verbundenen Plattformen. Du startest und stoppst wie gewohnt in OBS.

Uplink ist für Streamer gedacht, die ihren OBS-Stream an mehrere verbundene Plattformen senden möchten.

### Wie läuft Uplink ab?

1. Lass den Dienst im Dashboard freischalten und kopiere die SRT-Adresse.
2. Stelle OBS auf unsere SRT-Adresse, nicht mehr direkt auf Twitch.
3. Starte den Stream in OBS. Die verbundenen Plattformen werden angelegt.
4. Beende den Stream in OBS. Die verbundenen Plattformen werden ebenfalls beendet.

### Was stellst du ein, was passiert bei uns?

Du stellst in OBS Encoder, Auflösung, FPS, Bitrate und die SRT-Adresse ein. Dein Twitch-Ziel trägst du im Dashboard ein. Titel und Kategorie änderst du in den Dashboards der Plattformen.

Für Twitch und Kick werden 1080p und CBR verwendet.

Uplink nimmt deinen Stream über SRT an. Im Dashboard steht dafür genau eine SRT-Adresse.

### Was passiert, wenn kein Platz frei ist?

Der Start wird abgelehnt. Dein laufender Stream bleibt unangetastet. Für diesen Abend streamst du direkt zur Plattform, wie ohne Uplink.
