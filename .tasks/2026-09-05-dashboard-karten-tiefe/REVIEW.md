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

---

# Runde 2: Fix-Commit f5399925

## Urteil

MÄNGEL

Der M1-Fix trennt Verlauf und Schatten korrekt und nimmt den Schatten von allen
neun gemeldeten Nicht-Karten. Zwei Punkte bleiben: ein weiteres Nicht-Karten-Element
faellt durch das `rounded-xl`-Heuristik-Netz (M2), und der Worktree traegt
uncommittete, nicht beauftragte Aenderungen an der Kartenflaeche, die ich nicht
freigeben kann (P1).

## M1 verifiziert (am gebauten CSS des committeten f5399925)

- Globale `.bg-card` traegt nur noch den Verlauf, keinen Schatten:
  `.bg-card{background-color:var(--color-card);background-image:linear-gradient(155deg,#362c23...)}`.
- Kartengenaue Schatten-Regel vorhanden:
  `.bg-card.rounded-xl:not(.h-14),.bg-card.rounded-2xl,.bg-card.rounded-3xl{box-shadow:...0 18px 40px...}`.
- `.glass` und `.panel-card` behalten ihren eigenen `0 18px 40px`-Schatten (unveraendert).
- Die neun gemeldeten Nicht-Karten bekommen keinen Schatten mehr, an den Klassen belegt:
  - `src/components/verwaltung/AdManagerSection.tsx:174` (Input, `rounded-lg`),
  - `src/components/verwaltung/AdManagerSection.tsx:558` (Icon-Kachel, `rounded-lg`),
    `:589` (Chip, `rounded-lg`),
  - `src/components/charts/TagPerformance.tsx:380` (Pill, bare `rounded`), `:398` (`rounded-lg`),
  - `src/components/heatmaps/HourlyHeatmap.tsx:71`,
    `src/components/heatmaps/CalendarHeatmap.tsx:134`, `src/pages/Schedule.tsx:286`
    (Tooltips, bare `rounded`, `shadow-xl` bleibt),
  - `src/components/cards/PaidTeaserBlock.tsx:41` (Skeleton, `rounded-xl` aber per `:not(.h-14)` raus).
- Echte Karten auf allen sieben Routen behalten den Schatten: sie nutzen
  `bg-card rounded-xl`/`rounded-2xl` mit `border border-border` und matchen den Selektor.

## M2 [MITTEL] Ein weiteres Nicht-Karten-Element bekommt den Kartenschatten

Punkt 3 (Robustheit): der `rounded-xl`-Selektor ist nicht ganz dicht. Systematischer
Scan aller className-Bloecke (auch Template-Literale) auf `bg-card`+`rounded-xl/2xl/3xl`
ergibt 92 Treffer. Davon sind fuenf scheinbare Nicht-Karten in Wahrheit echte
Karten-Wrapper (korrekt beschattet): `TrialCallout.tsx:18` (`rounded-2xl p-6 md:p-8`),
`SessionTable.tsx:26`, `Category.tsx:196/210/298` (je `bg-card border border-border
rounded-xl overflow-hidden`, Tabellen-Container).

Genau ein echtes Nicht-Karten-Element bleibt:

- `src/pages/SessionDetail.tsx:248`: Tab-Button, im inaktiven Zustand
  `rounded-xl px-4 py-2 ... border border-white/10 bg-card text-text-secondary`.
  Er matcht `.bg-card.rounded-xl:not(.h-14)` und bekommt jetzt den tiefen
  `0 18px 40px`-Schatten. Ein Tab, keine Karte, schwebt damit wie eine Karte.
  In Runde 1 uebersehen, weil die Klasse in einem Template-Literal steht.

Hebel: `pages/`-Dateien sind laut Contract fuer Aenderungen gesperrt, also bleibt nur
der Selektor in index.css. Ein engerer Selektor (etwa zusaetzlich `.border-border`
verlangen) wuerde den Tab ausschliessen, aber `TrialCallout.tsx:18` (Karte ohne
`border-border`) den Schatten nehmen. Abwaegung noetig; so wie es steht, floated der
Tab.

## P1 [BLOCKER, Prozess] Worktree dreckig mit nicht beauftragten Aenderungen

`git status` zeigt uncommittete Modifikationen ausserhalb von f5399925:

- `bot/dashboard_v2/src/index.css`: `.glass`, `.panel-card` und `.bg-card` wurden vom
  soliden warmen Verlauf `#362c23 -> #1a1310` auf einen transluzenten Gold-Overlay
  `rgba(241,210,153,0.1) -> rgba(0,0,0,0.22)` umgestellt, `backdrop-filter: blur(14px)`
  aus `.glass` entfernt, `background-color` aus der globalen `.bg-card` entfernt.
- `bot/dashboard_v2/tests/dashboardLook.test.ts`: die Verlaufs-Assertion auf die neuen
  rgba-Werte umgeschrieben.

