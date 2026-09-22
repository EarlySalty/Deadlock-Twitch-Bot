# Gemeinsamer Dashboard-Rahmen und Social-Studio-UI

## Auftrag

Das neue Social-Media-Design erhalten, aber wieder dieselbe globale Größe wie Home und Uplink verwenden. UI-Fehler im Social-Media-Bereich beheben. Keine Änderungen an Posting-, Freigabe-, OAuth- oder Uplink-Logik.

Branch: `fix/dashboard-shell-social-ui`.
Basis: `8bbfb496` auf `origin/main`.
Eigener Worktree: `/home/nathanael/.worktrees/tb-dashboard-shell-social-ui`.

## Ursache und Änderung

Der Umbau hatte in `DashboardShell.tsx` einen Sonder-Rückgabepfad für Social Media angelegt: 1920 px Maximalbreite, 216 px Sidebar, eigene Innenabstände und eine bildschirmhohe Sidebar. Die gemeinsame Shell verwendet dagegen 1680 px, 240 px Sidebar und 20 px oberen Abstand.

- Sonder-Shell entfernt. Alle Routen verwenden dieselbe Komponente und Geometrie; Studio-Schriften und Farben sind auf den Main-Inhalt begrenzt.
- Sidebar verwendet auch im Studio dieselbe Kartenform, Breite, Polsterung und Sticky-Position. D-Logo und goldene aktive Navigation bleiben erhalten.
- Desktop-Menüknopf ausgeblendet. Die ungeschichtete CSS-Regel `.studio-button { display: inline-flex }` hatte Tailwinds `lg:hidden` überstimmt; die responsive Sichtbarkeit steht jetzt ebenfalls im Studio-CSS.
- Abstand zwischen Kennzahlen, Vorratshinweis und Filterleiste wiederhergestellt. Listen-/Kartenansicht hat einen sichtbaren Auswahlzustand.
- Clip-Zeilen richten sich nach ihrer tatsächlichen Kartenbreite. Bei weniger Platz stehen Aktionen unter dem Text; Menüs bleiben am rechten Kartenrand. Kopf-Aktionen können umbrechen.

## Prüfungen

### Rot-Gegenprobe

Unverändertes Produktionsbundle vor dem Fix mit den neuen Prüfungen:

- Shell-Regressionstest: 10/11 bestanden, neuer Test rot.
- Browservergleich bei 1024 px: Social-Sidebar x=0, Breite=216, Main x=216/y=0; Home-Sidebar x=24, Breite=240, Main x=284/y=20. Vergleich rot.
- Abstand zwischen Kennzahlen und Vorrat: 0 px, Prüfung rot.

### Endstand

- `npm run build`: erfolgreich, TypeScript und Vite.
- ESLint für alle geänderten TypeScript-/TSX-/MJS-Dateien: Exit 0.
- `git diff --check`: sauber.
- Fokussierte Tests: 68/68 bestanden.
- Produktionsbundle im isolierten Browser mit API-Fixtures: 16/16 bestanden. Keine Mutationen an Produktionsdaten.
- Home, Uplink und Social Media bei 1024, 1280, 1440, 1920 und 2560 px exakt gleiche Frame-, Sidebar- und Main-Koordinaten. Messwerte: `browser/shell-geometry.json`.
- Alle vier Studio-Bereiche bei 320, 390, 768, 1024, 1280, 1440, 1920 und 2560 px ohne horizontalen Seitenüberlauf. Mobile Navigation, Listen-/Kartenmodus, nicht verdeckte Aktionsmenüs und nicht abgeschnittene Karteninhalte geprüft.
- Bestehende Freigabe-, Zeitplan-, Teilfehler-, Retry-, Kanalwechsel-, Layout- und Dialogprüfungen weiterhin grün.

### Vorbestehende Fehler der Gesamtsuite

Frische Gegenprobe auf separatem, unverändertem Basis-Worktree `8bbfb496`: Kalender 9/9, Hauptsuite 386/391. Mit Fix: Kalender 9/9, Hauptsuite 387/392. Dieselben fünf Fehler, keine neue Regression:

1. Hex-Werte außerhalb der Industrial-Gold-Palette.
2. Tailwind-Standardfarben.
3. Weißer Text auf heller Markenfläche.
4. Veraltete Sidebar-Skeleton-Quelltextprüfung.
5. OBS-Hilfe: Uploadbudget und geprüfte Plattformausgabe.

Diese fremden Bestandsfehler wurden nicht in den UI-Fix hineingezogen. Build-Warnungen zu zentral bereitgestellten Markenfonts, Vite-Konfigurationsloader und Bundle-Größe bestanden ebenfalls schon vor der Änderung.

## Auslieferung

Review, Merge und Live-Auslieferung werden separat protokolliert. Dieser Bericht allein behauptet keinen erfolgten Deploy.
