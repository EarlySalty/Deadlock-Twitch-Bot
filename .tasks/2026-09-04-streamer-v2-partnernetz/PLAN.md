# Plan: /streamer/v2 Partner-Block und Partner-Übersicht

status: aktiv
datum: 2026-09-04
klasse: mittel
contract: CONTRACT.md (Ziel, REQ, INV dort)
research: RESEARCH.md, EVIDENCE.md

Branch: feat/streamer-v2-partner-merge (bestehender v2-Branch, kein neuer
Branch). Arbeitsverzeichnis: website/. Tests: `node --test tests/*.test.mjs`
(Baseline 26 grün, 0 rot). Build: `npm run build`.

## Zielreihenfolge der Seite

GlowOrb, Navbar, Hero, PartnerPitch (neu, REQ-02), PartnerNetwork (neu,
REQ-03), RaidExplainer, BanFeed, Stats, Features, ClipManager, Community,
Security, CTA, Footer. StreamDay fällt weg (REQ-04).

## M1: Regressionstest zuerst rot

Änderungen: `website/tests/partnerPage.test.mjs`
- Reihenfolge-Test auf die Zielreihenfolge umschreiben (`<PartnerPitch`,
  `<PartnerNetwork` nach `<Hero`, kein `<StreamDay`).
- Eigener Test: `partner-clean/StreamDay.tsx` existiert nicht mehr und wird
  nirgends importiert.
- FORBIDDEN-Liste um die REQ-05-Wörter erweitern (kleingeschrieben, gegen alle
  Dateien in partner-clean plus Page): "dashboard mit demo-daten",
  "alle funktionen", "funktionen im vergleich", "features" (nur als Wort in
  JSX-Text, nicht als Dateiname; die Prüfung liest nur String-Literale und
  JSX-Text, oder die Komponente Features.tsx wird in "Leistungen"-freies
  Naming umbenannt, Entscheidung beim Bauen), "plan", "tarif", "preis",
  "pricing", "tool", "software", "saas", "produkt", "jetzt testen", und
  Sektionsnummern `"01"`, `"02"`, `"03"` als String-Literale.
- Test für PartnerNetwork: Datei enthält `twitch.tv/`, `player.twitch.tv`,
  `target="_blank"`, `rel="noopener`, einen Leerzustand-Text und keine
  hart codierten Logins (Prüfung: keine Array-Literale mit `login:`).
Erwarteter Zustand: mindestens 3 Tests rot, Namen und Fehlermeldung in
PLAN.md unter "Roter Lauf" festhalten.
Validierung: `cd website && node --test tests/*.test.mjs`
Stop-Regel: Ist der neue Test von Anfang an grün, trifft er den Bug nicht:
Test nachschärfen, nicht weiter.

## M2: Hook für die volle Partnerliste

Änderungen: `website/src/hooks/useNetworkStreamers.ts` (neu), `useNetworkCount.ts`
bleibt (oder wird auf den neuen Hook umgestellt, wenn das ohne
Verhaltensänderung geht).
- Typ `NetworkStreamer { login, displayName?, avatarUrl?, isLive, viewers,
  game?, dlStreams30d, avgViewers30d }`, gemappt aus den snake_case-Feldern
  der API (`/twitch/api/v2/public/network`).
- Zustand: `loading | ready | error`, Liste sortiert: live zuerst nach
  Zuschauern absteigend, dann offline nach `deadlock_streams_30d` absteigend,
  dann Name.
Validierung: `npx tsc --noEmit` im website/-Paket grün.
Stop-Regel: fehlt ein Feld in der API-Antwort, nicht erfinden, Option lassen.

## M3: PartnerPitch (REQ-02)

Änderungen: `website/src/components/partner-clean/PartnerPitch.tsx` (neu),
Styles dort, wo partner-clean sie heute hält (Tailwind-Klassen oder
`website/src/styles/`).
- Erste Sektion nach dem Hero. Aufbau im v1-Look: links großer Textblock,
  rechts ein bewegtes Visual (Glow, Puls, Marquee der Partner-Avatare aus M2
  als "Netzwerk-Gefühl", kein Kachelraster, keine Nummern).
- Inhalt in Nanis Stimme (Skill community-ankuendigung, Du-Form, echte
  Umlaute, keine Em-Dashes):
  Chip: "Deutsche Deadlock Community"
  Headline: "Du wirst Partner. Der Bot macht den Rest."
  Absatz 1 (was wir sind): die deutsche Deadlock-Community, ein Netzwerk aus
  Streamern und einem aktiven Discord, kein Anbieter, kein Abo.
  Absatz 2 (was passiert): Sobald du Partner bist, übernimmt der Bot von
  selbst: Raids an den passenden Live-Partner, Live-Ankündigung im Discord,
  Scam- und Spam-Schutz im Chat, Befehle wie !clip und !lurk, Auswertung nach
  dem Stream. Du richtest nichts ein und verwaltest nichts.
  Drei kurze Anker-Zeilen mit Icon (kein Raster, untereinander mit Linie):
  "Du gehst live: das Netzwerk merkt es", "Du streamst: der Bot passt auf",
  "Dein Stream endet: deine Zuschauer bleiben im Netzwerk".
  Knopf: "Jetzt Partner werden" (bestehender Link aus externalLinks.ts).
