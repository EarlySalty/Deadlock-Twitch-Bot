# Streamer-Dashboard auf Apple-/Emil-Standard heben

status: erledigt · 2026-08-08 · Klasse mittel · live seit 17:32

## Auftrag

Die Skills aus `emilkowalski/skills` (apple-design, emil-design-eng) sind in
`claude-config/skills/` übernommen. Deren Regeln werden jetzt auf das
Streamer-Dashboard (`bot/dashboard_v2`, Route `/analyse`) angewandt, damit die
Oberfläche denselben edlen Stand hat wie der Rest der Marke.

Nicht im Scope: neue Features, Datenpfade, API, Layout-Umbau. Nur Material,
Motion, Typografie, Mikro-Interaktion.

## Befund (Bestand, Stand 2026-08-08)

Das Farb- und Materialsystem („Industrial Gold", `bot/shared-theme/`) ist gut und
bleibt. Was gegen die Skill-Regeln verstößt:

1. **Kein Press-Feedback.** `active:scale` kommt genau 1× im ganzen Frontend vor.
   Emil: jedes drückbare Element braucht sofortiges Feedback auf `:active`.
2. **Nur eingebaute Easings** (`ease`, `ease-out`). Keine eigenen Kurven — die
   Standardkurven sind zu weich, Bewegung wirkt beliebig.
3. **Hover ohne `@media (hover: hover)`** — auf Touch-Geräten bleiben Hover-Zustände
   nach dem Tippen hängen.
4. **`transition-all` 21×** — animiert Layout- und Paint-Eigenschaften mit.
5. **`prefers-reduced-motion` nur für `internal-home-*`**, sonst nirgends; kein
   `prefers-reduced-transparency`, kein `prefers-contrast`.
6. **Tracking fix auf `-0.02em`** für h1–h4 unabhängig von der Größe.
7. **Hover-Lifts bei 240–320ms** — über dem Emil-Limit von 200ms für Hover.
8. **Zwei Dauerschleifen auf ganzer Fläche**: `hero-aura-spin` 28s,
   `internal-home-gradient-flow` 36s. Apple §14 rät von langsamen Dauer-
   Oszillationen ab.
9. **framer-motion `x`/`y`-Props** statt `transform`-String — nicht
   hardwarebeschleunigt, verliert Frames wenn der Hauptthread lädt.

## Milestones

### M1 — Motion-Fundament (`bot/shared-theme/motion.css`)

Neue geteilte Datei, importiert von `dashboard_v2` und `admin_dashboard`.
Enthält Easing-/Dauer-Tokens, globales Press-Feedback, Hover-Gate,
Reduced-Motion/-Transparency/-Contrast.

- Validierung: `npm run build` grün, `npm test` grün.
- Stop-Regel: bricht der Build oder ändert sich ein bestehender Testwert, zurück.

### M2 — Primitives

Header (durchscheinende Leiste + Scroll-Kante statt harter Linie),
TabNavigation (Indikator federt, Bewegung unterbrechbar), Modals
(Scrim + Material statt reinem Fade), KpiCard/Panel (Press weich).

- Validierung: Build + Preview-Screenshot je Zustand.

### M3 — Anti-Patterns räumen

`transition-all` → benannte Eigenschaften, Hover-Dauern ≤ 200ms,
framer-motion `x`/`y` → `transform`-String in den heiß laufenden Pfaden.

- Validierung: `grep -c transition-all` = 0, Build grün.

### M4 — Typografie · erledigt

Größenabhängiges Tracking in `bot/shared-theme/typography.css`: die Werte hängen
an den Tailwind-Größen (`--text-xs--letter-spacing` bis `--text-7xl`), von
`+0.012em` bei 12px bis `-0.038em` bei der größten Stufe. Das pauschale
`-0.02em` auf h1–h4 ist in beiden `index.css` raus; als Fallback für
Überschriften ohne eigene Größen-Utility steht eine gestaffelte h1–h6-Regel in
`@layer base` — dort, damit eine `text-*`-Klasse am selben Element weiterhin
gewinnt (ungelayertes CSS schlägt sonst jede Utility, unabhängig von der
Spezifität).

Leading bewusst unangetastet: die Regel wäre dieselbe, aber jede geänderte
Zeilenhöhe verschiebt Kachelhöhen im ganzen Dashboard, und das ist ohne
Screenshot-Vergleich nicht nachprüfbar.

`bot/dashboard_v2/src/ddc-design-tokens.css` staffelt bereits korrekt
(`--tracking-tight` −0.03em für `.h1`/`.h2`, `--tracking-snug` −0.02em für
`.h3`/`.h4`, `+0.1em`/`+0.06em` für Labels) und bleibt unverändert.

- Validierung: `--text-xs--letter-spacing:.012em` bis
  `--text-6xl--letter-spacing:-.034em` im gebauten Stylesheet, `h1{letter-spacing:-.03em}`,
  0 Treffer für die alte Pauschalregel.

### M5 — Admin-Dashboard zieht nach · erledigt

Importiert `motion.css` und `typography.css`; im Bundle nachgewiesen
(`--ease-out:cubic-bezier(.23, 1, .32, 1)`, `--text-xs--letter-spacing:.012em`).
Eigene Verstöße: eine Stelle, `Sidebar.tsx:137` `transition-all` →
`transition-[width]`. Kein `scale(0)`, kein `ease-in`, keine
Spring-Konfiguration im Repo.

## Verifikation

- `bot/dashboard_v2`: `npm run build` EXIT=0 (665ms), `npm test` 39 pass /
  0 fail, `npm run lint` EXIT=0 (0 errors, 15 vorbestehende Warnungen — keine
  in einer angefassten Datei).
- `bot/admin_dashboard`: `npm run build` EXIT=0 (383ms).
- Offen: Screenshot-Vergleich. In dieser Umgebung gibt es keine
  Browser-Automation (`preview_open` meldet „No preview automation host is
  available"), der Preview-Server läuft auf Port 4174 zum Draufschauen.

## Live

Merge `32481998` auf `main`. Zwei Gate-Durchgänge haben blockiert, beide
Befunde waren echt und sind behoben: ungelayertes CSS, das alle Tailwind-
Utilities schlug, und Übergangslisten ohne `translate`/`scale` (Tailwind v4
nutzt die eigenständigen Properties, nicht `transform`).

Der Frontend-Deploy ist der Build: `tb-dashboard` (PID 3582916, unverändert)
liest `dist` bei jedem Request von der Platte, ein Neustart wäre wirkungslos.

- `bot/analytics/dashboard_v2/dist/assets/index-CSu-HWlB.css`, 164882 Bytes,
  17:32 — vorher `index-DUwD9-FN.css` von 16:30.
- Über HTTP ausgeliefert: `https://deutsche-deadlock-community.de/twitch/dashboard-v2/assets/index-CSu-HWlB.css`
  → 200, `text/css`, 164882 Bytes, enthält `--text-xs--letter-spacing:.012em`,
  `--text-4xl--letter-spacing:-.026em`, `--ease-out:cubic-bezier(.23, 1, .32, 1)`,
  `a[data-press]:active{scale:var(--press-scale)}`.
- Die Shell verweist darauf: `/twitch/demo` und `/twitch/pricing` liefern
  `assets/index-CSu-HWlB.css` im HTML.
- `bot/admin_dashboard/dist/assets/index-B5UCzibt.css`, 69419 Bytes, 17:33,
  dieselben Anker.
- `journalctl --user -u deadlock-twitch-dashboard-rust -p err --since "5 minutes ago"`
  leer, `NRestarts=0`, `ActiveState=active`.

Zu sehen: `https://deutsche-deadlock-community.de/analyse`, Tab „Übersicht" —
Druck auf eine Kachel, Tab-Wechsel, Streamer-Dropdown oben rechts.
