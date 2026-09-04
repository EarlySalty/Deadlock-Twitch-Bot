# Research: Einheitliche Dashboard-Shell (Sidebar und Rahmen)

status: erledigt
datum: 2026-09-04
klasse: mittel

## Auftrag

Alle sieben Dashboard-Routen (Home, Analyse, Social Media, Uplink, Verwaltung, Stream-Overlay, Preise) zeigen dieselbe linke Sidebar wie heute die Home-Seite und denselben äußeren Rahmen, sodass der Seitenwechsel keinen Layout-Sprung erzeugt.

## Beobachtungen (belegt, Datei:Zeile)

- Die Sidebar ist heute nur auf Home vollständig. Quelle ist `InternalHomeLanding.tsx:594-790`: Rahmen `internal-home-vibe px-3 py-4 md:px-6 md:py-6` (594), `mx-auto max-w-[2200px]` (605), Drei-Spalten-Grid (606), Sidebar-`aside` (607) mit Profilkopf (611), Main-Nav (639), Tools-Nav (674), Admin-Gruppe (685), Partner-Auswahl bei Admin-Ansicht (730), Hilfe-Gruppe (750).
- Die Sidebar braucht diese Daten: `authStatus` aus `useAuthStatus` (340) für `planName` (382), `adminEligible` (385), `adminMode` (386), `csrfToken`; `home` aus `fetchInternalHome` für `displayName` (523) und `avatarUrl` (524); `canAccessAnalyticsDashboard` (525) blendet den Analyse-Eintrag ein/aus. Der Admin-Schalter ist eine Mutation auf `setAdminMode` mit anschließendem Refetch von `['auth-status']` (387).
- Uplink hat eine abgespeckte Kopie der Sidebar: eigenes `SidebarLink` (`Uplink.tsx:59`, mit `aria-current`), Rahmen `max-w-[1800px]` (953), gleiches Grid (954), `Rise as="aside"` (955), aber ohne Profilkopf, ohne Admin-Gruppe, ohne Hilfe-Gruppe (961-972). `useAuthStatus` wird hier nur für den Admin-Warteschlangen-Block genutzt (881).
- Analyse rendert in `App.tsx:AnalyticsDashboard` (149) einen ganz anderen Rahmen: `px-3 py-4 md:px-7 md:py-8` (299), `max-w-[1700px]` (304), keine Sidebar, stattdessen `Header` (320) und `TabNavigation` (339). Auth/Demo kommen aus `useAuthStatus` und `resolveEffectiveDemoMode` (162-170), Deep-Links aus `resolveTabParam` (196) und `analyticsTabHref`.
- `Header.tsx:91` ist eine `panel-card`-Kopfzeile mit Titel „Channel Intelligence", Streamer-Dropdown, Zeitraum-Umschalter und Sprachwahl. Sie gehört zur Analyse-Seite, nicht zur Sidebar.
- `TabNavigation.tsx:49` rendert die sieben Analytics-Tabs mit `layoutId="activeTab"`; die Tab-Liste (34-42) und die Plan-Gates (55-61) sind unabhängig von der Hülle.
- SocialMediaAdmin (`SocialMediaAdmin.tsx:155`) hat eigenen Rahmen ohne `internal-home-vibe`, `max-w-[1700px]` (161), eigene Kopfzeile mit Film-Icon (162), eigenen `AuthBadge` (111) und den Link „← Analyse-Dashboard" (219). Keine Sidebar.
- Verwaltung (`Verwaltung.tsx:409`) hat `internal-home-vibe px-3 py-4 md:px-7 md:py-8`, `max-w-[900px]` (415), Hero-Kopf (453) mit „Zurück zur Startseite" (480). Keine Sidebar.
- OverlayBuilder (`OverlayBuilder.tsx:10`) kapselt den Rahmen in `OverlayBuilderFrame`: `internal-home-vibe px-3 py-4 md:px-7 md:py-8`, `max-w-[900px]` (18), Zurück-Link zur Verwaltung (73). Keine Sidebar.
- Pricing (`Pricing.tsx:77`) fällt am stärksten heraus: `max-w-7xl mx-auto px-6 lg:px-10 py-10`, weder `internal-home-vibe` noch Sidebar, „Zurück zum Dashboard" (164).
- Der Rahmen ist heute uneinheitlich: Gesamtbreite 2200 (Home) / 1800 (Uplink) / 1700 (Analyse, Social) / 900 (Verwaltung, Overlay) / 7xl (Pricing); Außenabstände `md:px-6 md:py-6` (Home) gegen `md:px-7 md:py-8` (Analyse, Verwaltung, Overlay); Hintergrund `internal-home-vibe` fehlt bei Analyse, Social und Pricing.
- `App.tsx:399-436` wählt die Seitenkomponente per `pathname`. Über allem liegen `QueryClientProvider` (415), `LanguageProvider` (418), `ErrorBoundary` (419). Nur Analyse und Social wrappen Fachinhalt zusätzlich in `PlanProvider` (App.tsx:310, SocialMediaAdmin.tsx:227).
- Der Layout-Test `Uplink.layout.test.tsx` liest Quelltext als String (8) und prüft Regex-Muster, kein Render. Er berührt die Sidebar nicht direkt, verlangt aber, dass `useAuthStatus`/`authStatus?.adminMode` und die `data-section`-Marker im Uplink-Quelltext bleiben (70-75).
- Testlauf: `node --import tsx --test` (package.json). Bauen: `tsc -b && vite build`, Ausgabe nach `../analytics/dashboard_v2/dist`, base `/twitch/dashboard-v2/` (vite.config).

