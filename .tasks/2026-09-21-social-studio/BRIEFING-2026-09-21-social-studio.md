# Briefing: Social Studio Industrial Gold integrieren

Datum: 2026-09-21
Auftrag: Social-Media-Dashboard fertig integrieren, testen, pushen, nach main mergen, live deployen, eigene Arbeitsbranches aufräumen.

## Ausgangslage

- Übergabepaket `social-studio-industrial-gold.zip` (Vorschau + JSX-Referenz) als Design- und Komponentenreferenz.
- `origin/main` enthielt bei Arbeitsbeginn `1327f414` und damit bereits `29dd9c2d` („feat: Social Media Dashboard neu strukturieren“) und `8a2afe31` („fix: Inter im Dashboard laden“).
- Lokaler Branch `feat/social-studio-redesign-20260921` im Worktree `/home/nathanael/.worktrees/tb-social-studio-redesign` zeigte auf `1327f414` ohne eigenen Commit, trug aber die uncommittete Integrationsarbeit einer vorherigen Session (8 geänderte, 5 neue Dateien plus Task-Logs).
- Separate Fremdbranches `feat/social-media-ui-redesign-20260921` (bereits in main) und `feat/twitch-ddc-brand-20260921` (nicht in main) blieben unangetastet.

## Verbindliche Richtung des Nutzers

- Aufgeräumte Vier-Bereiche-Struktur behalten (Pipeline, Auto-Pilot & Zeitplan, Templates & Layouts, Konten & Einstellungen).
- Vorhandene Marken-Tokens des Dashboards verwenden, kein zweiter globaler Farbsatz, kein Rebranding auf Indigo/Violett.
- Original-Deadlock-D-Logo, Manrope/Sora laden im Produkt.
- Arbeitsname „Social Studio“ bleibt Arbeitsname, kein Produkt-Rename.
- Keine Demo-API oder simulierten Erfolgszustände in Produktion.

## Entscheidungen

1. Die vorhandene, uncommittete Integrationsarbeit wurde übernommen, im Detail reviewt und ergänzt statt neu gebaut.
2. Tote `@font-face`-Deklarationen in `studio.css` (nicht existierende `/brand/fonts/*.woff2`) entfernt; Font-Stacks verweisen direkt auf die im Dashboard-Index geladenen Familien Manrope und Sora.
3. Nur-lesende Browserprüfung gegen das echte Backend über einen lokalen Proxy mit dem vorhandenen Internal-Token; Confirm-Dialoge werden grundsätzlich verworfen, damit keine mutierende Aktion auslösen kann.
