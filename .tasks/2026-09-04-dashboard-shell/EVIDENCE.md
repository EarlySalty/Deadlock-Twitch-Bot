# Evidence: Einheitliche Dashboard-Shell (Sidebar und Rahmen)

status: erledigt
datum: 2026-09-04
contract: CONTRACT.md

Alle Pfade relativ zu `bot/dashboard_v2/`. Fundstellen aus dem Worktree
`/home/nathanael/.worktrees/tb-dashboard-shell` (ab origin/main).

## Analoge Implementierungen (wie löst das Repo so etwas schon?)

- src/pages/InternalHomeLanding.tsx:594 : Home-Rahmen `internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6`, der Referenz-Hintergrund und die Referenz-Außenabstände.
- src/pages/InternalHomeLanding.tsx:605 : `relative mx-auto max-w-[2200px]`, die Referenz-Gesamtbreite (2200px, größer als alle anderen Seiten).
- src/pages/InternalHomeLanding.tsx:606 : Referenz-Grid `lg:grid-cols-[220px_minmax(0,1fr)] xl:grid-cols-[220px_minmax(0,1fr)_340px] 2xl:grid-cols-[240px_minmax(0,1fr)_420px]`. Home hat rechts eine dritte Spalte, die die anderen Seiten nicht haben.
- src/pages/InternalHomeLanding.tsx:607 : Sidebar-Karte `<Rise as="aside" className="panel-card card-glow self-start rounded-2xl p-4 lg:sticky lg:top-4">`.
- src/pages/InternalHomeLanding.tsx:239 : `SidebarLink` mit aktiv/inaktiv-Klassen, `flex items-center gap-3 rounded-xl px-3 py-2 ... no-underline`; aktiv setzt links `lg:border-l-2 lg:border-primary`.
- src/pages/InternalHomeLanding.tsx:265 : `interface SidebarNavItem { href; label; icon; active? }`.
- src/pages/InternalHomeLanding.tsx:548 : `mainNavItems` mit Home (active:true), Analyse (`analyticsTabHref('overview')`, nur wenn `canAccessAnalyticsDashboard`), Social Media Dashboard (`/social-media-admin`), Uplink.
- src/pages/InternalHomeLanding.tsx:556 : `toolNavItems` mit Verwaltung, Stream-Overlay, `Plan: ${planName}`, Changelog.
- src/pages/InternalHomeLanding.tsx:611 : Profilkopf mit Avatar (`avatarUrl`) oder Initiale, `data-tour-id="tour-plan"`, Plan-Badge `{planName}`.
- src/pages/InternalHomeLanding.tsx:639 : Nav mit `data-tour-id="tour-nav"`, Mobil-Verhalten `flex gap-2 overflow-x-auto pb-1 lg:block lg:space-y-1 lg:overflow-visible` (unter lg horizontale Scroll-Leiste, ab lg vertikale Liste).
- src/pages/InternalHomeLanding.tsx:685 : Admin-Gruppe nur bei `adminEligible`; Schalter Zeile 692 `adminModeMutation.mutate(!adminMode)`, `aria-pressed={adminMode}` plus Hinweistext.
- src/pages/InternalHomeLanding.tsx:750 : Hilfe-Gruppe `data-tour-id="tour-help"` mit FAQ-Link `/twitch/faq` (Zeile 752) und „Tour neu starten" (Zeile 762), das `resetWelcomeTour()` plus `window.location.reload()` ruft.
- src/pages/InternalHomeLanding.tsx:98 : `BackgroundBlobs`, Zeile 112 `DashboardSkeleton` (Skeleton-Zustand der Home-Shell während des Ladens).
- src/pages/OverlayBuilder.tsx:10 : `OverlayBuilderFrame` mit eigenem Rahmen `internal-home-vibe min-h-screen relative px-3 py-4 md:px-7 md:py-8`, Zeile 18 `max-w-[900px]` (schmaler und andere Abstände als Home).

## Bestehende Abstraktionen (werden wiederverwendet, nicht nachgebaut)

