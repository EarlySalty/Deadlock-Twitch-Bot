# Plan: v2-Cutover auf /streamer

status: erledigt
datum: 2026-09-04
klasse: mittel
contract: CONTRACT.md
evidence: EVIDENCE.md

Branch `feat/streamer-v2-cutover` von origin/main (99758498), Worktree
`~/.worktrees/tb-streamer-cutover`. Tests `cd website && node --test tests/*.test.mjs`
(Baseline 35 grün). Build `npm run build`.

## M1: Tests rot

`partnerPage.test.mjs` und `streamerV2.test.mjs` auf die Soll-Ablage umschreiben:
`index.html` lädt `streamer-v2.tsx`, hat `data-theme="v2"`, `robots` mit
`index, follow`, canonical `/streamer/`; `v1/index.html` lädt `main.tsx`, hat
`noindex, nofollow`, canonical `/streamer/`; `v2/index.html` existiert nicht
mehr; `vite.config.ts` hat Entry `streamerV1` auf `v1/index.html` und keinen
`streamerV2`. Roten Lauf eintragen.

## M2: HTML-Hüllen und Vite

- `website/v1/index.html` = bisherige `index.html`, nur `robots` auf
  `noindex, nofollow`, canonical bleibt `/streamer/`.
- `website/index.html` = bisherige `v2/index.html` als Grundlage plus den
  SEO-Kopf aus v1 (Title, Description, OG, Twitter, JSON-LD, Preloads,
  Favicon), Texte auf Partner-Positionierung umgeschrieben, `robots index,
  follow ...` wie v1, canonical `/streamer/`. `prerender`-Attribut und
  `nav.js` nur übernehmen, wenn sie für v2 nötig sind (prüfen, was das
  Attribut in vite.config.ts oder Plugins auslöst).
- `website/v2/index.html` löschen; `vite.config.ts`: `main` bleibt
  index.html, `streamerV2` -> `streamerV1: v1/index.html`.
Validierung: Tests grün, Build grün, `dist/index.html` enthält
`streamer-v2`-Asset, `dist/v1/index.html` enthält `main`-Asset, kein `dist/v2/`.

## M3: Abschluss Branch

Commit, push. Merge, Deploy und Caddy macht die Hauptsession:
Caddy-Redirects (`/streamer/v2*` -> `/streamer/` 308, `/streamer/v1` ->
`/streamer/v1/` 308), `caddy validate`, `systemctl reload caddy`, rsync dist,
Live-Prüfung per curl (Status, robots, Title) für `/streamer/`, `/streamer/v1/`,
`/streamer/v2/`.

## Roter Lauf (M1)

`node --test tests/*.test.mjs` nach dem Umschreiben: 38 Tests, 33 grün, 5 rot.

- `not ok - index.html ist die indexierbare Partner-Landing`: `index.html` trägt
  noch die v1-Hülle (kein `streamer-v2.tsx`, kein Partner-Title, robots noch v1).
- `not ok - v1/index.html ist die alte Landing mit noindex`:
  `ENOENT: no such file or directory, open '.../website/v1/index.html'`.
- `not ok - v2/index.html existiert nicht mehr`: `v2/index.html` liegt noch da.
- `not ok - die Streamer-Landing v2 trägt den Community-Markennamen`: neuer
  Title `Deadlock Partner-Netzwerk` fehlt in `index.html`.
- `not ok - vite.config baut v1/index.html als eigenen Entry und kennt kein
  streamerV2 mehr`: Entry heißt noch `streamerV2` auf `v2/index.html`.

## Status

- M1: erledigt (Tests rot, oben festgehalten)
- M2: erledigt. `index.html` = Partner-Landing (streamer-v2.tsx, prerender, indexierbar, SEO-Kopf partner), `v1/index.html` = alte Landing (main.tsx, noindex), `v2/index.html` gelöscht, `vite.config.ts` Entry `streamerV1`, `streamer-v2.tsx` window-Guard + prerender-Export. Tests 38/38, tsc sauber, Build grün, Prerender rendert 1 Seite; dist/index.html trägt vorgerendertes "Werde Partner", dist/v1/index.html noindex, kein dist/v2/.
- M3: erledigt. Branch `feat/streamer-v2-cutover` gepusht (origin). Merge, Deploy, Caddy-Redirects macht die Hauptsession.
