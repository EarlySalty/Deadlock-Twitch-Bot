# Contract: /streamer zeigt Inhalt sofort, Partner-Kacheln und Avatare laden schnell

status: aktiv
datum: 2026-09-04
klasse: mittel
repo: Deadlock-Twitch-Bot (website/)

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Wer `/streamer/` öffnet, sieht den vorgerenderten Inhalt sofort (nicht erst
nach dem Laden des JS), die drei Live-Kacheln zeigen ab dem ersten Bild eine
Stream-Vorschau statt Schwarz, und die Partner-Avatare kommen klein statt in
300 Pixel.

Nutzer-Befund (wörtlich, über Peer-Session): "die Assets laden lange, auch das
mit deinem Kanal"; sichtbar: die drei Partner-Kacheln bleiben lange schwarz,
der Block "Dein Platz im Netzwerk" kommt spät.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Vorgerenderter Inhalt sichtbar: `dist/index.html` enthält für
  keine `ScrollReveal`-Hülle mehr einen Inline-Stil mit `opacity: 0` oder
  `opacity:0`. Vor der Hydration ist der komplette Seiteninhalt sichtbar.
- REQ-02 Reveal nur unterhalb des Sichtbereichs: Nach der Hydration
  animieren nur Hüllen, die beim Laden unterhalb des Viewports lagen (kurz
  ausblenden ohne Übergang, dann beim Scrollen einblenden wie bisher). Hüllen
  im ersten Viewport bleiben durchgehend sichtbar, kein Flackern.
  `prefers-reduced-motion` bleibt respektiert (keine Animation).
- REQ-03 Live-Kacheln mit Vorschau: Jede Embed-Kachel zeigt sofort das
  Twitch-Vorschaubild des Streams (`previewImageUrl`) als Hintergrund, der
  Player legt sich darüber, sobald er läuft. Die bis zu drei Embeds laden
  nicht mehr lazy (`loading="eager"`), damit sie beim Seitenaufruf starten.
- REQ-04 Avatare in passender Größe: Für `Avatar`-Größen bis 70 Pixel wird
  die Twitch-Avatar-URL von `-300x300` auf `-70x70` umgeschrieben, bis 150
  Pixel auf `-150x150`; URLs ohne dieses Muster bleiben unverändert.
  `onError` fällt weiter auf das Monogramm zurück.
- REQ-05 Netzwerk-Fetch startet beim Modul-Import (einmalige, geteilte
  Promise), nicht erst im `useEffect` nach der Hydration; der Hook verwendet
  diese Promise. Verhalten bei Fehler und Abbruch bleibt (Status `error`,
  kein Refetch, `console.error`).
- REQ-06 Tests: ein Test prüft nach dem Build, dass `dist/index.html` keinen
  `opacity: 0`/`opacity:0`-Inline-Stil enthält (oder ersatzweise die
  ScrollReveal-Quelle `initial={false}` beim Server-Render nutzt); ein Test
  für die Avatar-URL-Umschreibung (300x300 -> 70x70, 150x150, unverändert
  bei fremdem Muster).

## Invarianten (darf sich nicht ändern)

- INV-01 Inhalte, Texte, Reihenfolge und Layout der Sektionen bleiben; nur
  Ladeverhalten und Bildgrößen ändern sich.
- INV-02 v1 (`src/App.tsx`, `src/components/sections/`, `src/main.tsx`,
  `v1/index.html`) bleibt byteidentisch. `ScrollReveal` wird auch von v1
  genutzt? Falls ja, darf sich sein Verhalten dort nur auf gleiche Weise
  ändern (kein v1-eigener Pfad, keine Kopie der Komponente).
- INV-03 Ein Fetch pro Seite bleibt; keine zweite Datenquelle, kein neuer
  Endpunkt.
- INV-04 Bestehende Tests nicht gelöscht oder abgeschwächt; Build,
  `tsc --noEmit`, `node --test` grün; Prerender läuft weiter ("Prerendered 1
  page").
- INV-05 Hero unverändert (Clips, Bühne).

## Nicht-Ziele

- Bundle-Splitting, Bildkomprimierung der Hero-Clips, CDN.
- Änderungen am Twitch-Player selbst (Autoplay-Ton etc.).

## Erlaubter Änderungsbereich

- website/src/components/ui/ScrollReveal.tsx
- website/src/components/partner-clean/PartnerNetwork.tsx
- website/src/components/partner-clean/partnerShared.tsx
- website/src/hooks/useNetworkStreamers.ts
- website/src/lib/partnerNetwork.ts
- website/tests/
- .tasks/2026-09-04-streamer-ladezeit/

## Verbotene Änderungen

- website/index.html, website/v1/index.html, website/vite.config.ts
- website/src/pages/, website/src/App.tsx, website/src/components/sections/
- website/src/components/partner-clean/ außer den zwei genannten Dateien
- rust/, Caddyfile

## Offene Produktfragen

- keine

## Amendments
