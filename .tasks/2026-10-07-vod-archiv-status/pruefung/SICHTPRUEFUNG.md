# VOD-Archiv: Sichtprüfung

Stand: 2026-10-07. Produktionsbundle aus `bot/analytics/dashboard_v2/dist`, isolierter lesender Testserver auf 127.0.0.1:4197. Archivdaten wurden vom tatsächlichen Rust-Listenhandler gegen Wegwerf-PostgreSQL erzeugt. Das ist keine angemeldete Produktionssitzung und kein Nachweis heutiger YouTube-/Drive-Verfügbarkeit.

Moli 1.1.14 wurde als eigener Loopback-Prozess mit Layout-, Bild- und Schriftunterstützung gestartet. Playwright verbindet sich ausschließlich per CDP mit diesem Moli-Prozess. Kein Chromium-/Brave-Start, keine persönlichen Profile.

## Ergebnis der gebündelten Runde

Desktop 1440 × 1100: gemeldete Dokumentbreite 1440. Mobil 390 × 844: gemeldete Dokumentbreite 390. Je acht Einträge, acht Statusicons, keine JavaScript-Seitenfehler. Die vier Aufnahmen wurden angesehen: Status steht beim Titel links, ohne sichtbaren horizontalen Überlauf. Kein „Noch kein Versuch“. Die Eintragskoordinaten aus getBoundingClientRect sind dagegen null und belegen die Platzierung nicht. Diese Moli-Schnittstellengrenze wurde im Gate als NIT erkannt.

`desktop-abschluss.png` und `mobil-abschluss.png` zeigen bestätigten älteren Upload mit Abschlusszeit sowie Teilerfolg mit eigenem Icon, Teilezahl und tatsächlicher Versuchszeit. `desktop-oben.png` zeigt bestätigten älteren Upload ohne erfundene Zeitzeile. Ziellinks sind hervorgehoben; Ausblenden ist nachgeordnet. Der mobile Seitenkopf ist bestehender Rahmen und nicht Gegenstand des Fixes.

Keine produktive UI-Korrektur erforderlich. Genau eine gebündelte Bestätigungsrunde nach Neubau von HEAD e5c04f4d8c8ce42cd7faa27ab43ab1499b9f71cf: Erst die Aufnahme auslösen, danach DOM-Geometrie lesen. Moli liefert dann reale Grenzen. Titel und Status beginnen bei x=305 auf Desktop und x=29 auf Mobil; sämtliche Statuslabels enden innerhalb des jeweiligen Viewports. Dokumentbreite entspricht jeweils dem Viewport. Acht Icons je Ansicht, keine Seitenfehler, kein „Noch kein Versuch“. SHA und sieben Asset-Hashes stehen in `moli-bestaetigung.json`.

Die Desktop- und Mobilaufnahmen `*-bestaetigung-abschluss.png` und `*-bestaetigung-unklar.png` wurden angesehen. Sie belegen bestätigten älteren Upload, Teilerfolg, aktive Übertragung, Drive-Abschluss und unklaren Altbestand mit unterschiedlichen Icons. Keine weitere Polierschleife. Nicht als Chrome-, Firefox- oder Safari-Kompatibilitätsbeweis verwenden.

TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 1 belegt | Senke: Taskakte
