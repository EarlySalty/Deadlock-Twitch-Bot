# WORKFLOW

## 2026-05-25

- Schritt 1 "IA + Shell" gestartet.
- Kontext gelesen: Plan-Dokument, `Sidebar.tsx`, `App.tsx`, `AdminShell.tsx`, `TopBar.tsx`, `package.json`, Shared-Komponenten.
- Neue Layout-Bausteine angelegt: `PageHeader`, `Section`, `StickyActionBar`.
- Sidebar auf fünf Gruppen mit Collapse-Verhalten, neuen Zielpfaden und Legacy-Link-Card unten umgestellt.
- Router auf `/operations`, `/community`, `/content`, `/money` umgebaut; Legacy-Redirects und generische Placeholder-Seite ergänzt.
- Lokale Verifikation abgeschlossen: `npm install`, `npx tsc --noEmit`, `npm run build` erfolgreich. `pnpm` war auf dem System nicht vorhanden, daher npm-Fallback genutzt.
- Schritt 2 "Cockpit-Home neu" gestartet.
- API-/Backend-Payloads für Cockpit geprüft: `internal-home` liefert aktuell keine dedizierte Live-Now-, Pending-Subscriptions- oder Recent-Activity-Struktur; Dashboard wird daher mit dokumentierten Fallbacks auf Admin-Streamer-, Scope-, EventSub- und Health-Daten aufgebaut.
- `src/pages/Dashboard.tsx` komplett als neues Cockpit umgesetzt: `PageHeader`, Status-Streifen, Live-Now-Tabelle, Pending-Actions-Panel und Recent-Activity-Feed mit Fallbacks auf vorhandene Admin-APIs.
- `src/components/layout/TopBar.tsx` um globale Streamer-Quick-Search mit `fetchAdminStreamers('all')`, Keyboard-Navigation und Direktlink auf `/community/streamers/:login` erweitert.
- `src/components/shared/StatusBadge.tsx` um `ok`-Variante ergänzt, damit die Cockpit-Statuspillen konsistent `ok/warning/error` anzeigen.
- Lokale Verifikation fuer Schritt 2 abgeschlossen: `npm install`, `npx tsc --noEmit`, `npm run build` erfolgreich.
- Schritt 3 "Operations" gestartet.
- Neue Routes fuer `operations/scopes` und `operations/bot` umgesetzt; Placeholder in `App.tsx` ersetzt.
- `src/pages/operations/Scopes.tsx` als read-only OAuth-Scope-Diff-Seite mit KPI-Row, Required/Critical-Scopes und filterbarer Streamer-Tabelle umgesetzt.
- `src/pages/operations/BotControl.tsx` als Operations-Seite fuer Reload, globalen Promo-Mode und read-only Live-Announcement-Defaults umgesetzt.
- Frontend-Reload-Pfad fuer `/twitch/reload` minimal ueber `src/api/client.ts` plus `useReloadBot()` in `src/hooks/useAdmin.ts` ergaenzt; keine Backend-Aenderungen.
- Lokale Verifikation fuer Schritt 3 abgeschlossen: `npm install`, `npx tsc --noEmit`, `npm run build` erfolgreich.
- Schritt 4 "Community" gestartet.
- Neue Pages `src/pages/community/Engagement.tsx`, `ChatActions.tsx` und `RaidsActivity.tsx` angelegt; Community-Placeholder in `src/App.tsx` ersetzt.
- Engagement-Page mit lazy geladenen Channel-Settings via lokaler Queue (max. 8 parallel), Toggle-Confirm und read-only Defaults umgesetzt.
- Chat-Actions-Page mit Streamer-Live-Suche, Confirm-Submit und lokalem Session-Verlauf umgesetzt; Backend-Limit fuer Nachrichtenlaenge wird im UI respektiert.
- Raids-Page als read-only Config-/Activity-Ansicht mit Multi-Key-Fallbacks auf `ConfigOverview.raids.raw` umgesetzt.
- Lokale Verifikation fuer Schritt 4 abgeschlossen: `npm install`, `npx tsc --noEmit`, `npm run build` erfolgreich; ein Build-Typfehler in `Engagement.tsx` (`Map.get()` -> `undefined`) wurde direkt behoben.
- Schritt 7 "Visual Polish" gestartet.
- Shared-UI erweitert: `EmptyState` neu angelegt, `StatusBadge` um fehlende Varianten ergaenzt, `DataTable` um `density` plus optionalen `emptyState`-Slot erweitert.
- Streamer-, Monitoring-, Config-, Billing- und Community-Pages auf `PageHeader`, konsistente Empty-States und gemeinsame Hover-/Button-Patterns umgestellt; `StreamerList` und `Scopes` erhielten Dichte-Toggle, `StreamerList` zusaetzlich entfernbare Filter-Chips.
- Zwischenstand verifiziert: `npx tsc --noEmit` erfolgreich.
