# Plan: /streamer als Partner-Seite

status: aktiv
datum: 2026-09-02
klasse: mittel
research: RESEARCH.md

## Ziel

Fertig, wenn https://deutsche-deadlock-community.de/streamer/ die sechs Sektionen aus dem Contract zeigt, der primäre CTA Partner-OAuth startet, und Live- beziehungsweise Clip-Karten echte Partner tragen.

## Nicht-Ziele

- Pricing, Dashboard, Onboarding, FAQ umbauen
- Neue API
- Caddy anfassen

## Milestones

### M1 — Copy und Gerüst
Änderungen: `website/src/data/partnerPage.ts`, `website/src/components/partner/*`, `website/src/App.tsx`, `website/index.html`
Erwarteter Zwischenzustand: Landing rendert sechs Sektionen mit der Contract-Copy, Hero-CTA zeigt auf OAuth.
Validierung: `cd website && npm test`
Stop-Regel: Anker- oder Asset-Test rot.

### M2 — Live-Karten und Partnergrid
Änderungen: Stream-Karten, Roster an `useNetworkMetrics`
Erwarteter Zwischenzustand: bei Live ein stummes Embed, sonst Clips. Jeder Name ist ein Twitch-Link.
Validierung: `cd website && npm test` plus Dev-Server gegen die Public-API.
Stop-Regel: leere Play-Fläche oder erfundene Zuschauerzahl.

### M3 — v2 entkoppeln, SEO, Tests
Änderungen: `StreamerNetworkPage.tsx`, `v2/index.html`, `tests/partnerPage.test.mjs`
Erwarteter Zwischenzustand: `/streamer/v2/` zeigt dieselbe Partner-Seite, Title/Meta aus `partnerPage.ts`, Verbote im Test.
Validierung: `cd website && npm test` und `npm run build`
Stop-Regel: Build oder neuer Test rot.

## Verlauf

- 2026-09-02: Research und Contract geschrieben.
- 2026-09-02: M1-M3 gebaut. Tests 25/25. Rot-Gegenprobe: ohne `id="partner"` fällt der Sektions-Test (Ist: kein Treffer, Soll: `id="partner"`).
