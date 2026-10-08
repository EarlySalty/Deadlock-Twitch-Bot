# Messung am echten Bot-Clip

## Material und Verfahren

Clip 124453: `StrangeClumsyHerringTheRinger-QwGMA1Zt2fh6loKr`, Titel „sigma movement“, Konto earlysalty. Die Datenbank liefert ein Stacked-Layout mit Facecam. Lokal vorhandene Bot-Quelldatei: 1920 × 1080, 60 fps, 48,25 Sekunden. SHA-256: `d7dbb9b104e1e519e90c1a0cd00f761b6fb48eab8639fd52ed20dd94437b0531`.

Der bestehende Rust-Befehl `render_clips` ruft denselben `render_clip_vertical`- und `render_branded`-Pfad wie der Preview-Worker auf. Gemessen wurde vor und nach der Änderung mit Rust 1.97.1. Datenbankzugang per lokalem Peer mit `default_transaction_read_only=on`; vorhandene Quelle, kein Download, kein Upload, keine Änderung an Zustimmung oder Vorschauzustand. Ausgaben liegen separat unter `/tmp/tb-vollbild-measure-20261008`.

```sh
DATABASE_URL='postgresql://nathanael@localhost/twitch_analytics?host=/var/run/postgresql&options=-c%20default_transaction_read_only%3Don' rust/target/debug/render_clips --twitch-user-id 1186925760 --out /tmp/tb-vollbild-measure-20261008/rust-after --clip-db-ids 124453
```

## Hauptmessung: echter Rust-Renderpfad

| Messwert | Vorher | Nachher |
| --- | ---: | ---: |
| Einstellung | medium, CRF 18 | veryfast, CRF 20 |
| Gesamtdauer mit DB-Lesen und FFmpeg | 267,117 s | 90,308 s |
| Dateigröße | 71.563.457 Bytes | 53.033.451 Bytes |
| Auflösung | 1080 × 1920 | 1080 × 1920 |
| Bildrate | 60 fps | 60 fps |
| Ausgabedauer | 48,300 s | 48,300 s |
| Audio | AAC, 48 kHz | AAC, 48 kHz |

Der gemessene Lauf ist 2,96-mal schneller, die Wartezeit sinkt um 66,2 Prozent. Die Datei ist 25,9 Prozent kleiner. Die Originale und Protokolle bleiben unter `rust-before/`, `rust-after/`, `rust-before.json` und `rust-after.json` im Messverzeichnis.

## Zusätzliche FFmpeg-Messungen

Identische echte Quelle, vollständige Clips, Titel-ASS und Logo, gleicher Loudnorm-Filter. Die Argumentlisten sind in den Mess-JSON-Dateien gespeichert.

| Pfad | Vorher | Nachher | Dateigröße vorher / nachher |
| --- | ---: | ---: | ---: |
| Stacked, exakt nachgebildeter Filter | 174,105 s | 71,116 s | 71.545.563 / 53.026.113 Bytes |
| Blur-Rand, ohne Facecam-Layout | 275,107 s | 133,003 s | 65.499.352 / 48.314.878 Bytes |
| Audio-Loudnorm allein | 3,205 s | unverändert | keine Videodatei |

Der Blur-Hintergrund wird mit 270 × 480 statt 1080 × 1920 gerechnet. Radius 5 auf der kleinen Fläche entspricht Radius 20 nach vierfacher Vergrößerung. Der scharfe Vordergrund bleibt in der bisherigen Zielauflösung. Der teure Vollbild-Blur entfällt. Loudnorm bleibt erhalten, weil sein isolierter Anteil klein ist.

Die Hostlast schwankte. Stacked-Vorher: Load 9,65 bis 25,00; Stacked-Nachher: 20,41 bis 21,43. Blur-Vorher: 16,77 bis 29,01; Blur-Nachher: 22,84 bis 31,86. Rust-Nachher: 17,23 bis 20,78. Diese Einzelmessungen belegen den Vorteil unter realer Last, keine garantierte feste Dauer für jeden Clip.

## Sicht- und Qualitätsvergleich

Stacked-Vergleich bei Sekunde 9: links medium/18, rechts veryfast/20. Facecam, Gameplay, Schrift, Logo und Goldkante zeigen bei der geprüften Ansicht keinen auffälligen Qualitätsverlust. Blur-Vergleich bei Sekunde 9: Vordergrund unverändert angeordnet, Hintergrund weiterhin weich. Screenshots:

- `/home/nathanael/.claude/sichtpruefung/tiktok-vollbild/vergleich-9s.png`
- `/home/nathanael/.claude/sichtpruefung/tiktok-vollbild/vergleich-blur-9s.png`

SSIM zwischen den beiden Stacked-Encodes über 2.895 Frames: Mittel 0,98937, Minimum 0,97901. Das ist ein ergänzender Vergleich der Encodes, kein Beweis identischer Bildqualität und kein TikTok-Reencode-Test.

## Andere Warteanteile

Die Hauptmessung benutzt eine vorhandene lokale Quelldatei. Download-Wartezeit fällt darin nicht an. Der Preview-Worker wartet bei leerer Warteschlange bis zu seinem bestehenden 20-Sekunden-Pollintervall; das UI prüft alle drei Sekunden. Diese Werte sind gegenüber den gemessenen Encodezeiten kleiner und wurden nicht verändert. Eine volle Warteschlange kann zusätzlich warten lassen; diese Messung behauptet keine End-to-End-Latenz der produktiven Warteschlange.
