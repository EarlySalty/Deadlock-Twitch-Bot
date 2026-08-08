# Bewegungs-Standard auf alle Streamer-Dashboards ziehen

Status: erledigt
Branch: `feature/motion-rollout-streamer`
Vorlage: `.tasks/2026-08-08-dashboard-apple-polish/PLAN.md` (Commit `32481998`)

## Ausgangslage

Der zentrale Hebel war schon gesetzt: `/twitch/dashboard` liefert `dashboard_v2`,
und dessen `index.css` zieht `shared-theme/motion.css` + `typography.css`. Tailwind
v4.3.3 gated `hover:`-Utilities selbst hinter `@media (hover:hover)` — im dist-CSS
nachgewiesen. Offen war nur, was ein CSS-Hebel prinzipiell nicht erreicht: die
Auftritte, die framer-motion auf dem Hauptthread rechnet.

## Befunde

| # | Befund | Stellen | Reaktion |
|---|---|---|---|
| 1 | `initial={{opacity:0,y:20}}` — `y` läuft über rAF, nicht über den Compositor | 94 | auf CSS-Klasse `.rise-in` umgestellt |
| 2 | Gerechnete Staffelung `delay: i * 0.1` — letzter Eintrag nach einer Sekunde | 36 | auf `Math.min(off + i*0.04, 0.24)` normalisiert |
| 3 | `scale: 0` als Startwert | 1 (`FeaturePicker`) | `scale: 0.6` + Deckkraft |
| 4 | Wanderdistanz 16–20px | alle Rise-Stellen | 8px in `ddc-rise-in` |
| 5 | `prefers-reduced-motion` deckte `.rise-in` nicht ab | — | Klasse in den Reduce-Block |
| 6 | Hover ohne Zeiger-Gate | — | kein Befund, Tailwind v4 gated selbst |
| 7 | `duration-700` Shimmer, `duration-500` Balkenbreite | 2 | kein Befund, Absicht |

## Umsetzung

- `bot/shared-theme/motion.css`: Klasse `.rise-in` + Keyframe `ddc-rise-in`
  (260ms, `--ease-out`, 8px), Verzögerung über `--rise-delay`. Im
  Reduce-Block: Animation aus, Verzögerung auf 0.
- `bot/dashboard_v2/src/motion/rise.ts`: Delay-Rechnung, 40ms je Stufe,
  gedeckelt bei 240ms. Nimmt Sekunden aus den alten framer-Props entgegen.
- `bot/dashboard_v2/src/motion/Rise.tsx`: dünne Hülle, `as`-Prop für
  section/aside/span.
- `tools/rise_rewrite.py`, `tools/rise_imports.py`, `tools/stagger_normalize.py`:
  der mechanische Umbau, damit 94 Stellen nicht von Hand wandern.

Ausgenommen: `pages/SocialMedia.tsx`, `pages/SocialMediaAdmin.tsx`,
`bot/admin_dashboard/`, `/twitch/overlay`, `/twitch/pause-loop`.

## Bewusst liegen geblieben

- 16 `whileInView`-Stellen: anderer Auslöser (Scroll), braucht einen eigenen
  Umbau über IntersectionObserver oder `animation-timeline: view()`.
- Stellen mit `exit`: ohne `AnimatePresence` gibt es keinen Abgang.
- `delay: (weekday * 24 + hour) * 0.002` in der Heatmap: Klammerausdruck, vom
  Normalisierer nicht erfasst, Maximum liegt bei ~336ms.
- Dauerschleifen (`hero-aura-spin` 28s, `internal-home-gradient-flow` 36s,
  `shimmer` 4s, `logo-spin` 20s): stehen bei reduced-motion still, sonst
  unverändert.
- Tote Klasse `.stagger-children` — wird nirgends benutzt.

## Validierung

| Befehl | Ergebnis |
|---|---|
| `npx tsc -b --force` | Exit 0 |
| `npm test` | Exit 0, 44/44 (vorher 39) |
| `npm run lint` | Exit 0, 15 vorbestehende Warnungen |
| `npm run build` | Exit 0, 940ms, CSS 164.58 kB |

Rot-Gegenprobe: `RISE_MAX_DELAY_MS` 240 → 999 gesetzt, 2 von 5 Tests fielen um,
danach zurückgesetzt.

dist-Nachweis in `bot/analytics/dashboard_v2/dist/assets/index-*.css`:
`.rise-in`, `--rise-delay`, `ddc-rise-in`, `--ease-out:cubic-bezier`,
`--text-xs--letter-spacing` je vorhanden.
