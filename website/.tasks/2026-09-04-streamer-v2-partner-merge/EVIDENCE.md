# EVIDENCE: Bestandsaufnahme /streamer/v2

## Verdrahtung
- v2/index.html:20 lädt `/src/streamer-v2.tsx`.
- src/streamer-v2.tsx:7,12 rendert `StreamerNetworkPage`.
- src/pages/StreamerNetworkPage.tsx:4 rendert `PartnerPage`.
- src/components/partner/PartnerPage.tsx:108 ist die aktuelle schlanke v2 (hero/problem/bedeutung/partner/sicherheit/abschluss).
- src/data/partnerPage.ts:9 PARTNER_COPY, :58 PARTNER_FORBIDDEN (bannt "was du bekommst", "drei schritte", "leistungen", "kanal-report"; bewusst geschnitten, wird per Nutzer-Go gelockert).

## v1 (unantastbar)
- src/App.tsx:16 komponiert Hero/StreamDay/RaidExplainer/BanFeed/Stats/Features/ClipManager/Community/Security/CTA.
- Copy dort bot-zentriert ("Was der Bot für dich macht", "Unser Bot erkennt Spam-Bots").

## Wiederverwendbare Bausteine (partner-gerahmt, aktuell tot, nicht verdrahtet)
- src/components/v2/NetworkHero.tsx: Split-Hero plus Live-Metriken plus eingebettete NetworkRaidDemo. CTA aktuell "Kostenlos verbinden"/"Kanal-Report", muss auf "Jetzt Partner werden".
- src/components/v2/NetworkLive.tsx: echtes Twitch-Embed des live Partners plus Kanal-Marquee (useNetworkMetrics). Eigenständig, kein v1-Pendant.
- src/components/v2/NetworkStory.tsx: Vorher/Nachher "Sie versickern" gegen "Sie werden übergeben" plus NetworkPillarVisuals.
- src/components/v2/NetworkRaidDemo.tsx: 721 loc Viewer-Handoff-Animation, Twitch-Embed.
- src/components/v2/NetworkProof.tsx: Live-Chat-Ticker; enthält "Kanal-Report"-Panel (Report-Teil raus, Live-Ticker/Zahlen behalten).
- src/components/v2/NetworkSecurity.tsx: interaktives Dashboard plus Trust-Karten ("Nur echte Partner", "Raus in einer Minute").
- src/components/v2/NetworkChrome.tsx: Nav plus ProtocolSection-Rahmen plus Ambient-Rhythmus.
- src/components/v2/NetworkAmbient.tsx: dekorative Hintergrund-Lichtebene, respektiert prefers-reduced-motion.
- src/components/v2/NetworkPillarVisuals.tsx: Säulen-SVGs (schutz/coaching/clips/raid).
- src/components/v2/NetworkOffer.tsx: Preis-/Plan-Grid. NICHT verwenden (SaaS-Preisseiten-Gefühl, Nutzer-Verbot).

## Fehlt für die volle Landing (aus v1 übernehmen, partner-gerahmt neu betextet)
- "Was zum Netzwerk dazugehört": v1 Features/StreamDay/ClipManager/Community-Inhalte als Partner-Vorteile.
- Live-Ban-Feed (v1 BanFeed via useBanFeed-Hook) als "Spam-Schutz läuft für alle im Netzwerk".
- Zahlen (v1 Stats via useNetworkCount), teils schon in NetworkProof abgedeckt.

## Daten/Hooks
- src/hooks/useNetworkMetrics.ts (Partnerliste, liveNow, partners, settled), echte Live-Daten.
- src/data/externalLinks (buildTwitchBotAuthUrl, DISCORD_INVITE_URL, TWITCH_SECURITY_URL).
- src/data/networkPage (Copy für Network*-Komponenten), vorhanden, wird erweitert.
