# Evidence: v2-Cutover auf /streamer

status: erledigt
datum: 2026-09-04

Stand main 99758498.

- website/vite.config.ts:27-37: Multi-Entry, `main` = index.html (v1), `streamerV2` = v2/index.html, weitere Entries vertriebler, affiliate-portal, onboarding, vergleich, faq; `base: '/streamer/'`, `outDir: 'dist'`.
- website/index.html:9: `robots index, follow`; :13-14 Title und Description (Auto-Raid-Bot-Positionierung); :18 canonical `/streamer/`; :28-37 OG und Twitter; :43 JSON-LD-Block; :159 `<script type="module" src="/src/main.tsx" prerender>`; :164 `<script src="/brand/nav.js" defer>`.
- website/v2/index.html:6: `robots noindex, nofollow`; :7 Title "Deadlock Partner Netzwerk ..."; :12 canonical `/streamer/v2/`; :18 `<script type="module" src="/src/streamer-v2.tsx">`; `data-theme="v2"` am html-Tag (Zeile 2).
- website/tests/partnerPage.test.mjs:10: liest `v2/index.html` als `htmlFile`; website/tests/streamerV2.test.mjs:51: liest `v2/index.html` (Prüfung auf noindex und Entry).
- website/src/data/sitePaths.ts:1: `WEBSITE_HOME_PATH = '/streamer/'`, einziger Pfad-Bezug im Quellcode; keine Verweise auf `/streamer/v2` in src/ außer streamer-v2.css.
- /etc/caddy/Caddyfile:412-460: Block `handle /streamer*`; :417-418 Redirect `/streamer/v2` -> `/streamer/v2/` (308); :436-446 Asset-Regel mit `strip_prefix /streamer` und `root dist`; :448-458 Fallback `try_files {path} {path}index.html /index.html`, damit `/streamer/v1/` automatisch `dist/v1/index.html` findet.
