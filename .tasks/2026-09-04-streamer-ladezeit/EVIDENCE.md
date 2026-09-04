# Evidence: /streamer Ladezeit

status: erledigt
datum: 2026-09-04

Stand origin/main 687040f5.

- website/src/components/ui/ScrollReveal.tsx:20-22 und :28-36: `initial = { opacity: 0, y: 30 }` als `initial`-Prop von `motion.div`, `whileInView` mit `once`; beim Server-Render schreibt framer-motion den Initialzustand als Inline-Stil, der vorgerenderte Inhalt ist bis zur Hydration unsichtbar.
- website/src/streamer-v2.tsx:11-17: `hydrateRoot`, wenn `#root` Kinder hat; Prerender-Export :26-33 mit `renderToString`. Hydration ist korrekt, das Problem liegt nicht hier.
- website/src/components/partner-clean/PartnerNetwork.tsx:30-36: `<iframe ... loading="lazy">` für die Embeds; :42-51 `LivePreview` mit `previewImageUrl` existiert schon (nur für reduced-motion genutzt, :111).
- website/src/lib/partnerNetwork.ts: `previewImageUrl(login)` liefert `https://static-cdn.jtvnw.net/previews-ttv/live_user_<login>-640x360.jpg`.
- website/src/components/partner-clean/partnerShared.tsx:25-28: `<img src={avatarUrl} loading="lazy">`, Größe nur per CSS; die API liefert `...-profile_image-300x300.png` (Messung /twitch/api/v2/public/network, 53 Partner, alle mit Avatar).
- website/src/components/partner-clean/PartnerPitch.tsx:88 und :204: Marquee mit 14 Avataren, Größe 56, zweimal gerendert (Loop).
- website/src/hooks/useNetworkStreamers.ts:64-68: `fetch(NETWORK_API)` erst im `useEffect`, also nach Hydration.
- Messung 2026-09-04: Netzwerk-API 40 bis 70 ms, 15,8 KB; `dist/index.html` 95 KB vorgerendert, 88 ms; `prerenderEntry-*.js` 422 KB; `server.browser-*.js` (react-dom/server) wird nur vom Prerender-Import geladen, nicht vom Browser-Entry.
