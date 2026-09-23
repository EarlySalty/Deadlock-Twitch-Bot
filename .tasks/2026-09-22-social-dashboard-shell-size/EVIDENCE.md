# Nachweise

## Reproduktion vor dem Fix

Social hatte einen eigenen 1920px-Rahmen und eine 216px-Navigation gegenüber gemeinsam 1680px/240px. Neue Browser-Prüfungen reproduzierten drei Fehler: abweichende Geometrie, keine sichtbare Listen-/Karten-Auswahl, kein Abstand zwischen Metriken und Vorrat. Die zwei neuen Source-Guards schlugen ebenfalls vor dem Fix fehl.

## Änderung

Sonderrahmen/Sondernavigation entfernt, Logo in Workspace-Kopf erhalten, Studio-Theme auf Inhalt begrenzt. Gemeinsame Sidebar erhält min-w-0 gegen mobilen Grid-Überlauf. Clip-Zeilen orientieren sich per Container Query am tatsächlich verfügbaren Platz; Aktionen umbrechen, Menüs bleiben rechts im Bildschirm. Ansichtsbuttons, Metrik-Abstände/Trennlinien und Dialog-Scrollfläche korrigiert.

## Prüfungen

Produktionsbuild (tsc + vite): Exit 0. Geänderte TSX-Dateien und Shell-Tests: ESLint Exit 0. Fokussiert: 62/62 Tests.

Vollsuite: Kalender 9/9; übrige Suite 388/393. Exakt dieselben fünf Fehler auf unverändertem Basis-Commit 8bbfb496 separat reproduziert (dort 386/391): drei Palette-Checks, alter Sidebar-Skeleton-Source-Check und OBS-Hilfetext. Keine neue Suite-Regression. Logs: baseline-full.log und test.log.

Browser: gebautes Produktionsbundle gegen isolierte API-Fixtures, keine Produktions-Schreibzugriffe. Shell-Geometrie/Sidebar-Stil werden zwischen Home, Uplink und Social Media verglichen; alle vier Social-Bereiche von 320 bis 2560px geprüft. Freigaben, Plan-Speichern/Teilfehler/Fremdänderungen, Template-Auswahl, Kanalwechsel, Menüfokus und Dialoge werden ausgeführt. 20/20 Browserprüfungen bestanden, einschließlich langer Dialogtitel bei 320/390px. Ergebnis siehe browser.log; Messwerte und Screenshots unter browser/.

## Integration / Live

Noch nicht integriert oder deployed. Review-Gate und Live-Nachweis getrennt vom Implementierungsstand führen.

## Ergänzung 2026-09-22, abends: Volle Breite und Gold-Navigation (Nutzer-Entscheid)

Der Nutzer wollte die Breite von vor dem Redesign zurueck (REQ-07: volle Breite)
und die farbliche Navigation behalten. Die 1680px-Kappe aus dem Vormittags-Fix
kam erst mit dem Redesign (29dd9c2d) und ist entfernt; die Sidebar erhaelt auf
der Social-Route den Studio-Farblayer (Gold-Aktivzustand, Inset-Balken), Flaeche
und Geometrie bleiben mit Home/Uplink identisch.

Geaendert: DashboardShell.tsx (Kappe raus), DashboardSidebar.tsx
(studio-navigation-Klasse), studio.css (Farblayer), dashboardShell.test.ts und
socialStudio.browser.test.mjs (Guard auf volle Breite, Geometrie ohne Kappe).

Pruefungen: tsc+vite Exit 0, 44/44 Unit-Tests in vier Shell- und Studio-Testdateien (dashboardShell, socialStudioRedesign, socialMediaContract, socialMediaLayout), 20/20 Browser-Tests
gegen Produktionsbundle mit isolierten API-Fixtures (playwright-core, System-
Chrome), ESLint Exit 0. Messwerte und Screenshots in diesem Ordner, Stand
21:32; 1920px: Sidebar x=24/240px, Main x=284/Breite 1612px.

## Ergänzung 2026-09-23: Marken-Farben der Sidebar global zurück

Der Nutzer wollte das Redesign auf die Social-Media-Seite selbst begrenzt. Die gemeinsame Sidebar und der Shell-Hintergrund sind auf den Stand vor 29dd9c2d zurückgesetzt: panel-card mit card-glow, Gold-Aktivzustand mit Inset-Balken, gradient-accent-Avatar, Plan-Badge in Gold, Großbuchstaben-Gruppentitel. Der studio-navigation-Sonderfall aus f3ec187d ist entfernt; die Shell trägt wieder internal-home-vibe statt der ui-root-Fläche. Volle Breite und 240px-Spalte bleiben wie im Shell-Fix.

Dabei entdeckte Falle: panel-card setzt ungeschichtet position:relative und überstimmt damit das geschichtete lg:sticky; lg:top-5 wirkte dann als relative Verschiebung und schob die Sidebar 20px nach unten. Sticky liegt jetzt auf dem Rise-Element, die Karte auf dem inneren Div, gemessen wieder bei top=20.

Prüfungen: tsc+vite Exit 0, 51/51 fokussierte Unit-Tests, ESLint Exit 0, 21/21 Browser-Tests gegen das Produktionsbundle. Der Gold-Test prüft den Aktivzustand jetzt auf jeder Route gegen eine bg-primary/10-Referenzmessung, shell-geometry.json und Studio-Screenshots sind neu.
