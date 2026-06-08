# dashboard_v2/ (Streamer-Dashboard-Frontend) — Architektur & Funktionsreferenz

> Pfad: `bot/dashboard_v2/` (+ `bot/dashboard_preview/`) · Stand: 2026-06-08 · 108 src-Dateien, ~29.800 Zeilen (React/TypeScript/Vite)
>
> Teil der [Architektur-Doku](README.md). Backend: [analytics.md](analytics.md) (liefert `/twitch/api/v2/*` + serviert die Assets), [dashboard.md](dashboard.md) (Auth/Billing). Verwandt: [frontend-admin-dashboard.md](frontend-admin-dashboard.md), [frontend-website.md](frontend-website.md).

## 1. Zweck & Abgrenzung

`dashboard_v2/` ist die **Streamer-Analytics-SPA** unter `/analyse` — die React-Oberfläche, die die Analytics-v2-API visualisiert (Overview, Audience, Growth, Coaching, AI-Analyse, Social-Media, Title-Generator, Sessions, Monetization …). Sie ist **tab-basiert** (kein React-Router) und kann im **Live-** oder **Demo-Modus** laufen (öffentliches Demo-Dashboard).

Abgrenzung: Die Daten kommen aus [analytics.md](analytics.md); das Frontend rendert nur. `dashboard_preview/` ist die lokale Vorschau-Variante (eigene Build-Skripte + Fixtures, kein Live-Backend).

## 2. Einordnung & Abhängigkeiten

| Aspekt | Detail |
|--------|--------|
| **Stack** | React 18 + TypeScript, Vite, **@tanstack/react-query** (Data-Fetching/Cache), **recharts** (Charts), **framer-motion** (Animation), **tailwindcss**, lucide-react (Icons). |
| **Build** | Vite; Skripte `dev`/`build`/`preview` (live) + `dev:preview`/`build:preview` (Demo/Preview); `test` + `fuzz:protocol` (fast-check). Das Dist wird von [analytics.md](analytics.md) (`_serve_dashboard_v2`) ausgeliefert. |
| **API** | `/twitch/api/v2/*` (Cookie-Session), siehe [API.md](../API.md). |
| **Auth/Plan** | Plan-/Scope-Gating über `PlanContext` + `useScopes`. |

## 3. Struktur im Überblick

| Verzeichnis | Inhalt |
|-------------|--------|
| `(root)` | `App.tsx` (Tab-Shell), `main.tsx`, `runtimeConfig.ts` (Live/Demo-Umschaltung), `tabAliases.ts`. |
| `api/` | typisierte Fetch-Module je Thema (analytics, ai, admin, affiliate, auth, billing, engagement, home, socialMedia, title) + `core.ts` (Transport). |
| `hooks/` | `useAnalytics.ts` (react-query-Hooks), `useScopes.tsx` (Scope-Context). |
| `context/` | `PlanContext.tsx` (`PlanProvider`/`usePlan`). |
| `components/` | `cards/`, `charts/`, `heatmaps/`, `layout/`, `modals/`, `banners/`, `onboarding/` (Touren), `pricing/`, `socialmedia/`, `scopes/`, `roadmap/`, `verwaltung/`. |
| `pages/` | die Tabs (Overview, Audience, Coaching, AIAnalysis, SocialMedia, …) + Chat-Analytics-Subpages. |
| `types/` | TS-Typen (analytics 1541 Z., billing, scopes, socialMedia). |
| `preview/` | Demo-Fixtures + Preview-Routen (`dashboard_preview`). |
| `utils/` | Formatter, Engagement-KPI-Helfer. |

## 4. Datenfluss / Lebenszyklus

1. **Boot:** `main.tsx` mountet `App`. `runtimeConfig.ts` bestimmt über `isDemoDashboardPath`/`resolveEffectiveDemoMode`, ob `LIVE_API_BASE` oder `DEMO_API_BASE` genutzt wird; der Server injiziert die Runtime-Config (`_inject_dashboard_runtime_config`, siehe [analytics.md](analytics.md)).
2. **Navigation:** `App.tsx` ist eine Tab-Shell; `tabAliases.resolveTabParam` löst URL-/Alias-Parameter (auch deutsche Aliase: Publikum→Audience, Wachstum→Growth) auf den aktiven Tab auf. `TabNavigation`/`SubTabs` rendern die Navigation.
3. **Daten:** Eine Page ruft einen `use*`-Hook (`hooks/useAnalytics.ts`) → react-query → `api/<thema>.fetch*` → `api/core.fetchApi` (Cookie-Credentials, Timezone-Header, `buildApiUrl`) → `/twitch/api/v2/*`. react-query cached + dedupliziert.
4. **Gating:** `PlanContext`/`useScopes` blenden Features je Plan/Scope aus (`PlanGateCard`, `ScopeSummaryBanner`); fehlende Berechtigungen zeigen Upgrade-Hinweise statt Daten.
5. **Onboarding:** Geführte Touren (`WelcomeTour`, `AnalyticsTour`, `PricingTour`) erklären die Oberfläche; `reset*Tour` startet sie neu.

## 5. Referenz (Bereiche & Schlüsseldateien)

### (root)
- `App.tsx` — Tab-Shell + globale Provider (Query-Client, Plan/Scope-Context).
- `runtimeConfig.ts` — `DashboardRuntimeConfig`, `LIVE_API_BASE`/`DEMO_API_BASE`, `isDemoDashboardPath`, `resolveEffectiveDemoMode`, `hasDemoRuntimeConfig`.
- `tabAliases.ts` — `resolveTabParam` (Alias→Tab).

