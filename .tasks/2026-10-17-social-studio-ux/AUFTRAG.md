# Social-Media-Studio UX and structure cleanup (/twitch/social-media)

Goal: reorganise the studio frontend (bot/dashboard_v2) into the 6-step flow
(collect clips, posting plan, prepare videos, review or autopost, learn, publish + transparency).

Hard rules
- Left dashboard sidebar (MAIN, TOOLS, ADMIN, HILFE) unchanged: no structure, style or link edits.
- Keep the dark theme, typography, card borders, accent colours.
- No backend logic removed; every existing API call, state and save path stays reachable.
- UI text German with real umlauts, no em dashes; no code comments.

Target structure
- Head: status pills plus an interactive autopilot block (main switch + pause production). Remove the autopilot box at the bottom of Planen.
- Heute: left "Prüfung & Freigabe" (only human-approval items + real warnings; preview, date, title, Freigeben / Änderung wünschen with feedback text; legacy approvals as one banner linking to filtered pool). Right "Läuft gerade" (standard vs AI render) and "Als Nächstes" grouped per clip/term. Bottom strip: zuletzt veröffentlicht, von KI aussortiert (count + reasons), KI-Kosten (+ link to detail evaluation).
- Clips: no floating "Auswertung" button; pool header shows active rule + "Zeitraum & Reihenfolge" + one "Twitch-Clips importieren"; one primary status tab row (Alle, Neu, In Vorbereitung, Wartet auf Freigabe, Geplant, Gepostet, Aussortiert / Fehler, Archiv); source filter merged with search row.
- Planen: 3 numbered cards. 1 Postingplan (one shared week view, per-platform rules + clip distribution behind an accordion, lead time hours). 2 Video-Vorbereitung (Standard template with 3 layout cards + Layout anpassen, or KI with language, texts-only vs full cut, multi-video, experimental layout, external AI consent status; fallback texts/hashtags collapsed). 3 Kontrolle (Vollautomatisch / Freigabe nötig / Veto bis Frist).
- VOD-Archiv: the VOD archive settings card moves here.
- Einstellungen: connections with per-platform "Für Auto-Posting aktiv" toggle (from Zielplattformen), notifications and retention, legal + AI consent.

Acceptance: browser screenshots 390/1440 of every tab, every former form still saves (network check), sidebar DOM/CSS diff empty, npm build/lint/tests green, gate ALLOW, deployed live.
