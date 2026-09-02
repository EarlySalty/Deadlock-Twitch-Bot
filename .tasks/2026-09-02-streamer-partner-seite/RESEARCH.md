# Research: /streamer als Partner-Seite

status: aktiv
datum: 2026-09-02
klasse: mittel

## Auftrag

`/streamer/` ist die Partner-Seite der deutschen Deadlock-Community, nicht die Bot-Produktseite.

## Beobachtungen (belegt, Datei:Zeile)

- Live `/streamer/` ist die Vite-App in `website/` mit `base: '/streamer/'` (`website/vite.config.ts:21`). Die statische Datei `Website/dl-landing/streamer/index.html` ist nicht die Live-Strecke.
- Die produktive Landing ist `website/src/App.tsx` über `website/src/main.tsx` und `website/index.html`. Headline live: „Kein Stream endet im Leeren.“ CTA „Partner werden“ auf `buildTwitchBotAuthUrl()` (`website/src/components/sections/Hero.tsx:32-70`).
- `/streamer/v2/` ist ein zweiter Entry (`website/v2/index.html`, `website/src/streamer-v2.tsx`) mit `StreamerNetworkPage` (`website/src/pages/StreamerNetworkPage.tsx:30`). Nav verkauft Leistungen/Preise, CTA „Kostenlos verbinden“ (`website/src/components/v2/NetworkChrome.tsx:6-66`).
- Partner-Live-Daten: `useNetworkMetrics` liest `GET /twitch/api/v2/public/network` (`website/src/hooks/useNetworkMetrics.ts:3-137`). `liveDeadlock` nur bei Kategorie Deadlock.
- Visuelle v1-Kraft: `RaidDemo` mit echten Clip-Dateien unter `website/public/clips/*.mp4` und PfP unter `website/public/clips/pfp/` (`website/src/components/sections/RaidDemo.tsx:15-21, 703-751`). Zuschauerzahlen dort sind Pool-Konstanten, nicht API.
- OAuth: `buildTwitchBotAuthUrl` in `website/src/data/externalLinks.ts:31-37`. Discord: `DISCORD_INVITE_URL` Zeile 1. Sicherheit: `TWITCH_SECURITY_URL` Zeile 18.
- Tests: `website/tests/anchors.test.mjs` verlangt Navbar-IDs gegen `components/sections`. `entrypoints.test.mjs` schützt Vite-Entries. `publicAssets.test.mjs` prüft `/streamer/`-Assetpfade. `featureCards.test.mjs` hängt an der ungenutzten Feature-Sektion und bleibt bestehen.
- Brand: `data-theme="v2"` in `website/index.html:5` aktiviert Patch-Schwarz plus Gold in `website/src/theme-v2.css:10-22`. Wortmarke „Deutsche Deadlock Community“.

## Hypothesen (unbelegt — nie als Fakt weiterreichen)

- Twitch-Avatare von `static-cdn.jtvnw.net` können an der öffentlichen CSP scheitern. Fallback: Initialen plus lokale Clip-PfP.
- Mehr als ein Twitch-Embed im ersten Viewport ist schwer. Hero bekommt höchstens ein stummes Embed.

## Wahrscheinlich zu ändernde Dateien

- `website/src/App.tsx` — neue Seite einhängen
- `website/index.html` — Title, Meta, JSON-LD
- `website/src/data/partnerPage.ts` — Copy-SSO
- `website/src/components/partner/*` — sechs Sektionen
- `website/src/pages/StreamerNetworkPage.tsx` — dieselbe Seite, kein Rest-Slop unter v2
- `website/tests/partnerPage.test.mjs` — Struktur und Verbote

## Risiken / Seiteneffekte

- Anker-Test fällt, wenn die Landing weiter `Navbar.tsx` nutzt und Sektions-IDs verschwinden.
- Prerender von `main.tsx` muss die neue Seite ohne `window` in den Clips überleben.
- Zwei Live-Embeds plus Clips: Autoplay nur muted.

## Offene Fragen

- keine
