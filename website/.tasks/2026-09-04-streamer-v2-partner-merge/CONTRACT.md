# CONTRACT: /streamer/v2 Partner-Netzwerk-Merge

## Ziel
`/streamer/v2` wird von der schlanken PartnerPage zur vollen Partner-Netzwerk-Landing:
v1s Substanz (Sektionstiefe, Live-Ban-Feed, Zahlen, Clip/Community/Feature-Inhalte)
zusammengeführt mit v2s guten Netzwerk-Teilen (Live-Hero, echte Partner-Embeds,
Raid-Handoff-Animation, Ambient), alles im Partner-Framing des Nutzer-Briefs.
v1 (`/streamer`) bleibt Byte-für-Byte unverändert.

## REQ
- REQ1: Sektionsreihenfolge Partner-gerahmt: Hero (Zugehörigkeit + echte Live-Proof-Zeile)
  → Live-Partner (echte Embeds, früh und groß) → Problem/Moment (Vorher/Nachher-Übergabe)
  → So läuft die Übergabe (Raid-Flow) → Was zum Netzwerk dazugehört (v1-Werte als Partner-
  Vorteile, nicht als Modulliste) → Spam-Schutz live (Ban-Feed) → Sicherheit/Vertrauen
  → Abschluss-CTA "Jetzt Partner werden".
- REQ2: Jede Sektion netzwerk-/partner-zentriert getextet. Kein "der Bot macht X" als
  Leitsatz; der Bot ist Werkzeug der Community, nicht das Produkt.
- REQ3: Primär-CTA durchgehend "Jetzt Partner werden" (→ buildTwitchBotAuthUrl), Sekundär
  "Community-Discord beitreten" (→ DISCORD_INVITE_URL). Kein "kostenlos verbinden",
  kein "Kanal-Report holen".
- REQ4: Social Proof oben = echte Live-Metriken (useNetworkMetrics: Partnerzahl, gerade live).
  Keine erfundenen Testimonials, keine erfundenen Raid-/Viewer-Zahlen.
- REQ5: Atmosphäre statt SaaS-Grid: Ambient-Ebene, warme Gold-Töne, Bewegung/Glow,
  echte Partner-Gesichter/Embeds. An die Hauptseiten-Wärme angelehnt.
- REQ6: SEO in v2/index.html: Title "Deadlock Partner Netzwerk - Auto-Raid & Streamer
  Community (Deutsch)", Description aus PARTNER_SEO-Linie. `noindex` BLEIBT vorerst
  (v2 ist Vorschau, wird erst beim Umschalten auf /streamer indexierbar).

## INV
- INV1: v1 unangetastet: `src/App.tsx`, `src/components/sections/**`, `index.html` (Root),
  `src/components/layout/**` werden NICHT verändert.
- INV2: Keine erfundenen sozialen Beweise (Testimonials/Zahlen). Nur echte Live-Daten.
- INV3: Keine Em-Dashes in nutzersichtbarem Text. Echte Umlaute (ä ö ü ß), natives Deutsch,
  kein internes Vokabular (Token, autorisieren, Opt-out) in der Copy.
- INV4: Keine Code-Kommentare in geänderten/neuen Dateien.
- INV5: `npm run build` grün; v2 rendert ohne Konsolenfehler.

## Nicht-Ziele
- Hauptseite `/` (index.html Root) wird NICHT angefasst (Nutzer hat Scope auf v2 begrenzt).
- Kein Umschalten von /streamer auf v2, kein Deploy. Bleibt Vorschau bis Freigabe.
- Kein Pricing-/Plan-Abschnitt (NetworkOffer wird NICHT eingebaut).
- Keine neuen LLM-Aufrufe, keine neuen Secrets.

## Erlaubter Bereich
- website/v2/index.html
- website/src/streamer-v2.tsx
- website/src/streamer-v2.css
- website/src/theme-v2.css
- website/src/pages/StreamerNetworkPage.tsx
- website/src/components/partner/**
- website/src/components/v2/**
- website/src/data/partnerPage.ts
- website/src/data/networkPage.ts
- website/src/components/effects/** (nur Wiederverwendung, read)

## Amendments

### A1 (2026-09-04): Kehrtwende auf v1-Klon statt Network-Merge
Der Network*-Merge-Ansatz (REQ1 mit NetworkHero/NetworkLive/Protocol-Sektionen,
Ambient, PARTNER_SECTIONS) wird verworfen. Der Nutzer bewertet das Ergebnis als
zu leer und schlechter als die produktive v1-Landing. Neuer Auftrag:

- `/streamer/v2` ist ein optischer Klon von v1 (`src/App.tsx` +
  `src/components/sections/*` + GlowOrb/Navbar/Footer), nur die Copy ins
  Partner-/Netzwerk-Framing umgeschrieben. Kein Ambient-Punkte-Layer, keine
  nummerierten Protocol-Sektionen, kein leerer Schwarzraum.
- Sektionsreihenfolge = v1: Hero, StreamDay, RaidExplainer, BanFeed, Stats,
  Features, ClipManager, Community, Security, CTA.
- Klone liegen in `src/components/partner-clean/**` (neu, im erlaubten Bereich
  ergänzt). GlowOrb/Navbar/Footer/SiteChatbot/RaidDemo werden direkt
  wiederverwendet, nicht verändert. v1-Originale bleiben unangetastet.
- Die alten Bausteine `src/components/v2/Network*`, `src/components/partner/**`,
  `src/data/partnerPage.ts`, `src/data/networkPage.ts` und
  `src/hooks/useNetworkMetrics.ts` werden entfernt; die daran hängenden Tests
  `tests/partnerPage.test.mjs` und `tests/streamerV2.test.mjs` auf den neuen
  Aufbau umgeschrieben.
- REQ4 (Live-Metriken) bleibt über die bestehende `useNetworkCount`-Kachel in
  Stats erhalten. INV1/INV2/INV3/INV4/INV5 gelten unverändert. REQ6 (Title +
  noindex in v2/index.html) bleibt.
