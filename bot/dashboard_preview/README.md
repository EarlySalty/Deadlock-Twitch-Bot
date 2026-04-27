# Local Dashboard Preview

Isolierte localhost-Sandbox fuer Theme- und UX-Iterationen.

## Zweck

- keine Aenderungen am produktiven `dashboard_v2`
- Demo-Daten statt Live-Login
- komplette Twitch-UI-Shell lokal pruefbar

## Start

```bash
cd /home/naniadm/Documents
./start-local-dashboard-preview.sh
```

Danach im Browser:

- `http://localhost:4174/`
- `http://localhost:4174/dashboard`
- `http://localhost:4174/verwaltung`
- `http://localhost:4174/pricing`

## Hinweise

- Analytics-Daten laufen ueber den bestehenden Demo-API-Namespace `/twitch/demo/api/v2`.
- Auth, Billing, Internal-Home und einige nicht-demo-faehige Funktionen werden lokal ueber Preview-Fixtures simuliert.
- Diese App ist absichtlich von der produktiven Twitch-UI getrennt.
- Auf Node 20+ startet ein normaler Vite-Dev-Server.
- Auf Node 18 faellt das Startskript automatisch auf `build + lokalen Python-SPA-Server` zurueck.
