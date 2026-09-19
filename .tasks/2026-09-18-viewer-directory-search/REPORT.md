# Viewer-Verzeichnis: stabile Suche und getrennte Ladezustände

## Ursache und Änderung

`Viewers.tsx` gab aus einem onChange-Handler einen Timer-Cleanup zurück.
React führt diesen Rückgabewert nicht aus. Jeder Buchstabe startete deshalb
einen weiteren verzögerten Request. Neue Query-Keys hatten außerdem keine
Platzhalterdaten; der globale Ladezustand entfernte das Suchfeld samt
Inhalten und Fokus. Der sofortige Seitenreset erzeugte von Seite zwei einen
zusätzlichen Request für die alte Suche.

Die Suche nutzt jetzt einen Effect mit Timer-Cleanup und setzt Suchbegriff
und Seite gemeinsam nach 300 ms Ruhe. React Query behält vorhandene Zeilen
nur innerhalb desselben Kanals und Zeitraums. Das Abbruchsignal erreicht
den bestehenden fetchApi-Pfad einschließlich seiner Timeout-Unterstützung.
Übersicht und Tabelle laden unabhängig; Suchfeld, Filter und Fehler-Retry
bleiben montiert. Nachladen wird in der Tabelle und mit einem vorlesbaren
Status angezeigt. Währenddessen sind Seitensprünge gesperrt.

Keine Änderungen an Authentifizierung, Berechtigungen, Rust-Handlern,
Migrationen, Produktionsdaten oder externen Schnittstellen.

## Verifikation

Die Browserprüfung lädt die echte Komponente mit ihren echten Hooks und
HTTP-Client in einem isolierten Chromium-Profil über einen lokalen
Vite-Testserver. Nur die API-Antworten sind kontrollierte Testdaten.
Keine externe Testbibliothek oder zusätzliche npm-Abhängigkeit nötig.

| Prüfung | Unveränderter Stand 1c8dcb85 | Mit Fix |
| --- | --- | --- |
| Neun Browser-Szenarien | 1 bestanden, 8 fehlgeschlagen | 9 bestanden, 0 fehlgeschlagen |
| Vier Request-/Abbruchtests | 1 bestanden, 3 fehlgeschlagen | 4 bestanden, 0 fehlgeschlagen |
| Gesamte npm-Tests | 356 bestanden, 7 fehlgeschlagen | 360 bestanden, dieselben 7 fehlgeschlagen |
| TypeScript und Produktionsbuild | nicht als Vorhervergleich ausgeführt | erfolgreich |
| ESLint für eigene Änderungen | nicht als Vorhervergleich ausgeführt | erfolgreich |
| git diff --check | nicht als Vorhervergleich ausgeführt | erfolgreich |

Browserfälle: langsames initiales Laden, schnelles Tippen, stabiler Fokus
und bestehende Zeilen beim Nachladen, Abbruch einer überholten Anfrage,
Erholung nach HTTP 503, Suche von Seite zwei, Kanalwechsel, Zeitraumwechsel
und leere Ergebnisse. Zusätzliche Assertions prüfen die sichtbare
Viewer-Anzahl und den Exklusiv-Prozentsatz. Der Node-Testreport zählt den
äußeren Container mit und meldet daher 10 erfolgreiche Tests für neun
Browserfälle.

Die sieben Fehler der Gesamtsuite wurden in einem eigenen unveränderten
Worktree reproduziert: drei bestehende Farbpalettenprüfungen, drei
Social-Media-Vertragsprüfungen und eine Uplink-Hilfe-Prüfung. Kein neuer
Fehler durch diese Änderung. Bestehende Vite-Hinweise zu Chunkgröße und
zukünftigem Config-Loader bleiben unverändert.

Befehle im Frontend-Verzeichnis:

```text
npm run build
npm run test:viewer-browser
node --import tsx --test tests/viewerDirectoryRequests.test.ts
npm test
node node_modules/eslint/bin/eslint.js src/pages/Viewers.tsx src/hooks/useAnalytics.ts src/api/analytics.ts src/api/core.ts tests/viewerDirectoryRequests.test.ts tests/browser/viewerDirectory.tsx tests/viewerDirectory.browser.test.mjs
```

## Betriebsstand vor Veröffentlichung

Die laufende Dashboard-Instanz verwendet den unveränderlichen Release
`44cc2aa90e1d1b59269533b5d50b643e56bfbba8` unter
`/opt/deadlock/twitch/current`. Ein Build in einem Home-Worktree allein
ändert die Live-Oberfläche nicht. Der geprüfte Ausgangsstand hat gegenüber
diesem Release keine Rust- oder Migrationsänderung.

Eine nicht authentifizierte Probe des Viewer-Endpunkts liefert erwartbar
HTTP 401. Damit ist keine Aussage über die echte Datenbanklatenz oder eine
angemeldete Live-Browsersitzung verbunden. Der Fix beseitigt die unnötigen
Anfragen und das Entfernen der Oberfläche; eine Beschleunigung einzelner
SQL-Abfragen wird nicht behauptet.

Veröffentlichung über den bestehenden Release-Installer, ausschließlich
mit `--restart twitch-dashboard`. Seine Herkunftsprüfung verlangt trotz
reiner Frontend-Änderung neu gebaute, mit dem Commit markierte Binaries.
Den aktuellen Release oder seine Herkunftsprüfung nicht überschreiben.
