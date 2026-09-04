# Research: /streamer/v2 verkauft die Partnerschaft, Partner-Netz mit Live-Feed

status: erledigt
datum: 2026-09-04
klasse: mittel

## Auftrag

Wer /streamer/v2/ oeffnet, versteht direkt unter dem Hero, dass hier die
Deutsche Deadlock Community ist, dass man Partner wird und der Bot den Kanal
managt; die nummerierten 01/02/03-Karten fliegen raus und eine echte
Partner-Uebersicht mit Live-Feed aus der Netzwerk-API kommt rein.

## Beobachtungen (belegt, Datei:Zeile)

### Seitenaufbau heute

- website/src/pages/StreamerNetworkPage.tsx:22-31 rendert in dieser Reihenfolge:
  Hero, StreamDay, RaidExplainer, BanFeed, Stats, Features, ClipManager,
  Community, Security, CTA (Imports 5-14). Nur 37 Zeilen, reine Komposition.
- Die Sektion "Was im Netzwerk fuer dich laeuft" mit den Karten 01/02/03 ist
  StreamDay.tsx: die drei Phasen stehen in website/src/components/partner-clean/StreamDay.tsx:5-39
  (step "01" L8, "02" L19, "03" L30), Titel L47, section id="ablauf" L43.
  Die beiden Links sind dort: "Dashboard mit Demo-Daten ansehen"
  (StreamDay.tsx:105, href="/twitch/demo" L102) und "Alle Funktionen im
  Vergleich" (StreamDay.tsx:112, href="/streamer/vergleich/" L109). Das ist
  exakt der REQ-04-Block, der raus soll.
- Community.tsx zeigt heute KEINE Partnerliste, sondern generische
  Community-Karten (Leaderboard, Discord-Integration, Rollen-System,
  Live-Benachrichtigungen, website/src/components/partner-clean/Community.tsx:46-69)
  plus ein Discord-Beitritts-Panel (ab L72). Es gibt aktuell auf der ganzen
  Seite keine Komponente, die die Partner aus der Netzwerk-API rendert.

### Netzwerk-API (Partnerdaten)

- Frontend-Endpoint: website/src/hooks/useNetworkCount.ts:3-4 ruft
  GET https://deutsche-deadlock-community.de/twitch/api/v2/public/network ab und
  nutzt heute nur die Laenge der Streamerliste als Zahl (useNetworkCount.ts:25-26).
  Ein Hook, der die volle Liste liefert, existiert nicht mehr.
- Rust-Handler: rust/crates/tb-dashboard-api/src/handlers/network.rs:1 bedient
  GET /twitch/api/v2/public/network. Response ist { streamers: [...] }
  (NetworkResponse, network.rs:70-72).
- Felder pro Streamer (NetworkStreamerJson, network.rs:19-40): login,
  display_name (Option, per Helix angereichert), avatar_url (Option, per Helix),
  is_partner (immer true), is_live (bool), viewer_count (i32), game (Option,
  Twitch-Kategorie), deadlock_streams_30d (i64), avg_viewers_30d (f64).
