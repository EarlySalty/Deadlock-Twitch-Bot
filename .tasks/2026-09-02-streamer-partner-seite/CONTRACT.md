# Contract: /streamer als Partner-Seite

status: aktiv
datum: 2026-09-02
klasse: mittel
repo: Deadlock-Twitch-Bot

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu. Wer ein REQ oder INV ändern will, schreibt ein
Amendment mit Begründung; Produkt-, API- oder Datenänderungen entscheidet der User.

## Ziel

`/streamer/` verkauft Partnerschaft in der deutschen Deadlock-Community, nicht den Bot als Produkt.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01: `/streamer/` hat genau sechs Sektionen in dieser Reihenfolge: Hero, Problem, Bedeutung, Partner, Sicherheit, Abschluss.
- REQ-02: Primärer CTA heißt „Jetzt Partner werden“ und führt auf denselben OAuth-Start wie bisher (`buildTwitchBotAuthUrl`). Sekundärer CTA heißt „Community-Discord beitreten“ und führt auf `DISCORD_INVITE_URL`.
- REQ-03: Hero-Headline lautet „Werde Partner der deutschen Deadlock-Community.“ Badge, Subline, Social-Proof und Bühnen-Beschriftung folgen `website/src/data/partnerPage.ts`.
- REQ-04: Hero und Partner-Sektion zeigen echte Partner: Live-Embed (stumm) wenn jemand Deadlock streamt, sonst Clips aus `website/public/clips/`. Keine leere Play-Fläche, keine erfundenen Zuschauerzahlen.
- REQ-05: Die Partner-Sektion listet alle Partner klickbar nach Twitch, Live hervorgehoben, Offline zurückgenommen. Überschrift: „Hinter deinem Kanal stehen jetzt andere Kanäle.“
- REQ-06: Title und Meta der kanonischen Seite entsprechen den SEO-Feldern in `partnerPage.ts`. Keine Feature-Liste, keine Pricing-Tabelle, kein CTA „Kostenlos verbinden“.
- REQ-07: Sichtbare Texte der sechs Sektionen enthalten die verbotenen Verkaufswörter aus `PARTNER_FORBIDDEN` nicht.

## Invarianten (darf sich nicht ändern)

- INV-01: Partnerdaten kommen nur aus `/twitch/api/v2/public/network` über `useNetworkMetrics`.
- INV-02: OAuth-Start bleibt `TWITCH_BOT_AUTH_START_URL` mit `scope_profile=base`.
- INV-03: Discord-Invite bleibt `DISCORD_INVITE_URL`. Sicherheitslink bleibt `/twitch/sicherheit`.
- INV-04: Bestehende Unterseiten (`/streamer/v2/`, FAQ, Onboarding, Vergleich, Affiliate) bleiben erreichbar.
- INV-05: Bestehende Tests dürfen nicht gelöscht oder abgeschwächt werden.

## Nicht-Ziele

- Pricing-Seite, Dashboard, Onboarding-Flow und Bot-FAQ umbauen.
- Neue Marke „DeadlockNetzwerk“.
- Caddy-Routing ändern.
- v1-RaidDemo mit Schritt-Pills und erfundenen Viewerzahlen übernehmen.

## Erlaubter Änderungsbereich

- `website/src/App.tsx`
- `website/index.html`
- `website/src/pages/StreamerNetworkPage.tsx`
- `website/v2/index.html`
- `website/src/data/partnerPage.ts`
- `website/src/components/partner/`
- `website/src/components/layout/Navbar.tsx` nur falls die Landing sie weiter nutzt
- `website/tests/*.test.mjs`
- `.tasks/2026-09-02-streamer-partner-seite/`

## Verbotene Änderungen

- Rust-Backend, OAuth, Netzwerk-API, Infisical.
- `website/src/data/externalLinks.ts` Ziel-URLs.
- Lint-Config, Vite-Entries außer Text in bestehenden HTMLs.
- `bot/dashboard_v2/`.

## Offene Produktfragen

- keine

## Amendments
