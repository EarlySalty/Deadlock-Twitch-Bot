# Evidence: Analyse-Dashboard Optik und Zeitraum

status: überholt
datum: 2026-09-05
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- bot/dashboard_v2/src/components/layout/Header.tsx:58: `timeRanges` mit 7/30/90 als Segment mit gleitendem Marker (`layoutId`, Zeilen 255-270).
- bot/dashboard_v2/src/App.tsx:193-196: URL-Parse `days` lässt nur 7/30/90 zu; App.tsx:227 schreibt `days` zurück in die URL.
- bot/dashboard_v2/src/pages/Overview.tsx:29: `useCalendarHeatmap(streamer, 365)` fest, unabhängig vom Zeitraum.
- bot/dashboard_v2/src/components/heatmaps/CalendarHeatmap.tsx:31-33: Fenster hart 364 Tage; Zeile 79 `min-w-[800px]`; Zeile 129 Zellen `w-3 h-3`; Zeilen 82-90 Monatslabels per 14px-Rechnung (überlappen "AugSep"); Zeile 167 Fußzeile "365 Tagen" hart.
- bot/dashboard_v2/src/components/charts/RetentionRadar.tsx:83-96: `h-64`, Margin 30, `PolarRadiusAxis` mit Ticks, `Legend` height 36: Ursache der Überlappung.
- bot/dashboard_v2/src/components/charts/RetentionRadar.tsx:131: Fußzeile "Benchmarks vs. Deadlock Kategorie", obwohl Overview.tsx:142 kein `categoryAvg` übergibt.
- bot/dashboard_v2/src/components/charts/WatchTimeDistribution.tsx:12-16: fünf Buckets mit drei Farben (#FF5A3C, 2x #E8A33D, 2x #00FF88); Zeile 168 Vorperiode `opacity-35`.
- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx:61-75: `from-[#00D9FF] to-[#0093AD]`, `from-[#C5A059]`, `from-success`; Zeilen 249, 264 Icon-Kachel und Balken.
- bot/dashboard_v2/src/components/charts/ViewerProfiles.tsx:40-50: Donut `w-48 h-48`, innerRadius 35, outerRadius 70.
- bot/dashboard_v2/src/components/charts/AudienceDemographics.tsx:133-143: Donut `w-32 h-32`, innerRadius 25, outerRadius 50; Zeile 217 "Methode: {data.peakHoursMethod}".
- bot/dashboard_v2/src/pages/Viewers.tsx:118-136: `RawChatGapNotice` mit "Roh-Chat-Lücke im Zeitraum", zeigt `status.note` ungefiltert.
- bot/dashboard_v2/src/pages/chatAnalyticsShared.tsx:44: "Roh-Chat-Lücke erkannt".
- bot/dashboard_v2/src/pages/Publikum.tsx:35 und InternalHomeLanding.tsx:777: Label "Chat-Aktivitaet".

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- bot/dashboard_v2/src/types/analytics.ts:766: `export type TimeRange = 7 | 30 | 90 | 365;`
- bot/dashboard_v2/src/index.css:27-48: Tokens primary #C5A059, accent #E0BE86, success #43b581, warning #E8A33D, danger #FF5A3C, info #00D9FF, secondary #9d968a.
- bot/dashboard_v2/src/utils/formatters.ts:128-144: SCORE_*-Farben, `getHeatmapColor` (Cyan-Skala, bleibt).
- bot/dashboard_v2/src/hooks/useAnalytics.ts:59-117: Hooks nehmen `days: TimeRange`, Query-Keys enthalten `days`.
- bot/dashboard_v2/src/i18n/dictionary.ts:49: 'Zeitraum: letzte {days} Tage'.
- bot/dashboard_v2/src/motion/Rise.tsx: Kartenwrapper mit Einblend-Animation (Kandidat für die Donut-Unschärfe).

## Relevante Tests (laufen vorher, laufen nachher)

- bot/dashboard_v2/tests/brandPalette.test.ts:68-91: verbietet Hex-Werte außerhalb der Gold-Palette und Tailwind-Standardfarben.
- bot/dashboard_v2/tests/scoreColors.test.ts:8-18: Score-Farbstufen.
- bot/dashboard_v2/package.json:11: Testliste für `npm test`.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- rust/crates/tb-dashboard-api/src/handlers/overview.rs:465: `days.clamp(7, 365)`.
- rust/crates/tb-dashboard-api/src/handlers/performance.rs:215,308: weekly/hourly 7..365.
- rust/crates/tb-dashboard-api/src/handlers/performance.rs:383: calendar-heatmap 30..365.
- rust/crates/tb-dashboard-api/src/handlers/performance.rs:472: viewer-timeline 1..365.
- rust/crates/tb-analytics/src/raw_chat_status.rs:280-314: `suspectedIngestionIssue` und `note` (Frontend zeigt Note künftig nicht mehr).

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- siehe CONTRACT.md, erlaubter Änderungsbereich

## Offene Architekturfrage

- keine
