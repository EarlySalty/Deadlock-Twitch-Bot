# admin_dashboard/ (Admin-Frontend) — Architektur & Funktionsreferenz

> Pfad: `bot/admin_dashboard/` · Stand: 2026-06-08 · 47 src-Dateien, ~10.600 Zeilen (React/TypeScript/Vite)
>
> Teil der [Architektur-Doku](README.md). Backend: [dashboard.md](dashboard.md) (Admin-Auth), [analytics.md](analytics.md) (`api_admin`, `_serve_admin_dashboard`). Produktdoku: [ADMIN.md](../ADMIN.md), [internal/admin-panel.md](../internal/admin-panel.md).

## 1. Zweck & Abgrenzung

`admin_dashboard/` ist die **Admin-SPA** unter `/twitch/admin` — die Oberfläche für Betreiber: Streamer verwalten/verifizieren, Billing/Affiliate/Gutschriften, Community (Chat-Actions, Engagement, Raids), Bot-/Chat-/Raid-Konfiguration, Inhalte (Announcements, Changelog, Legal, Roadmap), Monitoring (System, EventSub, DB-Query, Error-Logs) und Operations (Bot-Control, Scopes).

Abgrenzung: Im Gegensatz zum [Streamer-Dashboard](frontend-streamer-dashboard.md) nutzt das Admin-Frontend **React-Router** (echte Routen) und spricht die **Admin-Endpunkte** (`api_admin`). Auth = Discord-Gilden-Mitgliedschaft (siehe [dashboard.md](dashboard.md)).

## 2. Einordnung & Abhängigkeiten

| Aspekt | Detail |
|--------|--------|
| **Stack** | React + TypeScript, Vite, **react-router-dom** (Routing), **@tanstack/react-query**, recharts, framer-motion, tailwind, lucide-react. |
| **Build** | Vite (`dev`/`build`/`preview`); Dist von `analytics/api_overview` (`_serve_admin_dashboard`) ausgeliefert. |
| **API** | Admin-Endpunkte (`api_admin`) + interne Aktionen; Auth über Discord-Admin-Session. |

## 3. Struktur im Überblick

| Verzeichnis | Inhalt |
|-------------|--------|
| `(root)` | `App.tsx` (Router), `main.tsx`. |
| `api/` | `client.ts` (1190 Z., Admin-API-Client) + `types.ts` (Admin-Typen). |
| `hooks/` | `useAdmin.ts` (Daten-Hooks), `useAuth.ts` (Admin-Auth-Gate). |
| `components/layout/` | `AdminShell`, `Sidebar`, `TopBar`, `PageHeader`, `Section`, `StickyActionBar`. |
| `components/shared/` | `DataTable`, `KpiCard`, `ConfirmDialog`, `Toast`, `StatusBadge`, `SearchInput`, `EmptyState`, `TextPreview`. |
| `pages/<bereich>/` | Seiten je Bereich (streamers, billing, community, config, content, monitoring, operations, money). |
| `utils/` | `formatters.ts`. |

## 4. Datenfluss / Lebenszyklus

1. **Boot:** `main.tsx` mountet `App` (React-Router). `useRequireAdminAuth` (`hooks/useAuth.ts`) prüft den Admin-Status (`fetchAuthStatus`); ohne gültige Discord-Admin-Session → Login-Redirect (`buildDiscordAdminLoginUrl`).
2. **Shell:** `AdminShell` rahmt mit `Sidebar` (Bereichs-Navigation) + `TopBar`. Routen führen auf die `pages/<bereich>/*`-Komponenten.
3. **Daten:** Eine Page nutzt `hooks/useAdmin.ts` (react-query) → `api/client.ts` → Admin-Endpunkte. Tabellen über das geteilte `DataTable`; Aktionen über `ConfirmDialog` + `Toast`.
4. **Aktionen:** Schreibende Admin-Aktionen (Streamer verifizieren, Plan setzen, Chat-Action senden, Bot steuern) gehen über `api/client.ts` an die Admin-/internen Endpunkte (teils mit CSRF, siehe [analytics.md](analytics.md) `_admin_verify_csrf`).

## 5. Referenz (Bereiche & Schlüsseldateien)