Diese Aenderungen sind NICHT in f5399925 und waren nicht Teil des Review-Auftrags.
Sie beruehren REQ-01 (die Vorgabe war "deckend und warm braun-gold, Verlauf hell nach
dunkel" mit #362c23/#1a1310; der Overlay ist ein Gold-Schleier ueber `--color-card`)
und muessen als eigener Commit mit eigenem Review durchlaufen. Waehrend meines ersten
Runde-2-Testlaufs war der Worktree mitten in dieser Bearbeitung: `index.css` schon auf
rgba, der Test noch auf #362c23, Ergebnis 254/255 (`not ok 62`). Nach Abschluss der
Bearbeitung wieder 255/255. Ein Review auf einem live editierten, dreckigen Worktree
ist nicht zertifizierbar: erst committen, dann pruefen.

## Tests und Build (Zahlen)

- Committeter f5399925: `.bg-card`-Block per `git show` verifiziert (Verlauf ohne
  Schatten, separater Schatten-Selektor), Runde-2-Build Exit 0
  (CSS index-B81t4ysQ.css bzw. index-r_3BpOzg, 2972 Module).
- Testlauf 1 gegen dreckigen Zwischenstand: Exit 1, 255 tests, 254 pass, 1 fail
  (`dashboardLook.test.ts` panel-card-Verlauf, transiente Mid-Edit-Inkonsistenz).
- Testlauf 2 gegen den fertig editierten Worktree: Exit 0, 255 tests, 255 pass, 0 fail.
- Logs: /tmp/karten-build2.log, /tmp/karten-test2.log, /tmp/karten-test3.log.

---

# Nachtrag Runde 2: Endstand HEAD (f5399925 + 4edc82a4)

## Urteil

MÄNGEL

P1 ist erledigt: die zuvor uncommitteten Aenderungen sind jetzt als `4edc82a4`
committet, der Worktree ist bis auf diese REVIEW.md sauber. REQ-01, REQ-04, Hover,
Palette und Scope sind im Endstand bestaetigt. Es bleibt genau ein Blocker: M2, die
inaktiven Tabs auf der SessionDetail-Seite floaten mit dem Kartenschatten.

## 4edc82a4 verifiziert (am gebauten CSS, index-Cwfyp5FQ.css)

- Verlauf auf `.glass`, `.panel-card`, `.bg-card` von deckend `#362c23 -> #1a1310`
  auf Alpha `rgba(241,210,153,0.1) -> rgba(0,0,0,0.22)` ueber `--color-card` umgestellt.
  Effektiv gerendert nahezu identische Farben wie vorher (0% rgb(55,45,34) ~ #362c23,
  100% rgb(27,20,16) ~ #1a1310), aber Streifen-Textur und Grundfarbe scheinen jetzt
  durch. Directional hell oben links nach dunkel unten rechts bleibt.
- Ungelayerte `.bg-card` setzt keine `background-color` mehr:
  `.bg-card{background-image:linear-gradient(155deg,#f1d2991a...)}`. Damit greift die
  Tailwind-Utility wieder: `.hover\:bg-card-hover:hover{background-color:var(--color-card-hover)}`
  gewinnt jetzt per Hover-Spezifitaet (0,2,0 vs 0,1,0) im selben Utilities-Layer.
  Vorher schlug die ungelayerte bg-color jede Hover-Utility, Buttons ohne Feedback.
  Fix korrekt.
- `backdrop-filter: blur(14px)` aus `.glass` entfernt. War wirkungslos, weil `.glass`
  eine deckende `background-color: var(--color-card)` traegt (kein sichtbarer Blur).
  Unschaedlich.
- Palette: der Verlauf nutzt nur rgba (Gold 241,210,153 und Schwarz), keine neuen
  Hex-Werte, kein Cyan/Blau. INV-01 bleibt erfuellt.
- `.glass` und `.panel-card` behalten `background-color: var(--color-card)` und ihren
  Schatten (vom Diff nicht angefasst).

## REQ-01 (Karte heller als Grund): erfuellt

Kartenflaeche effektiv lum 0.0078 (dunkelste Ecke) bis 0.0279 (hellste). Grund:
#0b0b0b 0.0033, #0f0f0e 0.0047, #101010 0.0052. Selbst die dunkelste Kartenecke
(0.0078) liegt klar ueber dem hellsten Grundton (0.0052), also rund das 1.5- bis
2.4-fache. Die Karte bleibt ueberall deutlich heller als der Grund.

## REQ-04 (Innenkacheln dunkler): erfuellt fuer die dominante Kachel

Ueber der Kartenflaeche: `bg-background/40` lum 0.0083 (vs card-mid 0.0113) und
`bg-bg/40` lum 0.0074 bleiben dunkler. `bg-white/5` bleibt heller (0.0202), siehe H1,
vorbestehend und unveraendert.

## M2 [MITTEL] bleibt offen: SessionDetail-Tabs floaten

Der Schatten-Selektor `.bg-card.rounded-xl:not(.h-14)` ist von 4edc82a4 unveraendert.
`src/pages/SessionDetail.tsx:238-252` rendert eine Leiste aus drei Tabs
(Overview, Events & Chat, Viewer-Timeline); die inaktiven Tabs tragen
`rounded-xl px-4 py-2 ... bg-card` und bekommen damit den tiefen `0 18px 40px`-
Kartenschatten. Zwei von drei Tabs schweben so wie kleine Karten. Sichtbares Artefakt
auf einer Nutzerseite, gleiche Defektklasse wie das urspruengliche M1.

Hebel: `pages/` ist per Contract gesperrt, Fix nur in index.css moeglich. Vorschlag:
den Selektor auf `.bg-card.rounded-xl.border-border:not(.h-14)` verengen (echte Karten
tragen `border border-border`, der Tab traegt `border-white/10`). Nebenwirkung:
`TrialCallout.tsx:18` (`rounded-2xl bg-card p-6`, ohne `border-border`) verloere den
Schatten und muesste dann `.panel-card` nutzen oder den Schatten bewusst entbehren.
Abwaegung liegt beim Implementierer.

## Tests und Build Endstand (Zahlen)

- `npm run build` Exit 0 (2972 Module, CSS index-Cwfyp5FQ.css). Log /tmp/kb4.log.
- `npm test` Exit 0, 255 tests, 255 pass, 0 fail, 0 skipped. Log /tmp/kt4.log.

---

# Runde 3: M2-Fix e3de4238 (HEAD)

## Urteil

FREIGABE

M2 ist ursaechlich behoben: die SessionDetail-Tab-Leiste nutzt jetzt den
Dashboard-Tab-Stil ohne `bg-card` und faellt damit aus dem Karten-Schatten-Selektor.
Kein Nicht-Karten-Element bekommt mehr den Kartenschatten. Alle Runden abgearbeitet,
REQ-01 bis REQ-05 und INV-01 bis INV-04 erfuellt, Tests und Build gruen.

## Punkt 1: Tab-Leiste ohne bg-card, Stil wie SubTabs, nur die Leiste angefasst

- `src/pages/SessionDetail.tsx:248`: inaktive Tabs jetzt
  `rounded-lg px-3.5 py-2 text-sm font-semibold transition-colors` mit
  `text-text-secondary hover:text-white`, aktiver Tab `bg-primary/85 text-bg`.
  Kein `bg-card`, kein `rounded-xl`, kein `border`, kein `shadow-lg` mehr.
- Deckungsgleich mit `src/components/layout/SubTabs.tsx:35-36`
  (`rounded-lg px-3.5 py-2 text-sm font-semibold transition-colors`, aktiv
  `bg-primary/85 text-bg`, inaktiv `text-text-secondary hover:text-white`; SubTabs
  hat zusaetzlich nur `flex items-center gap-1.5` fuer sein Icon).
- Diff-Umfang: eine Datei, ein Hunk, 3 Zeilen geaendert, ausschliesslich die
  Button-className der Tab-Leiste. Nichts sonst in `pages/`.
- Contract-Amendment (CONTRACT.md:61) erlaubt genau
  `src/pages/SessionDetail.tsx, nur die Tab-Leiste Zeilen 238-252`. Scope eingehalten.
- Nebeneffekt positiv: das hartkodierte `text-[#0D0806]` faellt weg zugunsten `text-bg`.

## Punkt 2: kein Nicht-Karten-bg-card mit rounded-xl/2xl/3xl mehr

Vollstaendiger Scan aller className-Bloecke (auch Template-Literale) auf
`bg-card`+`rounded-xl/2xl/3xl`: alle verbleibenden Treffer sind echte Karten:

- `... bg-card border border-border rounded-xl p-4/5/6 ...` (padded, bordered Karten),
- `bg-card border border-border rounded-xl overflow-hidden` (SessionTable, Category,
  Tabellen-/Content-Wrapper),
- `relative rounded-2xl bg-card p-6 md:p-8` (TrialCallout, Callout-Karte),
- `h-14 rounded-xl bg-card border border-border` (Skeleton, per `:not(.h-14)` ausgeschlossen).

Kein echtes Nicht-Karten-Element mehr (0). Der SessionDetail-Tab ist verschwunden.
Der Schatten-Selektor `.bg-card.rounded-xl:not(.h-14), .rounded-2xl, .rounded-3xl` ist
damit robust: jeder Treffer ist eine Karte oder der explizit ausgeschlossene Skeleton.

## Punkt 3: Tests und Build (Zahlen)

- `npm run build` Exit 0 (2972 Module). Log /tmp/kb5.log.
- `npm test` Exit 0, 255 tests, 255 pass, 0 fail, 0 skipped. Log /tmp/kt5.log.

## Gesamturteil ueber alle Runden

FREIGABE. M1 (Kartenschatten nur auf Karten), der Gate-Fix zum Alpha-Verlauf mit
funktionierendem Hover und M2 (SessionDetail-Tabs) sind sauber. REQ-01 (Karte klar
heller als der Grund), REQ-04 (Innenkacheln dunkler, ausser dem vorbestehenden
`bg-white/5`-Akzent, H1) und die Palette halten. Kein Scope-Verstoss. Einziger
verbliebener Hinweis ohne Blocker-Charakter: H1 (`bg-white/5` ist als Weiss-Overlay
heller als die Karte, vorbestehend, vom Auftrag nicht beruehrt).
