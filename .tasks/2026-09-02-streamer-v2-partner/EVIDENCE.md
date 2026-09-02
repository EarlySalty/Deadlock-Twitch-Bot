# Evidence: Streamer-Landing v2 Partnerschaft

status: aktiv
datum: 2026-09-02
contract: CONTRACT.md

## Fundstellen (Stand main e84fa692)

- website/src/components/v2/NetworkChrome.tsx:8-14 : Nav-Einträge (Partner, Das Problem, Ablauf, Leistungen, Zahlen, Preise, Fragen) als Array mit href und label.
- website/src/components/v2/NetworkChrome.tsx:72 : Nav-Knopf "Kostenlos verbinden".
- website/src/components/v2/NetworkHero.tsx:65 : Badge "Für deutschsprachige Deadlock-Streamer".
- website/src/components/v2/NetworkHero.tsx:74-79 : Headline "Kein Stream endet im Leeren." mit Gradient-Span.
- website/src/components/v2/NetworkHero.tsx:89 : Subline "Gehst du offline, übergibt das Netzwerk ...".
- website/src/components/v2/NetworkHero.tsx:113,120 : Knöpfe "Jetzt kostenlos verbinden" und "Kanal-Report holen".
- website/src/components/v2/NetworkRaidDemo.tsx:560-563 : Stempel "Übergabe im Netzwerk · Beispielablauf" und "Netzwerk aktiv".
- website/src/components/v2/NetworkOffer.tsx:34-37 : PricingSection id "preise", Stempel "08 · Preise", Headline "Kostenlos bleibt kostenlos.".
- website/src/components/v2/NetworkOffer.tsx:157-160 : ObjectionsSection id "einwaende".
- website/src/components/v2/NetworkOffer.tsx:263-287 : NetworkCta id "start", Knopf "Jetzt kostenlos verbinden".
- website/src/components/v2/NetworkStory.tsx:188-191 : PillarsSection id "leistungen", Stempel "04 · Was du bekommst".
- website/src/components/v2/NetworkProof.tsx:62,164 : OpenMetricsSection id "zahlen", ChannelReportSection id "report".
- website/src/components/v2/NetworkSecurity.tsx:125-129 : id "sicherheit", Headline "Du behältst die Kontrolle. Immer.".
- website/src/pages/StreamerNetworkPage.tsx:36-60 : Reihenfolge der Abschnitte innerhalb von MotionConfig.
- website/src/data/networkPage.ts:128-160 : plans mit name, price, cta, note (Plus 4,99 €, Creator Pro 9,99 € "Noch nicht buchbar").
- website/src/data/externalLinks.ts:1 : DISCORD_INVITE_URL.
- website/tests/anchors.test.mjs:27-55 : sammelt href-Anker aus allen tsx und prüft, dass jede id existiert (auch v2).
- website/tests/streamerV2.test.mjs : prüft Clip-Dateien, Rückfallebene ohne Fantasienamen, Markenname in Nav und Titel.

## Änderungsfläche

- NetworkChrome.tsx, NetworkHero.tsx, NetworkRaidDemo.tsx (nur Stempeltext), NetworkOffer.tsx, NetworkStory.tsx (Stempel), StreamerNetworkPage.tsx (Reihenfolge), networkPage.ts (nur Umsortierung), streamer-v2.css (gedimmte Extras-Karten), streamerV2.test.mjs (Partner-Texte und Nav-Reihenfolge).