### api/ + hooks/
- `api/client.ts` — der Admin-API-Client: `fetchAuthStatus`, `fetchDashboardOverview`, `buildDiscordAdminLoginUrl`, `buildRaidAuthUrl`/`buildRaidRequirementsUrl`, plus die Aktions-Calls; `ApiError`. `api/types.ts` — `AdminUserInfo`, `AdminAuthStatus`, `StreamerView`, `StreamerPartnerStatus`, `AdminConfigScope`, `LegacyVerifyMode`.
- `hooks/useAdmin.ts` — `useDashboardOverview`, `useStreamers`, `useStreamerDetail`, `useSystemHealth`, `useScopeStatus`, `useEventSubStatus`. `hooks/useAuth.ts` — `useAuth`, `useRequireAdminAuth`, `toAuthErrorMessage`.

### components/
- `layout/` — `AdminShell` (Rahmen), `Sidebar` (Navigation), `TopBar`, `PageHeader`, `Section`, `StickyActionBar`.
- `shared/` — `DataTable` (+ `TableColumn`), `KpiCard`, `ConfirmDialog`, `Toast`, `StatusBadge`, `SearchInput`, `EmptyState`, `TextPreview`.

### pages/ (nach Bereich)
- `streamers/` — `StreamerList`, `StreamerDetail` (712 Z., das zentrale Streamer-Verwaltungs-Panel).
- `billing/` — `Affiliates`, `AffiliateDetailPanel`, `Gutschriften`, `Subscriptions`.
- `community/` — `ChatActionsPage`, `EngagementPage`, `RaidsActivityPage`, `MarketSharePage` (`/community/market`, Markt-Dominanz: Viewer-Anteil des Partner-Netzwerks an der Deadlock-Kategorie via `GET /twitch/api/v2/market-share`, Scope-Toggle deutschsprachig/global + Zeitraum 1–365 Tage; DE-Markt = Deutsch-Tag oder Partner, Alt-Daten vor 10.06.2026 zählen im DE-Scope ungefiltert).
- `config/` — `BotConfig`, `ChatConfig`, `RaidConfig`.
- `content/` — `AnnouncementsPage`, `ChangelogPage`, `LegalPage`, `RoadmapPage`.
- `monitoring/` — `SystemOverview`, `EventSubStatusPage`, `DatabaseQueryPage` (read-only DB-Konsole), `DatabaseStats`, `ErrorLogs`.
- `operations/` — `BotControlPage`, `ScopesPage`. `money/` — `AuditLogPage`. `Dashboard.tsx` — Admin-Startseite. `_placeholder/Placeholder.tsx` — Platzhalter für geplante Bereiche.

### utils/
- `formatters.ts` — `formatNumber`, `formatCurrency[Euro]`, `formatPercent`, `formatBytes`, `formatDuration`.

## 6. Datenbank & externe Schnittstellen

- **API:** Admin-Endpunkte (`api_admin`) + interne Aktionen; Auth über Discord-Admin-Session-Cookie.
- **Assets:** ausgeliefert vom Python-Backend (`_serve_admin_dashboard`).

## 7. Stolperfallen / Besonderheiten

- **Admin-Auth = Discord-Gilde:** Ohne gültige Discord-Admin-Session (Gilden-Mitgliedschaft) gibt es kein Daten-Rendering — `useRequireAdminAuth` redirectet. Lokal greift der Loopback-Bypass (siehe [dashboard.md](dashboard.md)).
- **React-Router hier, Tabs im Streamer-Dashboard:** Die beiden Frontends unterscheiden sich im Navigationsmodell — nicht verwechseln.
- **DB-Query-Seite ist read-only:** `DatabaseQueryPage` spricht den read-only-Query-Runner des Backends (`_run_admin_readonly_query`) — kein Schreibpfad.
- **Schreib-Aktionen brauchen ggf. CSRF:** Manche Admin-POSTs verlangen ein CSRF-Token (Backend `_admin_verify_csrf`); der Client muss es mitsenden.
- **Großer Client (`api/client.ts`, 1190 Z.):** Eine zentrale Stelle für alle Admin-Calls — Änderungen an Admin-Endpunkten zuerst hier nachziehen.
