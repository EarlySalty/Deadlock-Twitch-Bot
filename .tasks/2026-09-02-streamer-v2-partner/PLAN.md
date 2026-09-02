# Plan: Streamer-Landing v2 Partnerschaft

status: aktiv
datum: 2026-09-02
contract: CONTRACT.md
branch: feat/streamer-v2-partner
worktree: /home/nathanael/.worktrees/tb-streamer-v2-partner

## M1 Hero, Nav, CTAs (REQ-01, 02, 03, 04)

- Texte wortgleich aus dem Contract, Nav auf fünf Einträge, Discord-Knopf sekundär, Stempel der Bühne.
- Validierung: `npm test`, Screenshot 1440x900.

## M2 Reihenfolge und Preise (REQ-05, 06, 07)

- StreamerNetworkPage: Partner, Leere, Ablauf, Sicherheit, Zahlen, Fragen, dann "Optional mehr Tools" (Pillars + Preise), dann Abschluss. ChannelReportSection nicht mehr rendern.
- Preisabschnitt: Hauptkarte kostenlos, Plus und Pro als gedimmte kleine Extras-Karten.
- Validierung: Screenshot Full-Page und 390 px, `npm test`, `npm run build`.

## M3 Test und Abschluss (REQ-08)

- streamerV2.test.mjs ergänzen: Headline-Konstante und Nav-Reihenfolge (Struktur, nicht Wortlaut der Fließtexte), Rot-Gegenprobe.
- Kritiker-Selbstlauf, Commit, Push des Branches.

## Fortschritt

- 2026-09-02 M0 Baseline (Orchestrator): main e84fa692, `npm test` 20 passed, `npm run build` grün (Stand vom Deploy 11f94859).
- 2026-09-02 M1 Hero, Nav, CTAs, Bühnen-Stempel gesetzt (REQ-01 bis REQ-04).
- 2026-09-02 M2 Reihenfolge umgestellt, ChannelReport raus, Preise als Hauptkarte plus gedimmte Extras, Stempel vereinheitlicht (REQ-05 bis REQ-07).
- 2026-09-02 M3 streamerV2.test.mjs um Nav-Reihenfolge, Knopftext-Verbot und Hero-Headline ergänzt; Rot-Gegenproben je pass 0/fail 1, danach 23 passed, `npm run build` grün.
