# Contract: Streamer-Landing v2 verkauft die Partnerschaft, nicht das Tool

status: aktiv
datum: 2026-09-02
klasse: mittel
repo: Deadlock-Twitch-Bot (website/)

Nach dem Anlegen unveränderlich; nur `status:` und Anhänge unter `## Amendments`.

## Ziel

Unter `/streamer/v2/` bleibt jedes Visual aus dem Umbau vom 2026-09-02 (Clip-Bühne, Live-Embeds, Lichtinseln, Marke), aber jeder sichtbare Satz sagt "du wirst Partner der deutschen Deadlock-Community" statt "du verbindest ein Produkt". Nutzer-Urteil zum Stand 11f94859: "visuell näher an v1, inhaltlich wieder zurück ins SaaS".

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Hero-Texte wortgleich: Badge "Das Partner-Netzwerk der deutschen Deadlock-Community"; Headline "Werde Partner der deutschen Deadlock-Community." (Gold-Gradient auf "Deadlock-Community."); Subline genau: "Der Bot ist nur der Schlüssel. Ab dem Moment bist du Partner, deine Viewer bleiben im Kreislauf."
- REQ-02 CTAs: Primär-Knopf überall (Nav, Hero, Abschluss) heißt "Jetzt Partner werden" und führt weiter auf `buildTwitchBotAuthUrl()`. Sekundär-Knopf im Hero und im Abschluss heißt "Community-Discord beitreten" und führt auf `DISCORD_INVITE_URL` (neuer Tab). "Kostenlos verbinden", "Jetzt kostenlos verbinden" und "Kanal-Report holen" kommen als Knopftext nicht mehr vor.
- REQ-03 Navigation genau in dieser Reihenfolge: Partner · So funktioniert's · Zahlen · Sicherheit · FAQ (Anker `#partner`, `#ablauf`, `#zahlen`, `#sicherheit`, `#einwaende`). "Leistungen" und "Preise" stehen nicht mehr in der Nav.
- REQ-04 Bühne erzählt Partnerschaft: Der Stempel über der Bühne lautet "Wenn einer endet, übernimmt der nächste Partner." statt "Übergabe im Netzwerk · Beispielablauf". Die Ehrlichkeit bleibt über das "CLIP"-Abzeichen auf den Karten und die Unterzeile "Clip aus dem Netzwerk"; ein kleiner Hinweis "Beispiel" darf in der Statuszeile stehen, aber nicht als Überschrift.
- REQ-05 Reihenfolge der Abschnitte: Partner, Der Moment (Problem), So kommst du rein, Sicherheit, Offene Zahlen, Fragen, Abschluss. Die Leistungen (Pillars) und die Preise wandern hinter die Fragen und vor den Abschluss, unter einer gemeinsamen Stempelzeile "Optional mehr Tools". Kanal-Report-Abschnitt entfällt aus der Seite (Komponente bleibt im Code, wird nur nicht mehr gerendert).
- REQ-06 Preise nicht als Pricing-Table: Überschrift des Preisabschnitts "Kostenlos Partner werden." mit einem Satz, dass die Partnerschaft nichts kostet; Plus und Pro erscheinen als zwei kleine, gedimmte Karten "Optionale Extras" unter dem Hauptangebot, nicht gleichrangig daneben. Preise, Leistungsaussagen und Hinweise ("Noch nicht buchbar") bleiben wortgleich, nichts wird ergänzt.
- REQ-07 Abschluss-Text: Überschrift nennt die Partnerschaft (z. B. "Dein nächster Stream endet bei einem Partner."), Primär "Jetzt Partner werden", Sekundär "Community-Discord beitreten", Partner-Avatare bleiben.
- REQ-08 Beweis: `npm test` grün (Baseline 20 passed), `npm run build` grün, Screenshots 1440x900 und 390x844 zeigen Hero mit neuen Texten und Nav mit fünf Einträgen; nach Deploy enthält `/streamer/v2/` den Anker "Werde Partner der deutschen Deadlock-Community".

## Invarianten (darf sich nicht ändern)

- INV-01 Kein Visual wird entfernt oder verkleinert: Clip-Bühne, Schrittleiste, Live-Embeds, Avatar-Marquee, Lichtinseln, Partikel, Marke im Nav bleiben wie in 11f94859.
- INV-02 Keine Live-Behauptung ohne `useNetworkMetrics`; Clip-Karten tragen weiter "CLIP", Kennzahlen kommen nur aus dem Hook.
- INV-03 v1 (`website/index.html`, `src/App.tsx`, `src/components/sections/**`, `src/index.css`) unverändert.
- INV-04 Keine neuen Abhängigkeiten; Tests nicht gelöscht oder abgeschwächt; `tests/anchors.test.mjs` bleibt grün (jeder Nav-Anker existiert).
- INV-05 Keine neuen Verkaufsversprechen; Preis- und Leistungstexte nur umsortiert oder gekürzt.
- INV-06 Nutzersprache, echte Umlaute, keine Em-Dashes, keine Code-Kommentare in neuem Code.

## Nicht-Ziele

- Keine neuen Visuals, kein weiterer Layout-Umbau der Abschnitte 02 bis 07.
- Kein Umbau von v1, Hooks, Backend, Caddy.

## Erlaubter Änderungsbereich

- website/src/components/v2
- website/src/pages/StreamerNetworkPage.tsx
- website/src/streamer-v2.css
- website/src/data/networkPage.ts
- website/v2/index.html
- website/tests/streamerV2.test.mjs
- .tasks/2026-09-02-streamer-v2-partner

## Verbotene Änderungen

- website/index.html
- website/src/App.tsx
- website/src/components/sections
- website/src/components/layout
- website/src/index.css
- website/src/hooks
- website/package.json
- website/vite.config.ts

## Offene Produktfragen

- keine

## Amendments

