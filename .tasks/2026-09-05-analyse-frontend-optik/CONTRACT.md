# Contract: Analyse-Dashboard Optik und Zeitraum

status: aktiv
datum: 2026-09-05
klasse: mittel
repo: Deadlock-Twitch-Bot

Ersetzt `.tasks/2026-09-05-analyse-optik-zeitraum/` (überholt: Scope ohne `chatAnalyticsContent.tsx`).

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Auf `/analyse` wählt der Streamer den Zeitraum als Tageszahl oder ganzes Jahr; Performance Mix, Stream-Aktivität und die beiden Donuts füllen ihre Karten scharf und lesbar; Watch-Time-Verteilung, Follower-Funnel und Donuts tragen die Gold-Markenfarben; interne Methoden-Strings und Fachjargon verschwinden aus der Oberfläche.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: Der Zeitraum-Schalter im Header bietet 7d, 30d, 90d und Jahr (365). Daneben steht ein Zahlenfeld "Tage": eine Eingabe von 7 bis 365 wird mit Enter oder beim Verlassen des Feldes übernommen; Werte außerhalb werden auf 7 bzw. 365 gedeckelt. Der aktive Wert ist sichtbar markiert, auch wenn er keiner Voreinstellung entspricht (dann trägt das Zahlenfeld die Markierung).
- REQ-02: `?days=<n>` in der URL akzeptiert jede ganze Zahl von 7 bis 365 (Deckelung wie REQ-01, Unsinn fällt auf 30 zurück); der gewählte Wert wird wie bisher in die URL geschrieben. Die Kopfzeile "Zeitraum: letzte N Tage" zeigt den echten Wert.
- REQ-03: Alle Tabs und Hooks, die `days` bekommen, laufen mit jeder Zahl 7..365 weiter (`TimeRange` wird zur Zahl); kein Tab bricht bei 14 oder 365.
- REQ-04: Performance Mix: das Radar füllt die Kartenhöhe (Karte so hoch wie die Timeline-Karte daneben), Achsenbeschriftungen überschneiden weder einander noch Legende oder Fußzeile, die gestapelten Radius-Zahlen (25/50/75/100) verschwinden, die Legende erscheint nur bei zwei Serien. Die Fußzeile beschreibt nur, was gezeigt wird: kein Kategorie-Vergleich, solange keine Kategorie-Serie übergeben wird.
- REQ-05: Stream-Aktivität: die Zellen skalieren mit der Kartenbreite (keine feste 800px-Mindestbreite, kein horizontales Scrollen ab 1024px Viewport), Zellgröße mindestens 12px und höchstens 22px, Monatsbeschriftung sitzt an der ersten Spalte des Monats ohne Überlappung, die Karte ist so hoch wie "Beste Streaming-Zeiten" daneben. Das Fenster folgt dem gewählten Zeitraum (Untergrenze 30 Tage, Backend-Grenze); die Fußzeile "N Streams in X Tagen" nennt das echte Fenster.
- REQ-06: Watch-Time-Verteilung: fünf unterscheidbare Balkenfarben aus den Tokens (Verlauf danger, warning, primary, accent, success), die Vorperiode ist eine klar erkennbare gedämpfte Variante derselben Farbe (Umriss oder halbe Sättigung, nicht 35 % Deckkraft auf Schwarz); Legende und Delta-Farben passen dazu.
- REQ-07: Follower-Funnel: kein Cyan (#00D9FF, `--color-info`) mehr in Stufenbalken, Icon-Kacheln und Verläufen; die drei Stufen bilden einen Verlauf aus primary, accent, success, Icon-Kachel und Balken einer Stufe haben dieselbe Farbe, die Conversion-Skala nutzt danger, warning, success.
- REQ-08: Donuts in "Zuschauer-Segmente" (`ViewerProfiles.tsx`) und "Viewer-Typen" (`AudienceDemographics.tsx`) sind scharf (kein Rasterisieren durch verbleibende CSS-Transformationen oder Filter am Chart-Container; Ursache im Code belegen), füllen ihre Spalte (Durchmesser mindestens 160px, höchstens 220px, Ring proportional), haben keine hellen Sektor-Ränder und nutzen die Markenfarben (primary, accent, warning, success, secondary) statt Cyan, Rot und Neongrün.
- REQ-09: In "Audience Demographics" verschwindet die Zeile "Methode: weighted_chat_activity_…"; kein Methoden- oder Algorithmus-Name erscheint in der Oberfläche.
- REQ-10: Der Reiter "Chat-Aktivitaet" heißt "Chat-Aktivität" (Publikum-Tabs und interne Startseite).
- REQ-11: Der Hinweis "Roh-Chat-Lücke im Zeitraum" wird in Nutzersprache umformuliert: Überschrift "Chat-Nachrichten fehlen teilweise", Text ohne die Wörter Roh-Chat, KPI, message-basiert, Presence, Rollup, Ingestion; der Hinweis erscheint weiterhin nur, wenn das Backend `suspectedIngestionIssue` meldet, und die Backend-Notiz wird nicht mehr ungefiltert angezeigt, sondern durch den Frontend-Text ersetzt. Das Wort "Roh-Chat" verschwindet aus allen nutzersichtbaren Texten des Dashboards (auch `chatAnalyticsShared.tsx` und `chatAnalyticsContent.tsx`: "Chat-Nachrichten" statt "Roh-Chat-Nachrichten").
- REQ-12: `npm run build`, `npm run lint` und `npm test` in `bot/dashboard_v2` sind grün; `tests/brandPalette.test.ts` und `tests/scoreColors.test.ts` bleiben unverändert grün.

## Invarianten (darf sich nicht ändern)

- INV-01: Kein Rust-Code und keine API-Änderung; die Handler-Deckel (7..365, Kalender 30..365) bleiben die Wahrheit, das Frontend richtet sich danach.
- INV-02: `TimeRange` bleibt der einzige Typ für den Zeitraum; kein zweiter Zustand, kein zweiter URL-Parameter.
- INV-03: Bestehende Tests werden nicht gelöscht oder abgeschwächt; `brandPalette.test.ts` bekommt keine neuen Ausnahmen.
- INV-04: Farbwerte kommen aus den Tokens in `src/index.css` (`var(--color-*)`, Tailwind-Klassen primary/accent/success/warning/danger/secondary), keine neuen Hex-Werte in Komponenten.
- INV-05: Keine neuen Abhängigkeiten, keine Code-Kommentare, echte Umlaute in nutzersichtbaren Texten.
- INV-06: Verwaltungs-, Uplink- und Social-Media-Dashboard bleiben optisch unverändert; die beiden Heatmaps behalten ihre Cyan-Skala.

## Nicht-Ziele

- Kalender-Datumsauswahl oder Von-Bis-Bereiche.
- Kategorie-Vergleichsserie im Radar.
- Änderungen an KPI-Karten, Insights, Score-Gauges, Raid-Aktivität, Session-Tabelle.
- Backend-Themen (Bot-Ausschluss, Sprachermittlung, Auslöser der Chat-Lücken-Warnung): eigener Contract `.tasks/2026-09-05-analyse-backend-bots/`.

## Erlaubter Änderungsbereich

- bot/dashboard_v2/src/App.tsx
- bot/dashboard_v2/src/components/layout/Header.tsx
- bot/dashboard_v2/src/types/analytics.ts
- bot/dashboard_v2/src/pages/Overview.tsx
- bot/dashboard_v2/src/pages/Publikum.tsx
- bot/dashboard_v2/src/pages/InternalHomeLanding.tsx
- bot/dashboard_v2/src/pages/Viewers.tsx
- bot/dashboard_v2/src/pages/chatAnalyticsShared.tsx
- bot/dashboard_v2/src/pages/chatAnalyticsContent.tsx
- bot/dashboard_v2/src/components/charts/RetentionRadar.tsx
- bot/dashboard_v2/src/components/charts/ViewerProfiles.tsx
- bot/dashboard_v2/src/components/charts/AudienceDemographics.tsx
- bot/dashboard_v2/src/components/heatmaps/CalendarHeatmap.tsx
- bot/dashboard_v2/src/components/charts/WatchTimeDistribution.tsx
- bot/dashboard_v2/src/components/charts/FollowerFunnel.tsx
- bot/dashboard_v2/src/motion/Rise.tsx
- bot/dashboard_v2/src/i18n/dictionary.ts
- bot/dashboard_v2/src/utils/formatters.ts
- bot/dashboard_v2/src/utils/zeitraum.ts
- bot/dashboard_v2/tests/zeitraum.test.ts
- bot/dashboard_v2/tests/kalenderFenster.test.ts
- bot/dashboard_v2/package.json
- .tasks/2026-09-05-analyse-frontend-optik/

## Verbotene Änderungen

- rust/**
- bot/dashboard_v2/src/index.css
- bot/dashboard_v2/tests/brandPalette.test.ts
- bot/dashboard_v2/tests/scoreColors.test.ts
- bot/dashboard_v2/tailwind.config.*, vite.config.*, tsconfig*, eslint-Config
- website/**, bot/admin_dashboard/**

## Offene Produktfragen

- keine

## Amendments