## Hypothesen (unbelegt, nie als Fakt weiterreichen)

- Die Sidebar auf allen Seiten voll (mit Profilkopf/Avatar) zu zeigen verlangt, dass jede Seite `fetchInternalHome` mitlädt (heute tun das nur Home und OverlayBuilder). Prüfen: ob `avatarUrl`/`displayName` auch für nicht angemeldete/Partner-Sichten kommen, sonst Fallback auf Initiale plus `twitchLogin` (wie InternalHomeLanding.tsx:523 es schon macht).
- Der Drei-Spalten-Grid der Home (xl/2xl Zusatzspalte, 606) ist Home-spezifisch (rechte Info-Spalte). Hypothese: die Shell nutzt nur `lg:grid-cols-[220px_minmax(0,1fr)]`, Home behält seine dritte Spalte als Teil des Fachinhalts im Main-Bereich. Prüfen beim Umbau der Home-Seite.
- `resolveEffectiveDemoMode` hängt am `pathname` (App.tsx:163). Die Sidebar in der Demo-Shell darf keine Admin-Gruppe zeigen. Prüfen, dass `adminEligible` in der Demo false bleibt.

## Wahrscheinlich zu ändernde Dateien

- `src/components/layout/DashboardShell.tsx` (neu): Rahmen plus Sidebar. Props: `activeRoute` (Enum der sieben Ziele, bestimmt den aktiven Sidebar-Eintrag), `children` (Fachinhalt), optional `maxWidth`-Override für Analyse/Social falls nötig. Holt selbst `useAuthStatus` und den Profil-Fetch, rendert Profilkopf, Main/Tools/Admin/Hilfe-Gruppen, Admin-Schalter, FAQ, „Tour neu starten".
- `src/components/layout/DashboardSidebar.tsx` (neu, optional aus Shell ausgelagert): das heutige Home-`aside` inklusive `SidebarLink` und `SidebarNavItem`, einmal statt dreifach.
- `src/App.tsx`: Route-Switch (399-436) so umbauen, dass jede Seite in `<DashboardShell activeRoute=...>` steckt. Analytics-Rahmen (299-308) durch die Shell ersetzen, `Header`/`TabNavigation`/`PlanProvider` als Kinder im Main-Bereich behalten (REQ-05).
- `src/pages/InternalHomeLanding.tsx`: Sidebar/Rahmen (594-790) an die Shell abgeben, nur Fachinhalt (Willkommen-Kopf plus Karten ab 795) zurücklassen.
- `src/pages/Uplink.tsx`: eigenes `SidebarLink` (59) und Sidebar/Rahmen (952-976) löschen; adminMode/Waitlist-Block bleibt.
- `src/pages/SocialMediaAdmin.tsx`, `Verwaltung.tsx`, `OverlayBuilder.tsx`, `Pricing.tsx`: eigenen Rahmen abbauen, Fachinhalt in die Shell hängen, Seitenkopf auf die Home-Kopf-Klassen (`display-font`, `panel-card`) angleichen (REQ-06).
- `src/hooks/useDashboardProfile.ts` (neu, optional): kapselt Auth plus Home-Profil für die Sidebar an einer Stelle, damit die Shell nicht in jeder Seite doppelt fetcht.
- `src/pages/Uplink.layout.test.tsx`: nur anpassen, falls Regex-Muster durch das Entfernen der Sidebar-Kopie brechen (INV-03); die geprüften Muster liegen im Fachinhalt, sollten also halten.

