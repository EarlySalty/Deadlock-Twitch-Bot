# Review: /streamer/v2 Partner-Netz mit Live-Feed

status: erledigt
datum: 2026-09-04
pruefer: adversarialer Reviewer (Opus 4.8), read-only
gegenstand: git diff 2c047aec..HEAD -- website/ (Commits 84e36fb4..a64dc37d)
branch: feat/streamer-v2-partner-merge

## Ausgang: FREIGABE mit Auflagen

Alle REQ und INV erfuellt, Tests gruen (30 pass, 0 fail). Keine Blocker, keine
Korrektheits- oder Scope-Verstoesse mit Wirkung. Die Mangelliste sind vier
Minor-Punkte (Effizienz, Contract-Buchhaltung, Robustheit), die den Merge nicht
aufhalten, aber vor dem spaeteren Umschalten auf /streamer abgearbeitet werden
sollten.

WIRKUNGSPRUEFUNG[WP-1]: 4 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 1/1 geprueft

## REQ-Urteile

- REQ-01 Hero bleibt: erfuellt. `git diff` zeigt keine Aenderung an
  website/src/components/partner-clean/Hero.tsx; Page rendert `<Hero />` als
  erstes in main (StreamerNetworkPage.tsx:23).
- REQ-02 Partner-Block direkt unter Hero: erfuellt. `<PartnerPitch />` steht
  unmittelbar nach `<Hero />` (StreamerNetworkPage.tsx:24). Visuell gefuehrt
  (Glow, Marquee der echten Avatare, NetworkPulse-SVG), keine Nummern, kein
  Kachelraster (PartnerPitch.tsx:83-206). Copy sagt was wir sind und was der
  Bot uebernimmt (PartnerPitch.tsx:98-111).
- REQ-03 Partner-Uebersicht mit Live-Feed: erfuellt. PartnerNetwork zieht alle
  Partner aus useNetworkStreamers, Live zuerst mit Embed (erste 3) bzw.
  Vorschaubild, Offline als Raster (PartnerNetwork.tsx:161-231). Zaehler "N
  Partner" mit echter Zahl aus streamers.length (PartnerNetwork.tsx:180-186).
  Jede Karte Link auf https://twitch.tv/<login>, target=_blank,
  rel=noopener noreferrer (PartnerNetwork.tsx:50-54, 103-106). Ehrlicher
  Leerzustand bei error oder 0 (PartnerNetwork.tsx:197-198, 132-140).
- REQ-04 Nummerierte Ablauf-Karten weg: erfuellt. StreamDay.tsx geloescht
  (name-status D), Import und Verwendung raus (PAGE-DIFF). Sweep findet keine
  01/02/03-Sektionsnummern in partner-clean/.
- REQ-05 Kein SaaS-Vokabular: erfuellt. Eigener Sweep ueber alle gerenderten
  partner-clean-Komponenten und StreamerNetworkPage.tsx findet keine sichtbare
  Copy mit demo-daten, alle funktionen, funktionen im vergleich, pricing,
  tarif, saas, software, produkt, jetzt testen oder Sektionsnummern. Treffer
  auf "feature"/"features" sind ausschliesslich Code-Identifier (FeatureCard,
  const features), keine sichtbare Copy; Features.tsx-Heading ist "Was als
  Partner dazugehoert". "preis" in Security.tsx ist die Redewendung "gibt sie
  nicht preis", keine Preis-/Pricing-Copy. Keine Links auf /twitch/demo oder
  /streamer/vergleich in partner-clean. Haupt-CTA "Jetzt Partner werden" fuehrt
  auf buildTwitchBotAuthUrl() (PartnerPitch.tsx:141, CTA.tsx:27).
- REQ-06 Nanis Stimme: erfuellt. Neue Copy natuerliches Deutsch mit echten
  Umlauten, Du-Form, keine Em-Dashes (grep auf U+2013/U+2014 in allen fuenf
  neuen Dateien negativ), keine Testimonials.
- REQ-07 Ehrlich gegen den Code: erfuellt. Beworbene Faehigkeiten belegt:
  Auto-Raid (Hero/RaidDemo, EVIDENCE), Discord-Live-Ankuendigung
  (tb-monitoring), Scam-/Spam-Schutz (twitch_moderation_settings),
  !clip (rust/crates/.../clip_command_settings.rs, CLIP_COOLDOWN),
  !lurk (rust/crates/tb-dashboard-api/src/handlers/lurk_command_settings.rs),
  Auswertung nach dem Stream (Stats/Session-Metriken). Keine erfundene Zahl:
  Zaehler nutzt streamers.length, keine hart codierten Logins (Test prueft das).