- Jede Aussage gegen EVIDENCE.md (REQ-07-Fundstellen) prüfen.
Validierung: Build grün, Sektion im Browser sichtbar (Screenshot).
Stop-Regel: kein Text, der ein Feature bewirbt, das in EVIDENCE.md keine
Fundstelle hat.

## M4: PartnerNetwork (REQ-03)

Änderungen: `website/src/components/partner-clean/PartnerNetwork.tsx` (neu),
Helfer `TwitchEmbed`, `twitchParent`, `twitchUrl`, `Avatar` mit
Monogramm-Fallback, `LiveBadge` aus `git show 68194ff0:website/src/components/v2/NetworkLive.tsx`
übernehmen (nur die Helfer, nicht ProtocolSection/NetworkChrome).
- Kopf: Chip "Unsere Partner", Headline "Wer schon dabei ist", Zähler
  "N Partner" aus der Listenlänge, ein Satz Einleitung.
- Live-Bereich: bis zu 3 Live-Partner als große Karten mit Twitch-Embed
  (muted, autoplay, parent = hostname), Gold-Glow, pulsierender LIVE-Punkt,
  Name, Zuschauer, Kategorie. Weitere Live-Partner (ab dem 4.) als
  Karte mit Live-Vorschaubild
  `https://static-cdn.jtvnw.net/previews-ttv/live_user_<login>-640x360.jpg`
  statt Embed. `prefers-reduced-motion`: Embed durch Vorschaubild ersetzen.
- Offline-Bereich: vollständiges Raster (Avatar, Anzeigename, "N Deadlock-
  Streams in 30 Tagen" nur wenn > 0), gedimmt gegenüber Live.
- Jede Karte ist `<a href="https://twitch.tv/<login>" target="_blank"
  rel="noopener noreferrer">`.
- Zustände: loading (Skeleton-Karten), error oder leer: ehrlicher Text
  "Die Partnerliste lädt gerade nicht. Schau auf Twitch oder im Discord
  vorbei." Keine erfundenen Kanäle.
Validierung: Build grün, Tests grün, Screenshot mit echten Live-Partnern
(zur Tageszeit vermutlich 0 bis 3 live; dann zusätzlich Screenshot des
Offline-Rasters).
Stop-Regel: Embeds, die im iframe "parent"-Fehler zeigen, blocken den
Milestone; dann parent-Liste prüfen (localhost für Dev ergänzen).

## M5: Seite umbauen und REQ-05-Sweep

Änderungen: `StreamerNetworkPage.tsx` (Reihenfolge, Imports), `StreamDay.tsx`
löschen, alle partner-clean-Komponenten nach REQ-05-Wörtern durchsuchen und
umformulieren (Nutzersprache), Navbar-Anker anpassen, falls "#ablauf" oder
"Funktionen" dort verlinkt sind (nur im v2-Navbar, v1 bleibt).
Validierung: `node --test tests/*.test.mjs` komplett grün (M1-Tests jetzt
grün), `npm run build` grün, `npx tsc --noEmit` grün.
Stop-Regel: Bleibt ein FORBIDDEN-Treffer, der Text wird umformuliert, nie der
Test angepasst.

## M6: Sichtprüfung und Abschluss

- Dev-Server oder Build-Preview, Screenshots Desktop 1440 und Mobil 390 von
  Hero bis Partner-Übersicht ablegen in `.tasks/2026-09-04-streamer-v2-partnernetz/shots/`.
  Headless Chrome hängt in der Sandbox (bekannt); wenn kein Screenshot
  möglich, das im Bericht sagen, nicht an Flags drehen.
- Commit(s) auf feat/streamer-v2-partner-merge, kein amend, push.
- Status in PLAN.md je Milestone nachtragen.

## Roter Lauf (M1)

`node --test tests/partnerPage.test.mjs`, 2026-09-04: 10 Tests, 4 pass, 6 fail.
Rote Tests mit Fehlermeldung:
- "v2 rendert Partner-Block und Partner-Übersicht direkt unter dem Hero": Baustein fehlt in der Komposition: <PartnerPitch
- "die nummerierten Ablauf-Karten sind weg (StreamDay geloescht)": StreamDay.tsx muss geloescht sein
- "keine Sektionsnummern 01/02/03 in der Partner-Copy": Sektionsnummer als Literal in StreamDay.tsx
- "kein SaaS-Vokabular in der sichtbaren Partner-Copy": verbotenes Muster in der Copy: dashboard mit demo-daten
- "PartnerNetwork zeigt echte Partner mit Twitch-Link und ehrlichem Leerzustand": PartnerNetwork.tsx fehlt
- "PartnerPitch existiert als eigene Sektion": PartnerPitch.tsx fehlt

## Status

- M1: fertig (Test rot, Roter Lauf oben)
- M2: fertig (Hook useNetworkStreamers, tsc grün)
- M3: fertig (PartnerPitch, geteilte Helfer, tsc grün)
- M4: offen
- M5: offen
- M6: offen
