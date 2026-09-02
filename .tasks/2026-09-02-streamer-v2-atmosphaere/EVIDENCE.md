# Evidence: Streamer-Landing v2 Atmosphäre

status: aktiv
datum: 2026-09-02
contract: CONTRACT.md

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- `website/src/components/sections/Hero.tsx:8-82`: v1-Hero, zentrierter Text, darunter `RaidDemo` in voller Breite (`max-w-[1400px]`), CTAs unter der Bühne. Layout-Vorbild für REQ-01.
- `website/src/components/sections/RaidDemo.tsx:15-23`: Clip-Pool `streamerPool` mit `video: ${BASE}/clips/<login>.mp4`, Avatar und Farbe je Kanal. Quelle für REQ-02.
- `website/src/components/sections/RaidDemo.tsx:26-34`: `pickTwo()` wählt zwei verschiedene Kanäle ohne Wiederholung des letzten Paars.
- `website/src/components/v2/NetworkRaidDemo.tsx:48-53`: `FALLBACK_CHANNELS` mit ausgedachten Namen ohne Bild. Das ist die graue Bühne, die live zu sehen ist (Screenshot 2026-09-02, API liefert kein `avatar_url`).
- `website/src/components/v2/NetworkRaidDemo.tsx:150`: Auswahl `usable.length >= 2 ? usable : FALLBACK_CHANNELS`; hier wird die Rückfallebene auf den Clip-Pool umgestellt.
- `website/src/components/v2/NetworkRaidDemo.tsx:596-648`: `.v2-rd-stage` mit zwei Karten und `.v2-rd-art` (unscharfes Profilbild statt Video). Hier kommt das `<video>` mit Poster hinein.
- `website/src/components/v2/NetworkHero.tsx:56-186`: aktueller v2-Hero, Grid `0.78fr / 1.22fr`, Text links, Bühne rechts, Beweiszeile als Sockel (`lg:col-span-2`).
- `website/src/components/v2/NetworkLive.tsx:214-330`: `PartnersSection`, bis zu 3 `TwitchEmbed` in `md:grid-cols-2 lg:grid-cols-3`, darunter `PartnerGrid`.
- `website/src/components/v2/NetworkLive.tsx:331-379`: `PartnerGrid`, `COLLAPSED_TILES = 8`, Kacheln je Partner mit "Alle N Partner anzeigen".
- `website/src/components/effects/GlowOrb.tsx:52`: v1-Lichtkugeln, markiert mit `data-glow-orb`.
- `website/src/theme-v2.css:44-46`: `[data-theme="v2"] [data-glow-orb] { display: none }` blendet die v1-Lichtkugeln in v2 aus (Ursache der klinischen Wirkung).
- `website/src/streamer-v2.css:201-238`: `.v2-ambient`, `.v2-ambient-gold/teal`, `@keyframes v2-drift`, bestehende Lichtinseln, bisher nur im Hero eingesetzt.
- `website/src/streamer-v2.css:158-171`: `.v2-pulse` Puls-Animation für Live-Punkte.
- `website/src/streamer-v2.css:253-306`: `.v2-link`, `.v2-link-h/-v`, `@keyframes v2-link-run`, laufende Verbindungslinien der Netzwerk-Visuals.
- `website/src/streamer-v2.css:772-842`: `.v2-rd-beam-fill`, `.v2-rd-particle`, `.v2-rd-confetti`, Strahl und Partikel der Bühne existieren bereits.
- `website/src/components/v2/NetworkChrome.tsx:46`: Nav-Markenname "Deadlock Netzwerk".
- `website/src/components/v2/NetworkStory.tsx:195-216`: Aufzählungsliste in den Leistungskarten (entfällt nach REQ-07).
- `website/src/components/v2/NetworkProof.tsx:207-211`: Aufzählung im Kanal-Report.
- `website/src/components/v2/NetworkOffer.tsx:88-90`: Häkchenliste der Preiskarten (REQ-10).
- `website/src/data/networkPage.ts:22-40,54-110,128,197`: Texte für Plan-Schritte, Leistungen, Pläne, Einwände.
- `website/v2/index.html:6,13,27`: `data-theme="v2"`, Titel und OG-Titel (REQ-09).

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- `website/src/hooks/useNetworkMetrics.ts:1-174`: `NetworkMetrics`, `PartnerChannel` (isLive, liveDeadlock, avatarUrl). Einzige Quelle der Kennzahlen.
- `website/src/components/ui/ScrollReveal.tsx:1-40`: Scroll-Reveal-Wrapper.
- `website/src/components/ui/GradientText.tsx`: Gradient-Text für den Markennamen.
- `website/src/components/v2/NetworkPillarVisuals.tsx:1-194`: Visuals je Leistung.
- `website/src/components/v2/NetworkLive.tsx:38-113`: `avatarColor`, `initials`, `twitchUrl`, `LiveBadge`, `TwitchEmbed`.

## Relevante Tests (laufen vorher, laufen nachher)

- `website/tests/entrypoints.test.mjs`: Vite-Entries inklusive `v2/index.html`.
- `website/tests/publicAssets.test.mjs`: Dateien unter `public/` vorhanden.
- `website/tests/anchors.test.mjs`: Nav-Anker zeigen auf existierende Abschnitte (auch v2).
- `website/tests/themeClasses.test.mjs`: Theme-Klassen vorhanden.
- `website/tests/featureCards.test.mjs`: Leistungskarten-Daten.
- Baseline: `npm test` und `npm run build` in `website/` vor dem ersten Edit ausführen und Ergebnis in PLAN.md eintragen.

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- `website/vite.config.ts:25-38`: Entries; `base: '/streamer/'`; v2 unter `dist/v2/index.html`.
- `Caddy/hosts/v50671/Caddyfile:412-455`: `/streamer*` liefert aus `Deadlock-Twitch-Bot/website/dist`; `*.mp4` und `*.webm` sind als Assets erlaubt; frame-src erlaubt `player.twitch.tv`, `clips.twitch.tv`, `www.twitch.tv`.
- `.gitignore:59-60`: `*.mp4` ignoriert, `website/public/clips/*.mp4` ausgenommen; Poster-Bilder (png/jpg/webp) sind nicht ignoriert.

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- `website/src/components/v2/NetworkHero.tsx`: Layout zentriert, Bühne volle Breite.
- `website/src/components/v2/NetworkRaidDemo.tsx`: Clip-Pool, Video mit Poster, Statuszeile.
- `website/src/components/v2/NetworkLive.tsx`: große Live-Embeds, Avatar-Reihe für Offline.
- `website/src/components/v2/NetworkChrome.tsx`: Markenname.
- `website/src/components/v2/NetworkStory.tsx`, `NetworkProof.tsx`, `NetworkOffer.tsx`, `NetworkSecurity.tsx`: Kürzen, visuelle Anker.
- `website/src/components/v2/NetworkAmbient.tsx` (neu): Lichtinseln und Partikel für die ganze Seite.
- `website/src/streamer-v2.css`, `website/src/theme-v2.css`: Glow, Poster, Marquee.
- `website/src/data/networkPage.ts`: Texte kürzen.
- `website/public/clips/poster/*.jpg`: Standbilder aus den mp4 (ffmpeg).
- `website/tests/streamerV2.test.mjs` (neu): Clip-Pool, Poster, Markenname.
