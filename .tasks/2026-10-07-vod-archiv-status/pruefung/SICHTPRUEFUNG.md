# VOD-Archiv: Sichtprüfung

Stand: 2026-10-07. Produktionsbundle aus `bot/analytics/dashboard_v2/dist`, isolierter lesender Testserver auf 127.0.0.1:4197. Archivdaten wurden vom tatsächlichen Rust-Listenhandler gegen Wegwerf-PostgreSQL erzeugt. Das ist keine angemeldete Produktionssitzung und kein Nachweis heutiger YouTube-/Drive-Verfügbarkeit.

Moli 1.1.14 wurde als eigener Loopback-Prozess mit Layout-, Bild- und Schriftunterstützung gestartet. Playwright verbindet sich ausschließlich per CDP mit diesem Moli-Prozess. Kein Chromium-/Brave-Start, keine persönlichen Profile.

## Ergebnis der gebündelten Runde

Desktop 1440 × 1100: Dokumentbreite 1440. Mobil 390 × 844: Dokumentbreite 390. Je acht Einträge, acht Statusicons, kein horizontaler Dokumentüberlauf, keine JavaScript-Seitenfehler. Status bleibt beim Titel links sichtbar. Kein „Noch kein Versuch“. Die vier Aufnahmen wurden angesehen.

`desktop-abschluss.png` und `mobil-abschluss.png` zeigen bestätigten älteren Upload mit Abschlusszeit sowie Teilerfolg mit eigenem Icon, Teilezahl und tatsächlicher Versuchszeit. `desktop-oben.png` zeigt bestätigten älteren Upload ohne erfundene Zeitzeile. Ziellinks sind hervorgehoben; Ausblenden ist nachgeordnet. Der mobile Seitenkopf ist bestehender Rahmen und nicht Gegenstand des Fixes.

Keine UI-Korrekturrunde erforderlich. Die Messung aller Zustände liegt in `moli-layout.json`. Nicht als Chrome-, Firefox- oder Safari-Kompatibilitätsbeweis verwenden.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 1 belegt | Senke: Taskakte
