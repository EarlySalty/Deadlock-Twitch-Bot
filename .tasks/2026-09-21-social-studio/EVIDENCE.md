# Evidence: Social Studio Integration

## Merge-Protokoll (Stand: Fixer-Runde nach externem BLOCK)

- MERGEPROTOKOLL[MS-1]: Commits `83756fb5`, `3c02356d`, `2c4f4c46` auf `origin/feat/social-studio-redesign-20260921`.
- Gate-Läufe: glm-5.3-flash ohne Urteil (900 s), claude deaktiviert, Codex im Credit-Limit, dann glm-5.3-flash mit 1110 s: BLOCK (Schlüssel-Kollision „Freigeben"), behoben → **ALLOW**.
- Push nach main blockiert vom Scheduler-Review-Gate wie vorgesehen; SHA-gebundener Auftrag in `review-state/1630a6572469bdd2.json`.
- Externer Review (Scheduler Queue B, Lease 00:34): **BLOCK** mit zwei P1-Befunden an `PostingPlanDraft.tsx`:
  1. Speicher-Differenz gegen den wandernden Serverstand statt der Basis → fremde Änderungen an unberührten Feldern wurden still zurückschreibbar. Behoben: Unterschied einmalig gegen die feste Baseline, nur geänderte Felder werden gesendet.
  2. Modus starr zuletzt → Abschalten der Vollautomatik blieb bis zum Schluss wirksamlos und überstand Teilfehler. Behoben: einschränkende Moduswechsel (full_auto→veto_window→manual) werden zuerst geschrieben, Vollautomatik weiterhin zuletzt.
- Neue Prüfgruppen (13/13 grün): fremde Änderung zwischen Laden und Schreiben bleibt erhalten; full_auto→manual wird trotz Teilfehler sofort wirksam und vor den Zielen gesendet.
- Tests danach: fokussiert 58/58, Browser 13/13, vollständige Suite 384/389 (identische fünf Basisfehler), Build und ESLint sauber.
- Fixer-Head `2c4f4c46` gepusht; `complete-fix` (Lease + neue Head-SHA) ist Queue-Sache der Hauptsession, danach Rereview durch den externen Scheduler; bei `release/clean` übernimmt der Wache-Tick Push → Deploy → Live-Nachweis → Aufräumen.

## Baseline (sauberes main, `1327f414`, Worktree `tb-social-studio-baseline`)

`node --import tsx --test tests/brandPalette.test.ts tests/dashboardProfileCache.test.ts tests/uplinkEmpfehlung.test.ts`:
5 Fehler, identisch mit den im eigenen Lauf gemeldeten (Palette-Tests in Verwaltungs- und Admin-Dateien, Sidebar-Skeleton, OBS-Hilfe). Vom Auftrag nicht berührte Dateien; keine Nachbesserung im Scope dieses Branches.

## Fokussierte Suite nach Änderung

`socialStudioRedesign, socialMediaContract, socialMediaLayout, zeitplanFormular, dashboardShell, i18n`: 58/58 bestanden.

## Produktbuild

`npm run build` (tsc -b && vite build): erfolgreich, Exit 0.

## Vollständige Suite nach Änderung

`npm test`: 384/389 bestanden; die 5 Fehler sind deckungsgleich mit der Baseline auf sauberem main. Keine Regression.

## Browserprüfung am integrierten Frontend (29/29 bestanden)

Gegen den Produktions-build aus diesem Worktree und die echte Dashboard-API (127.0.0.1:8769) über lokalen Proxy mit Internal-Token aus dem Prozessumfeld des laufenden Dienstes; Token nie ausgegeben. Confirm-Dialoge wurden grundsätzlich verworfen, mutierende Aktionen nicht ausgelöst.

- Studio-Shell, Versionanker `industrial-gold-live-v1`, vier Bereichs-Tabs.
- D-Logo geladen (Original-Pfad `brand/deadlock-d-logo.png`).
- Studio Manrope und Studio Sora aus dem zentralen Marken-Paket `/brand/fonts/` tatsächlich geladen (Caddy liefert `/brand/*` aus `Website/dl-brand`); Headline-Stack `Studio Sora, Sora, Manrope, Inter`.
- Vier Kennzahlen, echte Daten: 36 Clips im Bestand, 24 Karten Seite 1, Bestandsanzeige „36 von 36 Clips".
- Statusfilter, Aktionsmenü offen/schließt mit Escape, Fokus zurück auf dem Auslöser.
- Auswertung als Dialog, Escape, Fokus-Rückkehr.
- Zeitplan: Formular mit echtem Stand, Speichern ohne Änderung gesperrt, Entwurfszustand, Kanal-Zeitzone Europe/Berlin.
- Templates: drei Vorlagen, Layout-Editor im Dialog.
- Konten: echte Verbindungszeilen (YouTube abgelaufen mit Warnung, TikTok/Instagram nicht verbunden).
- Kein Horizontalüberlauf bei 390 und 320 Pixel (Überhang 0 px).
- Keine Konsolenfehler, keine Seitenfehler.

Screenshots: `~/.claude/sichtpruefung/social-studio-gold/` (desktop-pipeline, desktop-zeitplan, desktop-konten, mobil-390-pipeline, mobil-320-pipeline, ergebnisse.json).

## EIGENES Deploy-Ergebnis (nach Merge eingetragen)

SIEHE EVIDENCE unten Abschnitt „Deploy".
