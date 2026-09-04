# Contract: v2 wird /streamer, die alte Landing zieht nach /streamer/v1

status: erledigt
datum: 2026-09-04
klasse: mittel
repo: Deadlock-Twitch-Bot (website/) plus Caddyfile

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

`https://deutsche-deadlock-community.de/streamer/` zeigt die Partner-Landing
(bisher v2), indexierbar und mit vollständigem SEO-Kopf. Die bisherige Landing
bleibt unter `/streamer/v1/` erreichbar, aber nicht indexierbar. `/streamer/v2/`
leitet dauerhaft auf `/streamer/` um.

Nutzer-Auftrag (wörtlich): "die aktuelle v1 also /streamer als /v1 speichern
und die V2 als neue /streamer speichern und alles mergen pushen deployen".

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 `/streamer/` rendert die Partner-Landing (`src/streamer-v2.tsx`,
  `StreamerNetworkPage`) mit `data-theme="v2"`.
- REQ-02 Der Kopf von `/streamer/` ist indexierbar: `robots index, follow`
  (wie bisher in v1), `canonical https://deutsche-deadlock-community.de/streamer/`,
  Title, Description, Open Graph, Twitter-Card und JSON-LD werden aus der
  bisherigen v1-`index.html` übernommen und auf die Partner-Positionierung
  umgeschrieben (Partner der Deutschen Deadlock Community, Auto-Raid-Netzwerk,
  Discord; kein "Tool", "Produkt", "SaaS", "Preis", "Tarif"). Das OG-Bild
  bleibt `og-image.png`.
- REQ-03 `/streamer/v1/` rendert die bisherige Landing (`src/main.tsx`,
  `App.tsx`) unverändert im Aussehen, mit `robots noindex, nofollow` und
  `canonical https://deutsche-deadlock-community.de/streamer/`.
- REQ-04 `/streamer/v2` und `/streamer/v2/` antworten mit 308 auf
  `/streamer/`. `/streamer/v1` ohne Schrägstrich antwortet mit 308 auf
  `/streamer/v1/`.
- REQ-05 Assets beider Seiten (JS, CSS, Clips, Poster, Fonts) laden unter
  `/streamer/assets/...` und `/streamer/clips/...` wie bisher; keine 404 im
  Netzwerk-Tab beim Laden von `/streamer/` und `/streamer/v1/`.
- REQ-06 Alle Tests im `website/`-Paket grün; Tests, die auf `v2/index.html`
  zeigen, werden auf die neue Ablage umgeschrieben (Prüfung: `index.html`
  lädt `streamer-v2.tsx` und hat keinen `noindex`; `v1/index.html` lädt
  `main.tsx` und hat `noindex`).

## Invarianten (darf sich nicht ändern)

- INV-01 `src/App.tsx`, `src/main.tsx` und `src/components/sections/` bleiben
  byteidentisch; v1 ändert nur seine HTML-Hülle.
- INV-02 `src/streamer-v2.tsx`, `src/pages/StreamerNetworkPage.tsx` und
  `src/components/partner-clean/` bleiben byteidentisch.
- INV-03 Die übrigen Vite-Entries (vertriebler, affiliate-portal, onboarding,
  vergleich, faq) und ihre Ausgabepfade bleiben unverändert.
- INV-04 Caddy: nur der Block `handle /streamer*` ändert sich, nur die
  Redirect-Zeilen; CSP, Asset-Regeln, `try_files` und die dynamischen
  Routen bleiben. Vor dem Reload `caddy validate`.
- INV-05 Die zentrale `robots.txt` und die Rechtsseiten-Regeln werden nicht
  angefasst.

## Nicht-Ziele

- Inhaltliche Änderungen an v1 oder v2.
- Sitemap, Google-Search-Console, externe Links.

## Erlaubter Änderungsbereich

- website/index.html
- website/v1/index.html
- website/v2/index.html
- website/vite.config.ts
- website/tests/
- .tasks/2026-09-04-streamer-v2-cutover/
- /etc/caddy/Caddyfile

## Verbotene Änderungen

- website/src/
- website/public/
- rust/
- Caddyfile außerhalb des Blocks `handle /streamer*`

## Offene Produktfragen

- keine

## Amendments

- 2026-09-04: Verbotene Änderungen `website/src/` alt = keine Änderung -> neu = `website/src/streamer-v2.tsx` darf einen `prerender`-Export analog `src/main.tsx` bekommen und den Client-Mount in `if (typeof window !== 'undefined')` kapseln. Grund: sonst crasht der Prerender-Import in Node am `document`-Zugriff auf Modulebene, nur so wird die neue `/streamer/`-Landing vorgerendert; Komponenten bleiben byteidentisch. entschieden von Orchestrator (Briefing).
- 2026-09-04: `website/src/streamer-v2.tsx` plus `website/index.html` alt = mit Prerender -> neu = bricht das Prerender beim Build, wird `streamer-v2.tsx` auf den Ursprungsstand zurückgesetzt und das `prerender`-Attribut aus `index.html` entfernt (offener Punkt im Bericht). Grund: Fallback laut Briefing, Komponenten unangetastet. entschieden von Orchestrator (Briefing).
- 2026-09-04: `website/v1/index.html` robots alt = nur `robots` auf noindex -> neu = `robots`, `googlebot` und `bingbot` auf `noindex, nofollow`. Grund: die `googlebot`-Direktive überstimmt ein reines `robots`-noindex, sonst bliebe v1 bei Google indexierbar. entschieden von Orchestrator (Briefing).