- Der Endpoint filtert bereits serverseitig auf aktive Partner und liefert
  ALLE Partner, live wie offline (network.rs:16 Kommentar "is_partner ist immer
  true"; is_live steuert nur den Live-Flag, network.rs:56). Profilbild und
  Anzeigename kommen aus dem Helix-Enrichment (apply_cached network.rs:122-132),
  koennen also fehlen (Option). INV-06 ist damit erfuellt: Login, Anzeigename,
  Profilbild und Live-Status liefert die API bereits, kein neuer Endpoint noetig.

### Frueherer Live-Feed (Git-History, geloescht)

- Commit 68194ff0 (Wed Sep 2) "Live-Partner gross, Offline-Partner als
  Avatar-Reihe": aenderte website/src/components/v2/NetworkLive.tsx.
- NetworkLive.tsx enthielt die brauchbaren Bausteine: TwitchEmbed
  (src=https://player.twitch.tv/?channel=<login>&parent=<host>&muted=true&autoplay=true,
  16:9-iframe), twitchParent() (window.location.hostname, Fallback
  deutsche-deadlock-community.de), twitchUrl(login)=https://twitch.tv/<login>,
  Avatar mit Monogramm-Fallback, initials(), LiveBadge (v2-pulse, rot),
  ChannelBar (klickbarer Kanal + Live-Badge + Zuschauer + game).
- ABER NetworkLive.tsx haengt an ProtocolSection aus
  website/src/components/v2/NetworkChrome.tsx und am Hook
  website/src/hooks/useNetworkMetrics.ts (Typ PartnerChannel: login,
  displayName, avatarUrl, isLive, viewers, game, dlStreams30d, avgViewers30d).
- Commit 2c047aec (Fri Sep 4) hat die ganze v2/-Familie geloescht: NetworkLive.tsx
  (436 Z.), NetworkChrome (159), NetworkHero (144), NetworkOffer (305),
  NetworkProof (217), NetworkRaidDemo (720), NetworkStory (201), NetworkSecurity
  (183), NetworkPillarVisuals (194), NetworkAmbient (60), dazu
  hooks/useNetworkMetrics.ts (174), data/networkPage.ts (258), data/partnerPage.ts
  (92) und das partner/-Verzeichnis. Grund laut Commit: Abkehr vom
  Network*/Protocol-Merge, weil das der abgelehnte "Slop"-Look war.

### v1-Seite und wiederverwendbares Embed

- v1 (/streamer) laeuft ueber website/src/App.tsx (eigener Vite-Entry index.html),
  Sektionen in website/src/components/sections/. v1 bleibt unangetastet (INV-01).
- Twitch-Embeds gibt es in v1 in website/src/components/sections/RaidDemo.tsx
  (rd-twitch-embed, Embeds ab L700) und der Parent-Host steht in
  website/src/components/sections/ClipManager.tsx:13
  (TWITCH_PARENTS = ["deutsche-deadlock-community.de"]).
- partner-clean/Hero.tsx bindet bereits sections/RaidDemo.tsx ein (belegt durch
  Test streamerV2.test.mjs:30-33). Die Live-Embed-Mechanik ist im Repo also
  doppelt vorhanden (RaidDemo als Bühne, NetworkLive-Helfer in der History).

### noindex und Route

- noindex steht in website/v2/index.html:6 (<meta name="robots"
  content="noindex, nofollow">), Titel v2/index.html:7. Eigener Vite-Entry
  streamerV2 = v2/index.html (vite.config.ts:31), base '/streamer/'
  (vite.config.ts:22). Entry-Skript website/src/streamer-v2.tsx:7,12 rendert
  StreamerNetworkPage. INV-03 haengt nur an dieser einen Zeile; solange die
  index.html unveraendert bleibt, bleibt v2 noindex.

### CTA / Onboarding-Weg

- "Jetzt Partner werden" (CTA.tsx:31) verlinkt buildTwitchBotAuthUrl()
  (CTA.tsx:27). Die Funktion (website/src/data/externalLinks.ts:31-37) baut
  https://deutsche-deadlock-community.de/twitch/raid/auth?scope_profile=base&source=website_onboarding&ts=<now>
  (Basis-URL externalLinks.ts:20-21). Das ist der bestehende Bewerbungs-/
  Onboarding-Weg, der fuer REQ-05 als Haupt-CTA erhalten bleibt.

### REQ-05-Wortfunde (was raus muss)

- StreamDay.tsx:8/19/30 Nummern 01/02/03; StreamDay.tsx:105 "Dashboard mit
  Demo-Daten ansehen"; StreamDay.tsx:112 "Alle Funktionen im Vergleich".
- Features.tsx:63 export Features, Features.tsx:65 section id="features",
  Import/Render in StreamerNetworkPage.tsx:10,27. "Features" als Sektionsname im
  Code; die sichtbare Copy von Features.tsx nutzt "Features" nicht als Wort,
  aber die id="features" und der Komponentenname sind SaaS-Vokabular im Code.
- Keine Treffer fuer "Plan", "Tarif", "Preis", "Pricing", "Software", "SaaS",
  "Produkt", "Jetzt testen" in partner-clean/ oder der Page (Grep leer).

### Tests

- Zwei Testdateien betreffen v2: website/tests/partnerPage.test.mjs und
  website/tests/streamerV2.test.mjs. Testlauf: `node --test tests/*.test.mjs`
  (package.json "test"), im Verzeichnis website/.
- Baseline gemessen: 26 pass, 0 fail (npm test, 2026-09-04). Keine rote Baseline.
- partnerPage.test.mjs:33-56 prueft die EXAKTE Sektionsreihenfolge (GlowOrb,
  Navbar, Hero, StreamDay, RaidExplainer, BanFeed, Stats, Features, ClipManager,
  Community, Security, CTA, Footer) und partnerPage.test.mjs:17-25 eine
  FORBIDDEN-Wortliste. streamerV2.test.mjs:21-33 prueft, dass Hero die RaidDemo
  einbindet, plus Clip-Pool und Markenname.

### REQ-07: Bot-Faehigkeiten belegt

- Auto-Raid beim Stream-Ende: rust/crates/tb-raid/src/auto_raid_pipeline.rs:441
  (run(AutoRaidRequest)), Trigger-Hook on_stream_offline_raid (tb-monitoring).
- Live-Ankuendigung im Discord: rust/crates/tb-monitoring/src/poller/hooks.rs:99
  (announce_live), Ausloeser on_stream_went_live hooks.rs:331.
- Scam-/Spam-Schutz im Chat: rust/crates/tb-chat/src/conversation_scam.rs:97
  (Verdict/GuardSettings) und rust/crates/tb-chat/src/spam_filter.rs
  (calculate_spam_score), Schalter tb-dashboard-api/handlers/moderation_settings.rs.
- Chat-Befehle !clip / !lurk: rust/crates/tb-chat/src/commands.rs:434 (!clip),
  commands.rs:394 (!lurk); Katalog catalog.rs:101 / :143.
- Stream-Auswertung/Report: rust/crates/tb-analytics/src/post_stream.rs
  (Report-Builder, Trigger in on_stream_offline).

## Hypothesen (unbelegt)

- Der abgelehnte "Slop"-Look steckte nicht im TwitchEmbed/Avatar-Code von
  NetworkLive.tsx, sondern in dessen Rahmen (ProtocolSection/NetworkChrome,
  NetworkOffer, nummerierte Sektionen). Pruefbar durch Diff von NetworkChrome.tsx
  gegen die v1-Section-Struktur; die reinen Embed-Helfer sind stilneutral.
- game=="Deadlock" muss vor der Live-Ausgabe geprueft werden (network.rs:28-30
  Kommentar), sonst wird ein Nicht-Deadlock-Stream faelschlich als Live-Partner
  gezeigt. Zu bestaetigen beim Bau der Partner-Sektion.

## Wahrscheinlich zu aendernde Dateien

- website/src/pages/StreamerNetworkPage.tsx - StreamDay raus, Partner-Block und
  Partner-Uebersicht rein, Reihenfolge Hero, Partner-Block, Partner-Uebersicht,
  Rest.
- website/src/components/partner-clean/StreamDay.tsx - loeschen (REQ-04).
- website/src/components/partner-clean/ - neue Sektion(en): Partner-Block
  (REQ-02) und Partner-Uebersicht mit Live-Feed (REQ-03), Embed-Helfer aus
  68194ff0 uebernommen.
- website/src/hooks/ - neuer Hook, der die volle Streamerliste der Netzwerk-API
  liefert (analog useNetworkCount, aber mit Feldern statt nur Laenge).
- website/tests/partnerPage.test.mjs - Reihenfolge-Erwartung an die neue
  Komposition anpassen, REQ-05-Wortverbote ergaenzen (nicht loeschen).

## Risiken / Seiteneffekte

- partnerPage.test.mjs:33-56 codiert die Ist-Reihenfolge inklusive StreamDay
  hart. REQ-04 (StreamDay weg) und REQ-03 (neue Sektion) aendern diese
  Reihenfolge zwangslaeufig. Der Test muss auf die neue Soll-Reihenfolge
  umgeschrieben werden. Das ist keine Abschwaechung im Sinn von INV-05, sondern
  Anpassung an die neue Spezifikation; er bleibt eine Reihenfolge-Assertion und
  wird mit den REQ-05-Verboten sogar strenger. Klar dokumentieren.
- Der Netzwerk-API-Fetch laeuft im Browser gegen die Live-Domain; im Dev/Build
  ist die Liste leer. Der Leerzustand (REQ-03) muss ehrlich greifen, nicht mit
  erfundenen Kanaelen (INV-04).
- Twitch-Embeds brauchen parent==ausliefernder Host; auf /streamer/v2/ ist das
  deutsche-deadlock-community.de. twitchParent() aus 68194ff0 deckt das ab.

## Offene Fragen

- Keine blockierenden. Contract-Defaults gesetzt: Live-Embed fuer max. 3
  Live-Partner, danach Vorschaubild; Offline-Raster vollstaendig; Reihenfolge
  Hero, Partner-Block, Partner-Uebersicht, Rest.
