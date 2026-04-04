## Viewer Presence Tracking

- 2026-04-04: Aufgabe aufgenommen. Relevante Analytics-/Dashboard-Muster geprüft, fehlende `WORKFLOW.md` angelegt.
- 2026-04-04: Presence-Ticks in Schema und `collect_chatters_data` ergänzt. Neues `api_viewer_timeline.py` mit Session- und Profil-Endpoint eingebaut, `api_v2.py` für Pfad-Streamer/Plan-Gating erweitert.
- 2026-04-04: Dashboard ergänzt: neue Viewer-Timeline-Fetcher/Hooks/Typen, neue `ViewerTimeline.tsx`-Seite, Session-Detail-Tabs erweitert, bestehende Overview-Timeline auf `useViewerCountTimeline` umgestellt.
- 2026-04-04: Verifikation: `python3 -m py_compile ...` erfolgreich, `./node_modules/.bin/tsc -b` erfolgreich, `npm run build` erfolgreich.
