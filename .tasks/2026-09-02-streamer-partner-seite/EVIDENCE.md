# Evidence: /streamer als Partner-Seite

status: aktiv
datum: 2026-09-02
contract: CONTRACT.md

Repo-Aufklärung vor dem ersten Edit. Jede Zeile ist eine Fundstelle `pfad:zeile`,
keine Vermutung. Der Hook (R11) gibt Quellcode-Edits erst frei, wenn hier
mindestens 3 Fundstellen stehen. Drei ist die Untergrenze, nicht das Ziel.

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- `website/src/pages/StreamerNetworkPage.tsx:30` — bisherige v2-Landing, wird durch dieselbe Partner-Seite ersetzt statt parallel weiterzuverkaufen.
- `website/src/components/v2/NetworkLive.tsx:113` — stummes Twitch-Embed mit `parent` aus `window.location.hostname`.
- `website/src/components/sections/RaidDemo.tsx:15` — Clip-Karten echter Partner aus `public/clips/`.
- `website/src/components/sections/Hero.tsx:66` — bestehender Partner-CTA auf `buildTwitchBotAuthUrl()`.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- `website/src/hooks/useNetworkMetrics.ts:63` — `useNetworkMetrics`, `PartnerChannel`.
- `website/src/data/externalLinks.ts:31` — `buildTwitchBotAuthUrl`.
- `website/src/data/externalLinks.ts:1` — `DISCORD_INVITE_URL`.
- `website/src/data/externalLinks.ts:18` — `TWITCH_SECURITY_URL`.
- `website/src/theme-v2.css:10` — Patch-Schwarz und Gold.

## Relevante Tests (laufen vorher, laufen nachher)

- `website/tests/anchors.test.mjs:33` — Sprungmarken brauchen echte IDs.
- `website/tests/entrypoints.test.mjs:49` — jede Einstiegs-HTML hat einen Vite-Entry.
- `website/tests/publicAssets.test.mjs:34` — referenzierte `/streamer/`-Assets liegen in `public/`.
- `website/tests/featureCards.test.mjs:19` — Feature-Sektion bleibt im Baum, auch wenn die Landing sie nicht mehr zeigt.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- `website/vite.config.ts:21` — `base: '/streamer/'`.
- `website/src/data/externalLinks.ts:20` — `/twitch/raid/auth`.
- `website/src/hooks/useNetworkMetrics.ts:4` — `/twitch/api/v2/public/network`.
- `website/index.html:18` — Canonical `https://deutsche-deadlock-community.de/streamer/`.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- `website/src/App.tsx` — Partner-Seite statt Feature-Landing
- `website/index.html` — SEO
- `website/src/data/partnerPage.ts` — Copy
- `website/src/components/partner/` — neue Sektionen
- `website/src/pages/StreamerNetworkPage.tsx` — gleiche Seite
- `website/tests/partnerPage.test.mjs` — neuer Wächter

## Offene Architekturfrage

- keine
