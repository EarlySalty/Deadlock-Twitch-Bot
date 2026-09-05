# Plan: Analyse-Dashboard Optik und Zeitraum

status: aktiv
datum: 2026-09-05
klasse: mittel
research: EVIDENCE.md

## Ziel

Siehe CONTRACT.md. Fertig, wenn REQ-01 bis REQ-12 im Preview-Build sichtbar erfüllt sind und Build, Lint und Tests grün laufen.

## Milestones

### M1: Zeitraum-Logik mit Test
Änderungen: `src/utils/zeitraum.ts` (parseDays(url) -> 7..365 oder 30; clampDays; kalenderFenster(days) -> max(days,30)); `tests/zeitraum.test.ts`, `tests/kalenderFenster.test.ts`; `package.json` Testliste.
Validierung: `npm test`
Stop-Regel: fremde Tests rot.

### M2: TimeRange zur Zahl, Header mit Jahr und Zahlenfeld, URL
Änderungen: `types/analytics.ts`, `Header.tsx`, `App.tsx`, `i18n/dictionary.ts`.
Validierung: `npm run build` und `npm run lint`
Stop-Regel: Typfehler außerhalb der Änderungsfläche.

### M3: Performance Mix und Stream-Aktivität
Änderungen: `RetentionRadar.tsx`, `CalendarHeatmap.tsx`, `Overview.tsx`.
Validierung: Screenshots `/dashboard` per Vite-Preview (Port 4175) bei days 30, 90, 365 nach `screens/`.
Stop-Regel: Labels schneiden oder horizontales Scrollen bei 1280px.

### M4: Farben und Donuts
Änderungen: `WatchTimeDistribution.tsx`, `FollowerFunnel.tsx`, `ViewerProfiles.tsx`, `AudienceDemographics.tsx`, ggf. `Rise.tsx`.
Validierung: `npm test` (brandPalette), Screenshot Publikum-Tab und Chat-Tab.
Stop-Regel: brandPalette meldet Fremdfarbe.

### M5: Texte
Änderungen: Methoden-Zeile raus, "Chat-Aktivität", Chat-Lücken-Hinweis in Nutzersprache.
Validierung: `grep -rn "Roh-Chat\|Aktivitaet\|peakHoursMethod" src` liefert keine nutzersichtbaren Treffer.

## Verlauf

- M1 fertig: `src/utils/zeitraum.ts` (clampDays, parseDaysParam, kalenderFenster) plus `tests/zeitraum.test.ts` und `tests/kalenderFenster.test.ts`, Testliste in package.json ergänzt. Volle Suite 222 Tests grün.

