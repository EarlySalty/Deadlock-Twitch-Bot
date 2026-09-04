# Evidence: /streamer/v2 Partner-Netz mit Live-Feed

status: erledigt
datum: 2026-09-04
contract: CONTRACT.md

## Analoge Implementierungen (wie loest das Repo so etwas schon?)

- website/src/components/v2/NetworkLive.tsx (geloescht, Stand Commit 68194ff0) - TwitchEmbed baut src=https://player.twitch.tv/?channel=<login>&parent=<host>&muted=true&autoplay=true als 16:9-iframe; twitchParent() liefert window.location.hostname mit Fallback deutsche-deadlock-community.de; twitchUrl(login)=https://twitch.tv/<login>; Avatar mit Monogramm-Fallback und initials(); LiveBadge (v2-pulse); ChannelBar (klickbarer Kanal + Live-Badge + Zuschauer + game). Genau die Bausteine fuer REQ-03.
- website/src/components/sections/RaidDemo.tsx:700 - v1-Twitch-Embed-Buehne (rd-twitch-embed), von partner-clean/Hero.tsx bereits wiederverwendet; zeigt, wie Embeds in v1-Stil eingebettet werden.
- website/src/components/sections/ClipManager.tsx:13 - TWITCH_PARENTS = ["deutsche-deadlock-community.de"], Parent-Host fuer Twitch-Embeds.
- website/src/hooks/useNetworkCount.ts:3 - bestehendes Muster fuer den Fetch gegen GET /twitch/api/v2/public/network; liefert heute nur die Laenge, ist die Vorlage fuer einen Hook mit der vollen Streamerliste.

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- rust/crates/tb-dashboard-api/src/handlers/network.rs:19 - NetworkStreamerJson (login, display_name?, avatar_url?, is_partner, is_live, viewer_count, game?, deadlock_streams_30d, avg_viewers_30d), Response { streamers } network.rs:70; liefert alle aktiven Partner live wie offline. Kein neuer Endpoint noetig (INV-06).
- website/src/data/externalLinks.ts:31 - buildTwitchBotAuthUrl() = /twitch/raid/auth?scope_profile=base&source=website_onboarding&ts; Haupt-CTA-Ziel "Jetzt Partner werden".
- website/src/components/partner-clean/CTA.tsx:27 - nutzt buildTwitchBotAuthUrl() bereits als Partner-CTA.
- website/src/components/partner-clean/Hero.tsx (via streamerV2.test.mjs:30-33) - bindet sections/RaidDemo.tsx ein, Hero bleibt (REQ-01, INV-02).

## Relevante Tests (laufen vorher, laufen nachher)

- website/tests/partnerPage.test.mjs:33 - prueft exakte Sektionsreihenfolge (Hero, StreamDay, RaidExplainer, BanFeed, Stats, Features, ClipManager, Community, Security, CTA, Footer); muss auf die neue Reihenfolge angepasst werden.
- website/tests/partnerPage.test.mjs:17 - FORBIDDEN-Wortliste; REQ-05-Verbote hier ergaenzen.
- website/tests/streamerV2.test.mjs:21 - Hero baut auf RaidDemo, Clip-Pool und Markenname; darf nicht brechen.
- Baseline: `node --test tests/*.test.mjs` in website/ ergibt 26 pass, 0 fail (2026-09-04). Keine rote Baseline.

## Oeffentliche Schnittstellen und Vertraege (duerfen nicht brechen)

- GET /twitch/api/v2/public/network (rust/crates/tb-dashboard-api/src/handlers/network.rs:1) - Antwort { streamers: NetworkStreamerJson[] }; Backend bleibt unangetastet (Contract Nicht-Ziele, INV-06).
- website/v2/index.html:6 - <meta name="robots" content="noindex, nofollow">; muss bleiben (INV-03).
- website/vite.config.ts:31 - Vite-Entry streamerV2 = v2/index.html, base '/streamer/' (vite.config.ts:22); Routing nicht aendern (verbotener Bereich).
- website/src/App.tsx (v1) und website/src/components/sections/ - unangetastet (INV-01).

## Aenderungsflaeche (welche Dateien voraussichtlich angefasst werden)

- website/src/pages/StreamerNetworkPage.tsx - StreamDay entfernen, Partner-Block und Partner-Uebersicht einfuegen, Reihenfolge Hero, Partner-Block, Partner-Uebersicht, Rest.
- website/src/components/partner-clean/StreamDay.tsx - loeschen (REQ-04, enthaelt 01/02/03 und die zwei SaaS-Links).
- website/src/components/partner-clean/ - neue Sektion(en) Partner-Block (REQ-02) und Partner-Uebersicht/Live-Feed (REQ-03) mit den Embed-Helfern aus 68194ff0.
- website/src/hooks/ - neuer Hook fuer die volle Streamerliste (Vorlage useNetworkCount.ts).
- website/tests/partnerPage.test.mjs - Reihenfolge-Erwartung neu, REQ-05-Wortverbote ergaenzen.
- website/src/streamer-v2.css / website/src/styles/ - Stil fuer Live-Embed, Glow, Offline-Raster.

## Offene Architekturfrage

- Keine. Contract-Defaults sind gesetzt (max. 3 Live-Embeds, danach Vorschaubild; Offline-Raster vollstaendig; Backend unveraendert; ehrlicher Leerzustand statt erfundener Kanaele).
