# Stream-Aktivität bei langen Zeiträumen

## Auftrag und Ursache

Die Kalenderkarte unter `/analyse?days=3650&tab=overview&streamer=earlysalty`
zeigt überlappende Monatsnamen und keine sichtbaren Tagesfelder.

Ausgangsstand: `e9652040`. `CalendarHeatmap.tsx` presste etwa 522 Wochenspalten
mit jeweils 4 Pixel Abstand in eine einzige Kartenbreite. Die Abstände allein
benötigten mehr Platz als die Karte. `minmax(0, 1fr)` ließ die Felder auf null
Pixel schrumpfen. Hinzu kamen mehrere Tausend einzelne Motion-Animationen.

## Änderung

- Bis einschließlich 366 Tage bleibt die Tagesansicht erhalten. Tagesfelder
  haben mindestens 12 Pixel Breite. Bei Platzmangel scrollt ausschließlich die
  Karte horizontal. Beim Öffnen sind die aktuellen Tage sichtbar.
- Ab 367 Tagen werden Tage nach Jahr und Monat zusammengefasst. Monatsnamen
  stehen einmal als Kopfzeile, Jahre als eigene Zeilen, neuestes Jahr zuerst.
  Das Zehnjahresfenster vom 21.09.2016 bis 18.09.2026 umfasst 121 Monatsfelder
  statt einer einzigen Zeile mit 522 Wochenspalten. Der Zeitraum wird nicht
  auf ein Jahr verkürzt.
- Zuschauerzeit und Streams werden innerhalb desselben Zeitfensters summiert;
  die Farbskala bezieht sich auf die gewählte Metrik und Auflösung. Angebrochene
  Randmonate zeigen in den Details die tatsächlichen Datumsgrenzen.
- UTC-Datumsschlüssel verhindern Verschiebungen durch Browser-Zeitzone oder
  Sommerzeit. Auffüllfelder außerhalb des Fensters bleiben nicht auswählbar.
- Details stehen außerhalb des Scrollbereichs und funktionieren per Maus,
  Tastatur und Antippen. Keine Animation pro Feld; die bestehende Kartenanimation
  bleibt unverändert. Leere Daten zeigen ausdrücklich 0 Streams.

Keine Änderungen an APIs, Datenhaltung, Berechtigungen oder Rust-Code.

## Prüfungen

- Rote Gegenprobe am Ausgangscode: beide neuen Layouttests fehlgeschlagen (0/2).
  Der Modelltest war vor der Implementierung wegen des fehlenden Moduls rot.
- Separate Browser-Gegenprobe für die anfängliche Scrollposition: zunächst rot,
  nach dem Fix grün.
- `npm run test:calendar`: 9/9 grün.
- Modelltests zusätzlich unter UTC, Europe/Berlin und America/Los_Angeles:
  jeweils 7/7 grün, einschließlich Schalttag, Jahreswechsel und Sommerzeit.
- `npm run test:calendar:browser`: grün. Acht Kombinationen aus 30, 365, 730 und
  3650 Tagen bei 1440 und 390 Pixel Fensterbreite. Keine überlappenden Labels,
  keine unsichtbaren Felder, kein horizontaler Seitenüberlauf, Details per Klick
  und Tastatur geprüft. Tagesfelder mindestens 12 × 28 Pixel, Monatsfelder
  mindestens 25,65 × 24 Pixel in der schmalen Ansicht.
- ESLint auf allen neuen/geänderten TypeScript-Dateien: grün.
- `npm run build` (TypeScript und Vite): grün. Bestehende Warnungen zu
  `configLoader` und großer Bundle-Datei bleiben unverändert.
- `git diff --check`: grün.

Der Browser-Test verwendet ausschließlich synthetische Daten und die echte
Komponente mit den echten Dashboard-Styles. Er liest keine produktiven
Analytics und startet seinen Testserver nur auf 127.0.0.1. Er setzt einen
installierten Chromium für die gepinnte `playwright-core`-Version voraus.
Optional schreibt `CALENDAR_SCREENSHOT_DIR` die Screenshots; auf dem Prüfhost
liegen sie unter `~/.claude/sichtpruefung/calendar-heatmap-long-range-20260918/`.

## Unveränderte Fehler im Gesamttest

`npm test` enthält zusätzlich 367 bestehende Tests: 360 grün, 7 rot. Dieselben
sieben Fehler wurden in einem separaten, unveränderten Checkout von `e9652040`
reproduziert. Im neuen Stand laufen vorher alle neun Kalendertests erfolgreich.
Der Gesamttest wird ausdrücklich nicht als vollständig grün ausgewiesen.

Die bestehenden Fehler betreffen drei Farbprüfungen, drei Social-Media-Vertrags-
beziehungsweise Übersetzungsprüfungen und einen OBS-Hilfetext. Keine dieser
Dateien gehört zum Kalender-Fix. Der bestehende separate Admin-Modus-Browsertest
konnte mangels installiertem `geckodriver` nicht laufen; der neue Kalender-
Browsertest ist davon unabhängig und wurde mit Chromium ausgeführt.

Prüflogs liegen unter `/tmp/calendar-heatmap-checks-20260918/`. Der Vergleich der
Fehlerlisten zwischen Ausgangscode und Endstand war identisch.
