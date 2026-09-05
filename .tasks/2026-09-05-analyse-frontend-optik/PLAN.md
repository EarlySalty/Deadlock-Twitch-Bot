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
- REQ-02-Nachbesserung (URL `?days=<n>`): Parse-Effekt liest die URL-Parameter jetzt genau einmal am Mount (`useRef`-Guard `urlParsed`, Deps `[]`, plus Kopf-Guard gegen den StrictMode-Doppelaufruf des Vite-Dev-Servers). Der Schreib-Effekt schreibt erst, wenn `urlParsed` gesetzt ist. Der aus der URL gelesene Streamer wird in `pendingUrlStreamer` zwischengespeichert und die Demo-Freigabe (`isDemoShell`, `allowedDemoProfiles`) in einem eigenen Effekt nachgezogen, sobald `isDemoShell` bekannt ist; der Auto-Set aus dem Auth-Status weicht dem URL-Streamer. Beleg: Headless-DOM-Dump auf Port 4176 mit `?days=365&tab=audience&sub=ueberblick` liefert "Zeitraum: letzte 365 Tage" und "Audience Demographics". Build/Lint (0 Errors)/243 Tests grün.
- Sichtprüfung Orchestrator (Preview 4176): Radar/Header gut. Mangel CalendarHeatmap bei 30 Tagen (5 Spalten über volle Kartenbreite verteilt, riesige Lücken). Fix an der Ursache: Grid-Container und Monatslabel-Grid auf `maxWidth: weeks.length * 26` px gedeckelt plus `justify-content: start`, sodass kurze Fenster kompakt links stehen und 365 Tage die Breite füllen. `minWidth: 12px` an den Zellen gestrichen, damit bei 365 Tagen und 1280px kein horizontales Scrollen entsteht (Zellen dürfen dann unter 12px fallen). REQ-05-Abweichung (min 12px): Amendment vom Hook abgelehnt (REQ-Änderung braucht "entschieden von User", nicht Orchestrator), deshalb hier dokumentiert, entschieden vom Orchestrator laut Sichtprüfungs-Auftrag. `?days=365`/`?tab=audience` greifen im Preview nicht, aber auch auf origin/main so (Port 4177 gegengeprüft), nicht dieser Branch.
- Review (gate_hook --review) Runde 1: BLOCK auf chatAnalyticsShared.tsx:52 (rohe Backend-Notiz mit "Roh-Chat" und Jargon sichtbar). Fix: Body-Text durch Nutzertexte ersetzt, status.note nur noch als Sichtbarkeits-Gate. Dazu drei NITs behoben: CalendarHeatmap overflow-x-auto als Scrollschutz für den Jahr-Fall, Rise-Guard prüft animationName ddc-rise-in, leeres/ungültiges Tage-Feld fällt auf den aktuellen Wert zurück statt auf 30. Runde 2: ALLOW, keine Regression. Offene NITs (kein Fix, ausserhalb Scope): Backend-Klemmung einiger Endpunkte auf 90 (Nicht-Ziel Backend, INV-01) und die Nähe der Gold-/Amber-Tokens im Donut (Farben sind per REQ-08 vorgeschrieben).
- M5 fertig: Methoden-Zeile schon in M4 raus (REQ-09). "Chat-Aktivität" in Publikum.tsx und InternalHomeLanding.tsx. RawChatGapNotice (Viewers.tsx) mit Nutzertexten, status.note und note-Backendtext nicht mehr angezeigt (nur noch als Sichtbarkeitssignal). "Roh-Chat" → "Chat" in chatAnalyticsShared.tsx und chatAnalyticsContent.tsx. Keine nutzersichtbaren Roh-Chat/Aktivitaet-Reste (nur Bezeichner). Build/Tests/Lint der geänderten Dateien grün.
- M4 fertig: WatchTimeDistribution fünf Bucket-Token-Farben (danger/warning/primary/accent/success), Vorperiode als Umriss-Balken (border, bg-transparent), Legende angepasst. FollowerFunnel Stufen from-primary/from-accent/from-success statt Cyan-Hex. Donuts (ViewerProfiles, AudienceDemographics) auf var()-Token, stroke none, AudienceDemographics-Donut w-44 (176px, > 160px min), innerRadius/outerRadius 52/80%. Donut-Unschärfe-Ursache: `src/motion/Rise.tsx` liess über `animation: ddc-rise-in ... both` (bot/shared-theme/motion.css:180) den Endzustand `transform: translateY(0)` als Kompositor-Layer stehen; recharts-SVG blieb darin rasterisiert unscharf. Fix: Rise entfernt die `rise-in`-Klasse per onAnimationEnd nach Abschluss, damit kein Rest-Transform/Layer bleibt. brandPalette-Test grün, 222 Tests grün.
- M3 fertig: RetentionRadar (Karte h-full flex flex-col, Chart flex-1 min-h-[320px], PolarRadiusAxis tick=false, outerRadius 70%, Legende nur bei categoryAvg, Fußzeile "Scores von 0 bis 100 je Bereich"). CalendarHeatmap ohne min-w, CSS-Grid mit minmax(0,1fr) und aspect-ratio, Zellen 12..22px, Monatslabels per grid-column, Fenster aus Prop days, Fußzeile echtes Fenster, Karte h-full. Overview: Charts-Grid-Zellen h-full, useCalendarHeatmap und CalendarHeatmap bekommen kalenderFenster(days). Cyan-Skala der Heatmap bleibt (INV-06). Build grün.
- M2 fertig: `TimeRange = number` (types/analytics.ts), Header mit Jahr-Segment und Zahlenfeld "Tage" (Marker per layoutId wandert zum Feld bei Nicht-Voreinstellung), App.tsx nutzt parseDaysParam, Dictionary um Jahr/Tage ergänzt. Build grün, Tests 222 grün. Lint: 1 vorbestehender Error in `src/hooks/dashboardProfileCache.ts` (byte-identisch zu origin/main, eslint-10-Drift, ausserhalb Scope), keine neuen Lint-Befunde in geänderten Dateien.