### api/
- `core.ts` — `fetchApi`/`fetchJson` (Transport), `buildApiUrl`, `withCookieCredentials`, `getBrowserTimezone`, `sanitizeInternalRedirectUrl`, `DASHBOARD_V2_LOGIN_FALLBACK`.
- `analytics.ts` — `fetchOverview`, `fetchMonthlyStats`, `fetchWeekdayStats`, `fetchHourlyHeatmap`, `fetchCalendarHeatmap`, `fetchChatAnalytics`, `fetchViewerOverlap`, `fetchTagAnalysis`, …
- `home.ts` — Internal-Home-Landing-Daten (`InternalHomeKpis30d`, `InternalHomeOAuthStatus`, …, Changelog-Create).
- `ai.ts` (AIChat + Rate-Limit), `socialMedia.ts` (Layout/Clips), `title.ts` (Titel/Insights), `admin.ts`/`affiliate.ts`, `auth.ts` (`fetchAuthStatus`), `billing.ts`, `engagement.ts`.

### hooks/ + context/
- `useAnalytics.ts` — `useOverview`, `useMonthlyStats`, `useWeekdayStats`, `useHourlyHeatmap`, `useCalendarHeatmap`, `useChatAnalytics`, `useViewerOverlap`, `useTagAnalysis` (react-query-Wrapper).
- `useScopes.tsx` — `ScopeProvider`/`useScopes`. `context/PlanContext.tsx` — `PlanProvider`/`usePlan`.

### components/
- `cards/` — `KpiCard`, `HealthScoreCard`, `ScoreGauge`, `InsightsPanel`, `PostStreamReportCard`, `PlanGateCard`, `NoDataCard`, `CategoryRankBadge`, `AffiliateDetailPanel`.
- `charts/` (recharts) — `FollowerFunnel`, `TagPerformanceChart`, `AudienceDemographics`, `AudienceSharing`, `WatchTimeDistribution`, `RetentionRadar`, `RaidRetention`, `ViewerTimelineChart`, `ViewerTrendChart`, `ViewerProfiles`, `LurkerAnalysis`, `CategoryTimingsChart`.
- `heatmaps/` — `CalendarHeatmap`, `HourlyHeatmap`. `layout/` — `Header`, `TabNavigation`, `SubTabs`.
- `onboarding/` — `WelcomeTour`, `AnalyticsTour`, `PricingTour`, `FeatureTooltip`. `pricing/` — `FeaturePicker`, `PlanCardRedesign`, `FeatureComparisonGrid`, `PricingHero`, `TrialCallout`.
- `socialmedia/` — `LayoutEditor`, `EnrichmentPanel`, `AnalyticsTab`. `banners/`/`modals/` — `TrialBanner`, `TrialExpiryModal`. `scopes/`, `roadmap/`, `verwaltung/` (AI-Engagement-Settings).

### pages/
Je Tab eine Page-Komponente, u. a.: `Overview`, `Audience`, `Growth`, `Schedule`, `Sessions`/`SessionDetail`, `Category`, `Comparison`, `Coaching` (mit Empfehlungs-Sektionen), `AIAnalysis`, `Monetization`, `Viewers`/`ViewerTimeline`, `Experimental`, `SocialMedia`/`SocialMediaAdmin`, `TitleGenerator`, `StreamReports`, `Pricing`, `Verwaltung`, `AuthScopes`, `InternalHomeLanding` (die Startseite). Chat-Analytics ist in Subpages (`chatAnalyticsContent`, `chatAnalyticsDeepSections`, `chatSubPages`, `chatAnalyticsViewModel`) aufgeteilt. Deutsche Alias-Pages (`Publikum`, `Wachstum`, `Planung`, `WasTun`) leiten weiter.

### types/ + preview/ + utils/
- `types/analytics.ts` — die TS-Spiegelung aller v2-Antworten; `types/billing.ts` — `PlanTier`/`EntitlementId`/`TabId`/`ALL_ENTITLEMENTS`; `types/scopes.ts`, `types/socialMedia.ts`.
- `preview/fixtures.ts` + `routes.ts` — Demo-/Preview-Daten (`dashboard_preview`).
- `utils/formatters.ts`, `utils/engagementKpi.ts`.

## 6. Datenbank & externe Schnittstellen

- **API:** `/twitch/api/v2/*` (Cookie-Session) — Daten + Auth-Status + Billing-Katalog.
- **Assets:** ausgeliefert vom Python-Backend (`analytics/api_overview`).
- **Keine** direkte DB/externe API im Frontend.

## 7. Stolperfallen / Besonderheiten

- **Kein React-Router:** Navigation läuft über Tabs + `tabAliases`. Wer „Route stimmt nicht“ debuggt, schaut in `App.tsx`/`resolveTabParam`, nicht nach einem Router.
- **Live vs. Demo ist Runtime-Config:** Dieselbe Build läuft live und als Demo; `runtimeConfig` + `isDemoDashboardPath` entscheiden über die API-Base. Der Server injiziert die Config ins HTML.
- **react-query ist der Cache:** Daten werden gecacht/dedupliziert — „warum lädt es nicht neu?“ ist meist eine Query-Key-/Stale-Time-Frage, kein Backend-Problem.
- **Plan-Gating ist Client-seitig sichtbar, aber Server-seitig erzwungen:** `PlanGateCard` blendet UI aus; die eigentliche Berechtigung prüft das Backend (Entitlements). Client-Gating ist UX, keine Sicherheit.
- **Types spiegeln die API:** Ändert sich eine v2-Antwort, muss `types/analytics.ts` nachgezogen werden — sonst bricht TypeScript oder (schlimmer) die Anzeige still.
- **Preview teilt den Code:** `dashboard_preview`/`preview/fixtures` rendert dieselben Komponenten mit Fixtures — Komponenten-Änderungen wirken auf beide.
