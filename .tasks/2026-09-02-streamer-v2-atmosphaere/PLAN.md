# Plan: Streamer-Landing v2 Atmosphäre

status: aktiv
datum: 2026-09-02
contract: CONTRACT.md
branch: feat/streamer-v2-atmosphaere
worktree: /home/nathanael/.worktrees/tb-streamer-v2-atmo

Ziel und Anforderungen stehen im Contract. Reihenfolge nach Nutzer-Priorität.

## M0 Baseline

- `cd website && npm ci && npm test && npm run build`; Ergebnis hier eintragen.
- Screenshots vorher: `google-chrome --headless=new --no-sandbox --hide-scrollbars --virtual-time-budget=8000 --window-size=1440,900 --screenshot=/tmp/streamer-shots/v2-vorher.png http://127.0.0.1:4173/streamer/v2/` gegen `npm run preview`.
- Stop-Regel: rote Baseline notieren, nicht reparieren.

## M1 Hero (REQ-01..04, INV-02)

- `NetworkHero.tsx`: zentrierte Kopfzeile (Chip, H1, eine Subline), darunter Bühne in voller Breite (`max-w-[1400px]` wie v1), darunter zwei Knöpfe, darunter Beweiszeile.
- `NetworkRaidDemo.tsx`: Rückfallebene = Clip-Pool aus v1 (`public/clips/*.mp4`, `pfp/*.png`), `<video muted autoplay loop playsinline poster=...>`; bei `prefers-reduced-motion`, `play()`-Fehler oder `error` bleibt das Poster mit Play-Zustand stehen. Zeitachse als Statuszeile unter der Bühne. Stempel "Beispielablauf" bleibt.
- Poster erzeugen: `ffmpeg -ss 1 -i public/clips/<login>.mp4 -frames:v 1 -q:v 3 public/clips/poster/<login>.jpg`.
- Validierung: Screenshot 1440x900 zeigt Bühne über 60 Prozent Breite mit Clip oder Poster; `npm test` grün.
- Stop-Regel: Video liefert 404 unter `/streamer/clips/` im Preview → Pfad prüfen (`import.meta.env.BASE_URL`).

## M2 Live-Partner groß, Offline zurück (REQ-05, REQ-06)

- `NetworkLive.tsx`: Featured-Embeds in `lg:grid-cols-2` (bei 3 Live: erster über volle Breite oder 2+1), Gold-Glow-Rahmen, `.v2-pulse` LIVE-Punkt, Intro auf einen Satz.
- Offline-Partner: gedimmte Avatar-Reihe (`.v2-marquee` existiert) mit Zähler und Knopf "Alle anzeigen" → bestehendes `PartnerGrid` aufklappen.
- Validierung: Screenshot der Sektion; ohne Live-Partner bleibt die Aussage ehrlich (Texte aus `PartnersSection` beibehalten).

## M3 Atmosphäre (REQ-08)

- Neue Komponente `NetworkAmbient.tsx`: zwei driftende Lichtinseln (`.v2-ambient`) fix über der Seite plus Partikel wie v1 `GlowOrb`; `theme-v2.css` Regel `[data-glow-orb] { display:none }` bleibt für v1-Bausteine, v2 nutzt die eigene Komponente.
- Verbindungslinien, Puls, Scroll-Reveal auf allen Abschnitten; ein gemeinsamer `@media (prefers-reduced-motion: reduce)`-Block.
- Validierung: keine Layout-Verschiebung (kein horizontaler Scroll), Lighthouse-Performance nicht messbar schlechter als vorher (optional, nur wenn schnell).

## M4 Feature-Blöcke kürzen (REQ-07, REQ-10)

- `NetworkStory.tsx`: Void auf zwei Karten mit je einem Satz, Plan-Schritte je ein Satz, Leistungskarten: Visual größer, ein Satz, keine `<ul>`.
- `NetworkProof.tsx`, `NetworkOffer.tsx`, `NetworkSecurity.tsx`: Einleitungen ein Satz, Preiskarten höchstens vier Zeilen, Abschluss mit Partner-Avataren.
- `networkPage.ts`: Texte kürzen, nichts hinzufügen.
- Validierung: `git diff` zeigt nur gestrichene oder gekürzte Sätze; kein neues Versprechen.

## M5 Marke (REQ-09)

- `NetworkChrome.tsx`: "Deutsche Deadlock Community" als `GradientText`, darunter oder daneben klein "Streamer-Netzwerk".
- `v2/index.html`: Titel und OG-Titel.
- Test `tests/streamerV2.test.mjs`: Markenname im Nav, Clip-Pool-Dateien und Poster vorhanden.

## M6 Abschluss

- Screenshots nachher (1440x900 und 390x844), Selbstprüfung gegen jedes REQ, `npm test`, `npm run build`, Commit(s) auf dem Branch.
- Review durch frischen Agenten gegen Contract, dann Merge, Build im Live-Checkout, Live-Screenshot.

## Fortschritt

- 2026-09-02 M0 Baseline (Orchestrator): `npm ci` exit 0, `npm test` 17 passed / 0 failed, `npm run build` exit 0 auf origin/main e27f2c16. Keine rote Baseline.
- Screenshots vorher: `/tmp/streamer-shots/v1-fold.png`, `v1-full.png`, `v2-fold.png`, `v2-full.png` (live, 1440 breit).