### Vorschlag Shell-Struktur

- Aktiver Eintrag: die Shell bekommt `activeRoute` als Prop (`'home' | 'analyse' | 'social' | 'uplink' | 'verwaltung' | 'overlay' | 'pricing'`) und markiert daraus den passenden `SidebarLink`. `App.tsx` kennt die Route ohnehin schon (399-406), gibt sie also direkt weiter. Kein `window.location`-Vergleich in der Shell nötig.
- Hülle je Seite: Home, Uplink, Verwaltung, Overlay geben Rahmen und Sidebar komplett ab. Analyse und Social geben den äußeren Rahmen ab, behalten aber `Header`/`TabNavigation` (Analyse) bzw. ihre Kopfzeile (Social) als Fachinhalt im Main-Slot. Pricing gibt Rahmen und Sidebar ab, sein `max-w-7xl`-Inhalt zieht in den Main-Slot.
- Datenweg: die Shell holt `useAuthStatus` und ein Profil-Fetch (Auth plus `fetchInternalHome`) einmal; über die React-Query-Keys `['auth-status']` und `['internal-home', ...]` teilt sie den Cache mit den Seiten, die dieselben Daten schon nutzen.

## Risiken / Seiteneffekte

- Doppel-Fetch: Shell und Seite rufen beide `useAuthStatus`/`fetchInternalHome`. React-Query dedupliziert über den Key, solange der Key gleich ist. Home nutzt `['internal-home', streamerOverride]` mit Admin-Override, die Shell sollte für den Profilkopf `null` nehmen; sonst zwei Requests. Empfehlung: Shell nutzt nur Auth plus einen schlanken Profil-Fetch, Home behält seinen Override-Query für den Fachinhalt.
- Analytics-Tabs: `Header`, `TabNavigation` und `PlanProvider` müssen im Main-Bereich der Shell bleiben, sonst brechen Tab-Wechsel und Deep-Links (REQ-05). `PlanProvider` liegt heute innerhalb der Analyse-Seite (App.tsx:310), nicht global. Die Shell darf ihn nicht schlucken.
- Demo-Modus: `resolveEffectiveDemoMode` hängt am `pathname` und gilt nur für die Analyse-Route. Die Sidebar-Admin-Gruppe darf in der Demo-Shell nicht erscheinen; `adminEligible` steuert das (685).
- Mobile Sidebar: das Home-Verhalten (`overflow-x-auto` unter lg, `lg:block` darüber, Grid einspaltig) muss in die Shell übernommen werden (639), sonst springt das Layout auf schmalen Viewports (REQ-04). Uplinks Kopie hat dieses Mobil-Verhalten nicht (kein `overflow-x-auto`), also nicht von dort übernehmen.
- Tests: `Uplink.layout.test.tsx` ist quelltextbasiert. Das Entfernen des Uplink-`SidebarLink` darf keine geprüften Muster löschen; die Marker (`data-section=...`, `useAuthStatus`, `adminMode`) liegen im Fachinhalt und bleiben. Vor dem Umbau die Baseline `npm test` messen.
- Rahmenvereinheitlichung ändert sichtbar Breite und Abstände auf jeder Seite (2200 gegen 1700 gegen 900). Eine Zielbreite ist zu wählen (Home 2200 ist der Contract-Maßstab), Fachinhalt-Karten müssen die neue Breite vertragen (besonders Verwaltung/Overlay, heute 900 schmal).
- `PlanProvider`-Kontext: `Header` und `TabNavigation` rufen `usePlan` (Header.tsx:43, TabNavigation.tsx:50). Wenn die Shell diese Komponenten nicht rendert, bleibt der Provider Sache der Analyse-Seite; die Shell selbst braucht `usePlan` nicht.

## Offene Fragen

- Zielbreite der Shell: Contract nennt „dieselbe maximale Gesamtbreite". Home nutzt 2200 mit dritter Spalte. Ob die Shell 2200 (zweispaltig, ohne Home-Zusatzspalte) oder einen kleineren Wert setzt, ist eine Umsetzungsentscheidung im erlaubten Bereich; Vorschlag: 2200 wie Home, Home-Zusatzspalte bleibt Fachinhalt.
- Ob die Sidebar-Profildaten (`avatarUrl`/`displayName`) für jede Route zuverlässig aus `fetchInternalHome` kommen oder ein leichterer Auth-only-Weg genügt, ist erst beim Bauen an der Live-Strecke sicher zu klären.
