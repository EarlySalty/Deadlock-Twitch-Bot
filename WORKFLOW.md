## Viewer Presence Tracking

- 2026-04-04: Aufgabe aufgenommen. Relevante Analytics-/Dashboard-Muster geprüft, fehlende `WORKFLOW.md` angelegt.
- 2026-04-04: Presence-Ticks in Schema und `collect_chatters_data` ergänzt. Neues `api_viewer_timeline.py` mit Session- und Profil-Endpoint eingebaut, `api_v2.py` für Pfad-Streamer/Plan-Gating erweitert.
- 2026-04-04: Dashboard ergänzt: neue Viewer-Timeline-Fetcher/Hooks/Typen, neue `ViewerTimeline.tsx`-Seite, Session-Detail-Tabs erweitert, bestehende Overview-Timeline auf `useViewerCountTimeline` umgestellt.
- 2026-04-04: Verifikation: `python3 -m py_compile ...` erfolgreich, `./node_modules/.bin/tsc -b` erfolgreich, `npm run build` erfolgreich.

## Dashboard Polish · Landing-Redesign + Spotlight-Onboarding

- Ziel: Landing-Page `/twitch/dashboard` auf Command-Center-Layout (Sidebar + Main) umbauen, `WelcomeTour` von zentralem Modal auf nicht-blockierende Spotlight-Tour für die Landing-Page umstellen.
- Stand 2026-04-16: Visuelles Brainstorming abgeschlossen. User-Wahl:
  - Landing-Layout: **C · Command Center** (Sidebar 200px links, Main-Area rechts; Profil + Nav-Sections "Main"/"Tools"; Topbar; 3-col Row mit Health + Stream; 4-col Week-KPIs; 2-col Updates/Aktionen).
  - Onboarding: **B · Spotlight-Tour** (Dashboard bleibt sichtbar, einzelne Bereiche per Ring + Popover, Skip/Weiter). Nur für Landing, nicht für Analyse-Dashboard.
- Entscheidungen:
  - Farbpalette/Tokens in `src/index.css` bleiben. Manrope/Sora bleibt.
  - Onboarding-Persistenz weiter über `localStorage` (Key `welcome-tour-dismissed`).
  - Admin-Streamer-Switch bleibt oben rechts erreichbar (in Topbar oder Sidebar-Footer).
- Betroffene Dateien:
  - `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx` — Layout-Refactor auf Sidebar+Main.
  - `bot/dashboard_v2/src/components/onboarding/WelcomeTour.tsx` — von Modal zu Spotlight-Komponente.
  - ggf. neue Tour-Steps auf echte Landing-Bereiche verlinken via `data-tour-id`.
- 2026-04-16: Implementierung an parallele GPT-Worker delegiert (Landing + Spotlight-Tour). User: "Bau das erstmal so ein, Änderungen machen wir später."
- 2026-04-16: Worker 1: `bot/dashboard_v2/src/pages/InternalHomeLanding.tsx` auf Command-Center-Layout umgebaut (Sidebar/Main, mobile Nav-Row, Tour-Anker `tour-nav|health|stream|week`, bestehende Queries/Action-Log/Changelog beibehalten).
- 2026-04-16: Worker 2 hat `WelcomeTour.tsx` auf Spotlight-Overlay mit `data-tour-id`-Ankern, animiertem Ring, Popover-Navigation, Resize/Scroll-Repositioning und LocalStorage-Persistenz umgestellt.
- 2026-04-16: Verifikation Worker 2: gezielter TypeScript-Check für `src/components/onboarding/WelcomeTour.tsx` erfolgreich. Gesamt-`./node_modules/.bin/tsc -b` und `npm run build` derzeit durch fremden Typfehler in `src/pages/InternalHomeLanding.tsx:683` blockiert (`item.active` fehlt auf Teilen des Union-Typs).
- 2026-04-16: Abschluss Worker 1: `src/pages/InternalHomeLanding.tsx` Typfehler bereinigt; `./node_modules/.bin/tsc -b` und `npm run build` im `bot/dashboard_v2` erfolgreich. Build lief mit bestehender Node/Vite-Hinweismeldung (Node 18.19.1 vs. empfohlen 20.19+), aber ohne Build-Abbruch.