- src/hooks/useAnalytics.ts:177 : `useAuthStatus()` mit `queryKey: ['auth-status']`, `queryFn: fetchAuthStatus`; liefert plan, adminMode, adminEligible, isAdmin, isLocalhost, csrfToken. Home, App, Uplink, SocialMediaAdmin, Verwaltung, OverlayBuilder rufen es je einzeln, React-Query dedupliziert über den Key.
- src/hooks/useAnalytics.ts:156 : `useStreamerList()` `queryKey: ['streamers']`, staleTime 10min.
- src/api/home.ts : `fetchInternalHome(streamerOverride)` liefert `avatarUrl`, `displayName`, `twitchLogin`, `changelog`. Home nutzt Key `['internal-home', streamerOverride]` (InternalHomeLanding.tsx:375), OverlayBuilder Key `['internal-home', null]` (OverlayBuilder.tsx:29).
- src/api/auth.ts : `setAdminMode(enabled, csrf)` (InternalHomeLanding.tsx:9 Import, 387 Mutation).
- src/motion/Rise.tsx:25 : `Rise({ as, className, step })` fügt `rise-in` plus Staffelung hinzu; Home-Sidebar und alle Karten nutzen es.
- src/preview/routes.ts:5 : Routenkonstanten `PREVIEW_HOME_ROUTE` bis `PREVIEW_CHANGELOG_ROUTE`; Zeile 17 `analyticsTabHref(tab)`.
- src/tabAliases.ts:9 : `TAB_ALIASES`, Zeile 28 `resolveTabParam`; steuert die Analytics-Deep-Links.
- src/context/LanguageContext.tsx:43 : `LanguageProvider`, Zeile 74 `useT`; src/context/PlanContext.tsx:32 `PlanProvider`, Zeile 111 `usePlan`.
- src/index.css:580 : `.internal-home-vibe`; :223 `.panel-card`; :259 `.card-glow`; :145 `.display-font`; :464 `.sidebar-avatar-glow`. Die geteilten Hintergrund-, Karten- und Typo-Klassen.

## Relevante Tests (laufen vorher, laufen nachher)

- src/pages/Uplink.layout.test.tsx:8 : liest `Uplink.tsx` als Textstring (`readFileSync`), kein React-Render, kein jsdom. Prüft per Regex.
- src/pages/Uplink.layout.test.tsx:70 : verlangt `useAuthStatus` und `authStatus?.adminMode` im Uplink-Quelltext, plus `data-section="uplink-admin-waitlist"` und `data-section="uplink-right-column"` (Admin-Warteliste, nicht die Sidebar).
- src/pages/Uplink.layout.test.tsx:13 : prüft Kopf, OBS-Disclosure-Liste, Fensteradressen; alles Fachinhalt der Uplink-Seite, unberührt von der Shell.
- package.json (test-Script) : Runner ist `node --import tsx --test <liste>`, kein Vitest. Enthält u. a. `src/pages/Uplink.layout.test.tsx`, `tests/ausschnittRahmen.test.ts`, `tests/riseMotion.test.ts`, `tests/languageProvider.test.tsx`, `tests/tabAliases.test.ts`.
- tests/tabAliases.test.ts : sichert `resolveTabParam`/`analyticsTabHref` (REQ-05, Deep-Links).

## Öffentliche Schnittstellen und Verträge (dürfen nicht brechen)

- src/App.tsx:399 : Routing per `window.location.pathname` auf `PREVIEW_HOME_ROUTE`, `/social-media-admin`, `PREVIEW_VERWALTUNG_ROUTE`, `PREVIEW_OVERLAY_ROUTE`, `PREVIEW_PRICING_ROUTE`, `PREVIEW_UPLINK_ROUTE` und die Analytics-Aliasse (`/analyse`, `/twitch/onboarding`, `/dashboard-v2`, `/twitch/dashboard-v2`, `PREVIEW_ANALYTICS_ROUTE`).
- src/App.tsx:418 : `LanguageProvider` über allem, Zeile 419 `ErrorBoundary` (INV-04).
- src/preview/routes.ts : laut CONTRACT verboten zu ändern, die Konstanten werden aber importiert.
- vite.config : `base: '/twitch/dashboard-v2/'`, `outDir: '../analytics/dashboard_v2/dist'`. Bundle-Auslieferung; spa.rs serviert das und bleibt unberührt (REQ-08, INV-05).

## Änderungsfläche (welche Dateien voraussichtlich angefasst werden)

- src/components/layout/ : neue Shell-Komponente (Sidebar plus Rahmen).
- src/App.tsx : jede Route in die Shell einwickeln, Analytics-Rahmen (Zeile 299/304) an die Shell angleichen.
- src/pages/InternalHomeLanding.tsx : Sidebar/Rahmen (Zeile 239, 265, 548, 556, 594-790) an die Shell abgeben, Fachinhalt (Karten ab 795) behalten.
- src/pages/Uplink.tsx : doppeltes `SidebarLink` (Zeile 59) und Sidebar/Rahmen (Zeile 952-976) entfernen, in die Shell überführen; adminMode/Waitlist bleibt.
- src/pages/SocialMediaAdmin.tsx:155 : Rahmen `max-w-[1700px]` plus eigener Kopf/AuthBadge in die Shell überführen.
- src/pages/Verwaltung.tsx:409 : Rahmen `max-w-[900px]` plus Hero-Kopf angleichen.
- src/pages/OverlayBuilder.tsx:10 : `OverlayBuilderFrame`/`max-w-[900px]` angleichen.
- src/pages/Pricing.tsx:77 : `max-w-7xl` ohne `internal-home-vibe`/Sidebar in die Shell überführen.
- src/hooks/ : evtl. gemeinsamer Hook, der Auth plus Home-Profil einmal für die Sidebar holt.

## Offene Architekturfrage

- keine. Umsetzungsempfehlung in RESEARCH.md, alle Entscheidungen liegen im erlaubten Bereich.
