# Contract: Streamer-Landing v2 bekommt die visuelle Kraft von v1 zurück

status: erledigt
datum: 2026-09-02
klasse: mittel
repo: Deadlock-Twitch-Bot (website/)

Dieser Contract ist der Maßstab für Implementierung und Merge-Kritiker. Nach dem
Anlegen ist er unveränderlich: der Hook lässt nur noch die `status:`-Zeile und
Anhänge unter `## Amendments` zu.

## Ziel

Die Vorschau unter `/streamer/v2/` behält Positionierung und Texte von v2, zeigt
das Netzwerk aber wieder so lebendig wie `/streamer/` (v1): großes bewegtes
Hero-Visual mit echten Clips, Live-Partner früh und groß, Offline-Partner
zurückgenommen, kurze Feature-Blöcke mit visuellem Anker, Glow und Bewegung im
ganzen Layout, warme Community-Marke statt kühlem Produktnamen.

Nutzer-Befund (wörtlich, Maßstab für das Review): "v2 erklärt das Netzwerk. v1
hat es gezeigt." Zielbild: Texte und Positionierung von v2, visuelle Sprache und
lebender Charakter von v1, konsequenter und moderner.

## Anforderungen (user-sichtbares Verhalten)

- REQ-01 Hero visuell führend: Die Übergabe-Bühne (zwei Stream-Karten) ist das
  dominante Element im ersten Viewport (Desktop 1440x900): sie liegt unter einer
  kompakten, zentrierten Kopfzeile (Chip, Headline "Kein Stream endet im
  Leeren.", eine Subline mit höchstens 20 Wörtern) und nutzt die volle
  Inhaltsbreite wie in v1, mindestens 60 Prozent der Viewport-Breite. Text im
  Hero: Chip, Headline, eine Subline, zwei Knöpfe, sonst nichts.
- REQ-02 Bühne mit echten Clips: Die beiden Karten spielen die lokalen
  Clip-Videos aus `website/public/clips/*.mp4` mit den Profilbildern aus
  `public/clips/pfp/` (derselbe Pool wie v1, Autoplay stumm, Loop). Sobald die
  Netzwerk-API Partner mit `isLive` und Profilbild liefert, bleibt der bisherige
  Weg über echte Partnerkarten erhalten; der Clip-Pool ist die Rückfallebene
  statt der grauen Platzhalterkarten "dein_kanal / ein_anderer_stream".
- REQ-03 Kein totes Rechteck: Wenn ein Video nicht startet (Autoplay geblockt,
  `prefers-reduced-motion`, Ladefehler), zeigt die Karte ein Standbild (Poster
  aus dem Clip) mit Gold-Glow, LIVE-Abzeichen und sichtbarem Play-Zustand. Nie
  eine leere schwarze Fläche.
- REQ-04 Geschichte sichtbar: Zwischen "dein Stream endet" (linke Karte) und
  "Zuschauer wandern weiter" (rechte Karte) läuft eine sichtbare Verbindung
  (Strahl, Partikel, Zähler, Stempel wie in v1), die die Übergabe ohne
  Lesetext erzählt. Die Zeitachse mit vier Schritten bleibt, aber als
  Statuszeile unter der Bühne, nicht als Textspalte daneben.
- REQ-05 Live-Partner früh und groß: Direkt nach dem Hero kommen die
  Live-Kanäle als große Embeds (höchstens 3, mindestens zwei Spalten breit auf
  Desktop, mit Gold-Glow und pulsierendem LIVE-Punkt). Die Einleitung darüber
  ist höchstens ein Satz.
- REQ-06 Offline-Partner zurückgenommen: Nicht-live Partner erscheinen nicht
  mehr als Kachel-Grid im Sichtbereich, sondern als gedimmte, kleine
  Avatar-Reihe (Marquee oder verdichtete Leiste) mit Zähler "N Partner" und
  einem Knopf "Alle anzeigen", der das vollständige Grid aufklappt. Live-Kanäle
  bleiben immer sichtbar und hervorgehoben.
- REQ-07 Feature-Blöcke gekürzt und visuell: Jeder Abschnitt (Das Problem,
  Ablauf, Leistungen, Kontrolle, Zahlen, Kanal-Report, Preise, Fragen,
  Abschluss) hat höchstens eine Einleitung von einem Satz, und jede
  Leistungskarte hat einen starken visuellen Anker (bestehende
  `NetworkPillarVisuals` größer und animiert) plus höchstens einen Satz
  Beschreibung; die Aufzählungslisten in den Leistungskarten entfallen.
  Überschriften und Kernsätze von v2 bleiben wortgleich, es werden nur Sätze
  gestrichen, keine neuen Aussagen erfunden.
- REQ-08 Bewegung und Atmosphäre: Zwei große, langsam driftende Lichtinseln
  (Gold und Türkis) und schwebende Partikel wie in v1 liegen unter der ganzen
  Seite; Live-Indikatoren pulsieren; die Verbindungslinien der Netzwerk-Visuals
  laufen; Abschnitte erscheinen mit Scroll-Reveal. Alles ist bei
  `prefers-reduced-motion: reduce` still (Endzustand ohne Animation).
- REQ-09 Marke warm: Die Navigation trägt "Deutsche Deadlock Community" als
  Gradient-Text wie v1 statt "Deadlock Netzwerk"; Seitentitel und
  Open-Graph-Titel nennen die Community. Der Chip im Hero lautet "Größtes
  Deadlock-Raid-Netzwerk auf Twitch" oder behält den v2-Text; beides erlaubt.
- REQ-10 Preise und Abschluss entschärft: Preisfläche ohne
  Vergleichstabellen-Charakter (keine Häkchenlisten mit mehr als vier Zeilen,
  kein "Pro"-Feeling), Abschluss mit Community-Ton und Partner-Avataren
  statt reinem Formular. Verkaufsaussagen werden nur gestrichen, nie ergänzt.
- REQ-11 Beweis: Nach dem Deploy zeigt ein Screenshot von
  `https://deutsche-deadlock-community.de/streamer/v2/` bei 1440x900 die Bühne
  mit laufendem Clip (oder Poster) im ersten Viewport; `npm test` und
  `npm run build` in `website/` sind grün.

## Invarianten (darf sich nicht ändern)

- INV-01 `/streamer/` (v1: `website/index.html`, `src/App.tsx`,
  `src/components/sections/**`, `src/index.css` außer neuen Klassen) bleibt
  unverändert; v1-Bausteine dürfen importiert, nicht umgebaut werden.
- INV-02 Ehrlichkeitsregel der Bühne: Der Stempel "Beispielablauf" bleibt an
  der Bühne sichtbar. Clip-Karten zeigen keine Zuschauerzahl als gemessenen
  Wert; ein animierter Zähler ist nur innerhalb der als Beispiel gestempelten
  Bühne erlaubt. Kein Kanal wird als "jetzt live" behauptet, ohne dass
  `useNetworkMetrics` das liefert.
- INV-03 Kennzahlen (Partner, live, Spam-Accounts) kommen weiterhin nur aus
  `useNetworkMetrics`; keine erfundene Zahl, keine hochgerechnete Zahl.
- INV-04 Keine neuen Abhängigkeiten in `website/package.json`.
- INV-05 Bestehende Tests in `website/tests/` werden nicht gelöscht oder
  abgeschwächt; neue Tests sichern REQ-02/03 (Clip-Pool und Poster vorhanden)
  und REQ-09 (Markenname im Nav).
- INV-06 Texte in Nutzersprache, echte Umlaute, keine Em-Dashes, keine
  Code-Kommentare in neuem Code.
- INV-07 Caddy-CSP für `/streamer/*` bleibt ausreichend: nur Same-Origin-Medien
  (`public/clips`), Twitch-Embeds aus der bereits erlaubten frame-src.

## Nicht-Ziele

- Kein Umbau von `/streamer/` (v1), keine Weiterleitung von v1 auf v2.
- Keine Änderung an API, Hooks (`useNetworkMetrics`, `useNetworkCount`),
  Backend oder Caddy.
- Keine neuen Verkaufsversprechen, keine neuen Features in Preistexten.
- Keine Mobile-Neugestaltung über das hinaus, was für die neuen Blöcke nötig
  ist (muss aber bei 390px sauber umbrechen).

## Erlaubter Änderungsbereich

- `website/src/components/v2/**`
- `website/src/pages/StreamerNetworkPage.tsx`
- `website/src/streamer-v2.tsx`, `website/src/streamer-v2.css`
- `website/src/theme-v2.css` (nur Regeln, die auf `[data-theme="v2"]`
  gescoped sind)
- `website/src/data/networkPage.ts` (nur Kürzen von Texten)
- `website/v2/index.html`
- `website/public/clips/poster/*` (neu, aus den mp4 erzeugt)
- `website/tests/*.test.mjs` (nur ergänzen)
- `.tasks/2026-09-02-streamer-v2-atmosphaere/**`

## Verbotene Änderungen

- `website/index.html`, `website/src/App.tsx`, `website/src/components/sections/**`,
  `website/src/components/layout/Navbar.tsx`, `website/src/index.css`
  (Streichen erlaubt, Ändern bestehender Regeln nicht)
- `website/src/hooks/**`, `website/package.json`, `website/vite.config.ts`
- Alles außerhalb von `website/` und `.tasks/`
- Caddyfile, Backend, Datenbank

## Offene Produktfragen

- keine

## Amendments

- 2026-09-02: Erlaubter Änderungsbereich alt -> neu unverändert, nur Formathinweis: diff-policy.py liest je Listenzeile genau einen Pfad ohne Zusatztext, deshalb meldet es die Zeilen mit Backtick-Listen, Klammern und Globs (website/src/streamer-v2.css, website/src/data/networkPage.ts, website/tests/streamerV2.test.mjs, website/public/clips/poster/*.jpg) als P4, obwohl sie oben ausdrücklich erlaubt sind; keine Scope-Erweiterung, Grund: Parser-Format, künftig ein Pfad je Zeile ohne Zusatz, entschieden von Orchestrator

