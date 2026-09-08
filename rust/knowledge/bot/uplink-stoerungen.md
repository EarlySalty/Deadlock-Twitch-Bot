---
title: "Uplink: Störungen"
namespace: bot
category: setup
audience: streamer
last_updated: 2026-09-08
source: Uplink-Dashboard
tip_eligible: false
---

### OBS sendet, aber auf der Plattform kommt nichts an

Prüfe im Dashboard den Eingang und den Status des betroffenen Ziels. Eingeschaltet ist eine Einstellung; Medien werden gesendet bestätigt lokalen Ausgangsverkehr. Eine veröffentlichte Plattformübertragung benötigt ihre eigene Bestätigung.

### OBS kann sich nicht verbinden

Verwende Benutzerdefiniert, die öffentliche RTMPS-Serveradresse und den getrennten privaten Uplink-Schlüssel aus dem Dashboard. Übernimm beide Felder vollständig. Eine alte Gesamtadresse mit eingebettetem Schlüssel ist ungültig.

### Zugang erneuern

Der Knopf Neu verbinden führt durch die bestehende Plattformfreigabe. Fehlendes Chatrecht bedeutet nicht automatisch, dass auch ein gespeicherter Medienzugang ungültig ist. Der Status der einzelnen Funktionen bleibt getrennt sichtbar.

### Das Bild stockt

Prüfe die tatsächliche Bitrate, verfügbare Uploadkapazität und ausgelassenen Frames in OBS. Senke bei knapper Leitung das CBR-Budget oder die Quellauflösung beziehungsweise Bildrate. Eine größere Ausgangsauflösung stellt fehlende Details nicht wieder her.

### Internet bricht weg

Beachte den aktuellen Verbindungszustand und die im Dashboard gespeicherte Wiederverbindungsfrist. Ein Transportabbruch ist nicht automatisch ein bewusstes Streamende. Verlass dich nur auf das tatsächlich angezeigte Verhalten; ein Wartebild oder ein nahtloser Wechsel darf nicht aus einem eingeschalteten Ziel abgeleitet werden.
