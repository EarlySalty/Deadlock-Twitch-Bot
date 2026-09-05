# Review: Dashboard-Karten heben sich vom Hintergrund ab

status: aktiv
datum: 2026-09-05
commits: f2b69bd9 (Implementierung), 1398be8d (Artefakte)
reviewer: adversarialer Merge-Kritiker, read-only

## Urteil

MÄNGEL

Ein Befund mittlerer Schwere: die neue globale `.bg-card`-Regel legt den tiefen
Kartenschatten und den Gold-Braun-Verlauf auf jedes `bg-card`-Element, auch auf
Inputs, Toggle-Chips, Pills, Tooltips und Skeleton-Balken, die keine Karten sind.
Der Rest (Cascade-Wirkung, Palette, Test-Dichte, Build und Tests grün) ist sauber.

## Verifizierte Fakten

- Build: `npm run build` Exit 0 (2972 Module, Ausgabe nach ../analytics/dashboard_v2/dist, CSS index-BrIZnhnn.css 168 kB). Log /tmp/karten-build.log.
- Tests: `npm test` Exit 0, 255 tests, 255 pass, 0 fail, 0 skipped. Log /tmp/karten-test.log.
- Punkt 1 (Cascade): am gebauten CSS bewiesen. Es gibt zwei Regeln fuer `.bg-card`:
  die Tailwind-Utility `.bg-card{background-color:var(--color-card)}` liegt in
  `@layer utilities`, die globale Regel (mit Verlauf, Schatten, Ring) ist UNLAYERED.
  Unlayered schlaegt jeden Layer unabhaengig von Spezifitaet, also gewinnt die
  globale Regel. REQ-01 wirkt auf `bg-card` echt.
- Punkt 3/REQ-04 (Innenkacheln): `bg-background/40` und `bg-bg/40` (die dominante
  Innenkachel, ~200 Nutzungen) bleiben per Luminanz dunkler als die Kartenflaeche
  (0.0083 vs card_mid 0.0113; ueber der hellen Ecke 0.0155 vs 0.0271). Passt.
- Punkt 4 (Palette): alle neuen Hex (#221a15, #2a221c, #1a1310, #362c23, #0f0f0e)
  stehen in der Allowlist tests/brandPalette.test.ts:20-24. Kein Cyan/Blau als
  Flaeche. INV-01 erfuellt.
- Punkt 5 (Testaenderung): dashboardLook.test.ts nur Erwartungswerte angepasst
  (0.045 auf 0.07, 158deg/0.05 auf 155deg/#362c23, Border 0.22 auf 0.34), eine
  Assertion ERGAENZT (#1a1310), keine geloescht, `doesNotMatch(linear-gradient(0deg))`
  bleibt. Pruefdichte nicht gesenkt. INV-03 erfuellt.
- Punkt 6 (Doppel-Schatten): kein Element traegt gleichzeitig `.panel-card`+`bg-card`
  oder `.glass`+`bg-card`. Kein doppelter Schatten aus dieser Kombination.
- REQ-03: body ohne radiale Gold-Gradienten (`background: var(--gradient-bg)`),
  keine BackgroundBlobs, keine `.internal-home-vibe::before/::after`. Im Endzustand
  erfuellt (war aber bereits im merge-base 038d162d so, siehe Befund H2).
- INV-02: keine Layout-, Abstands- oder Sidebar-Aenderung; dashboardShell.test.ts
  Teil der 255 gruenen Tests.

## Mängel

### M1 [MITTEL] Tiefer Kartenschatten und Verlauf auf Nicht-Karten

`bot/dashboard_v2/src/index.css:226` (die neue globale `.bg-card`-Regel mit
`box-shadow: ... 0 18px 40px rgba(0,0,0,0.6)` und dem 155deg-Verlauf) wirkt laut
gebautem CSS auf jedes `bg-card`-Element, nicht nur auf Karten. Betroffene
Nicht-Karten:

- `src/components/verwaltung/AdManagerSection.tsx:174`: Zahlen-Input in einer
  `bg-background/50`-Kachel (Zeile 160). Der Input bekommt jetzt den vollen
  Verlauf (oben #362c23, heller als die Kachel) plus 18px/40px-Schlagschatten und
  schwebt hell ueber seiner dunkleren Kachel. Hierarchie invertiert; die anderen
  Inputs derselben Datei nutzen `bg-background/50` (dunkel, versenkt).
- `src/components/verwaltung/AdManagerSection.tsx:558,589`: Toggle-Chips
  (`bg-card text-text-secondary`), jetzt kleine Chips mit Kartenschatten.
- `src/components/charts/TagPerformance.tsx:380`: Keyword-Pill `px-2 py-0.5`,
  `:398`: Mini-Kachel `rounded-lg p-2`.
- `src/components/heatmaps/HourlyHeatmap.tsx:71`,
  `src/components/heatmaps/CalendarHeatmap.tsx:134`, `src/pages/Schedule.tsx:286`:
  Tooltips `px-2 py-1 text-xs ... shadow-xl`. Der unlayered `.bg-card`-Schatten
  ueberschreibt ihr `shadow-xl`.
- `src/components/cards/PaidTeaserBlock.tsx:41`: Skeleton-Balken `h-14`.

REQ-02 knuepft den tiefen Schlagschatten an "jede Karte", nicht an jedes
`bg-card`. Fix-Richtung: den tiefen Schatten auf die Karten-Wrapper (`.panel-card`,
`.glass`) beschraenken und die genannten Nicht-Karten-Stellen auf `bg-background/*`
umstellen oder der globalen `.bg-card` nur Verlauf und Kante ohne den 40px-Schatten
geben. Widerspricht der Nutzer-Praeferenz "kein Glow, sauberer Look" fuer Inputs
und Inline-Elemente.

## Hinweise (kein Blocker)

### H1 [GERING] REQ-04 woertlich verletzt fuer `bg-white/5`

REQ-04 nennt `bg-white/5` als Innenkachel, die dunkler als die Karte bleiben soll.
Ein Weiss-Overlay hellt aber immer auf: Luminanz 0.0202 ueber card_mid (0.0113),
0.0397 ueber der hellen Ecke, 0.0146 ueber der dunklen Ecke. Damit ist `bg-white/5`
ueberall heller als die Kartenflaeche. Vorbestehend, vom Diff nicht angefasst; die
dominante Innenkachel `bg-background/40` bleibt korrekt dunkler. Nur relevant, wo
`bg-white/5` als versenkte Innenkachel statt als heller Akzent dient (etwa
`src/components/tables/SessionTable.tsx`, `src/pages/StreamReports.tsx`). Kein
durch diese Commits verursachter Regress.

### H2 [INFO] EVIDENCE stale zu REQ-03

`EVIDENCE.md` beschreibt BackgroundBlobs, die drei radialen body-Gold-Gradienten
und `.internal-home-vibe::before/::after` als noch vorhanden. Im merge-base
038d162d waren sie bereits entfernt. REQ-03 ist im Endzustand erfuellt, aber nicht
durch f2b69bd9/1398be8d. Kein Code-Defekt, nur eine ungenaue Bestandsaufnahme.