## INV-Urteile

- INV-01 v1 byteidentisch: erfuellt (mit Buchhaltungs-Mangel M2). name-status
  zeigt nur Aenderungen unter website/src/pages/StreamerNetworkPage.tsx,
  website/src/components/partner-clean/, website/src/hooks/,
  website/src/lib/, website/tests/ und .tasks/. App.tsx und
  src/components/sections/ unangetastet; v1 nicht betroffen.
- INV-02 Hero unveraendert: erfuellt. Kein Diff an Hero.tsx.
- INV-03 v2 bleibt noindex: erfuellt. Kein Diff an website/v2/index.html.
- INV-04 Nur API-Daten: erfuellt. useNetworkStreamers ruft nur GET
  /twitch/api/v2/public/network; keine hart codierten Partner, keine erfundenen
  Zahlen (Test doesNotMatch /login:\s*"..."/).
- INV-05 Tests nicht abgeschwaecht: erfuellt. Baseline 26 -> 30 pass, 0 fail;
  Reihenfolge-Test auf neue Komposition angepasst (Teil der Aenderung),
  Vokabular-Test verschaerft (FORBIDDEN_WORDS mit Wortgrenze zusaetzlich).
- INV-06 Kein neuer Endpunkt/Schema: erfuellt. Nutzt bestehende Antwort
  (login, display_name, avatar_url, is_live, viewer_count, ...).

## Mangelliste (alle Minor, nicht mergeblockierend)

1. website/src/components/partner-clean/PartnerPitch.tsx:78 und
   website/src/components/partner-clean/PartnerNetwork.tsx:162 und
   website/src/components/partner-clean/Stats.tsx:20 -- Schwere minor
   (Effizienz/Twin). Drei unabhaengige Fetches auf denselben Endpoint
   /public/network pro Seitenaufruf: PartnerPitch und PartnerNetwork rufen je
   useNetworkStreamers(), Stats ruft useNetworkCount(). Zwillingssuche per grep
   belegt (TWIN-FETCH). Wirkung: dreifache Netzlast und bei Teil-Ausfall
   divergierende Partner-Zahlen zwischen Stats und PartnerNetwork.
   Fix: Fetch einmal auf Seitenebene (StreamerNetworkPage) oder in einem
   gemeinsamen Provider/Cache halten, streamers als Prop durchreichen, den
   Zaehler aus streamers.length ableiten und useNetworkCount auf dieser Seite
   entfernen.
2. website/src/hooks/useNetworkStreamers.ts:1 -- Schwere minor
   (Contract-Scope). Der Hook liegt unter website/src/hooks/, der
   Contract-"Erlaubter Aenderungsbereich" listet website/src/lib/, nicht
   hooks/. Faktisch unkritisch (hooks/ existiert bereits mit useNetworkCount.ts,
   v1 unberuehrt), aber der Scope weicht ab. Fix: Amendment im CONTRACT.md, das
   website/src/hooks/ in den erlaubten Bereich aufnimmt (oder Hook nach lib/
   verschieben).
3. website/src/components/partner-clean/PartnerNetwork.tsx:35 -- Schwere minor
   (Robustheit). LivePreview <img src=previewImageUrl(login)> hat keinen
   onError-Fallback; 404 des Twitch-Vorschaubilds zeigt ein kaputtes Bild.
   Fix: onError analog Avatar (partnerShared.tsx) auf ein Monogramm/schwarze
   Kachel mit LiveBadge zuruecksetzen.
4. website/src/hooks/useNetworkStreamers.ts:82 -- Schwere minor (Sichtbarkeit).
   Der catch verschluckt den Fehler ohne console-Log; Status "error" ist zwar
   nutzersichtbar (ehrlicher Leerzustand), aber die Ursache ist im Feld nicht
   diagnostizierbar. Fix: console.error mit Rohursache vor setStatus("error").

## Tests

`cd website && node --test tests/*.test.mjs`: tests 30, pass 30, fail 0,
duration ~180 ms. Neue M1-Tests sind scharf (pruefen Datei-Existenz,
Reihenfolge, Twitch-Link/Embed/Leerzustand, Verbotswoerter); bei Rueckbau von
PartnerPitch/PartnerNetwork oder Wieder-Anlegen von StreamDay.tsx wuerden sie
rot. Kein bestehender Test geloescht oder abgeschwaecht.
