# Plan: Einheitliche Dashboard-Shell

status: aktiv
datum: 2026-09-04
contract: CONTRACT.md
research: RESEARCH.md, EVIDENCE.md

## Entscheidungen (Orchestrator)

- E1 Zielrahmen = Home-Maß: `internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6`, innen `relative mx-auto max-w-[2200px]`, Grid `grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]`, `BackgroundBlobs` aus Home wandert in die Shell. Home behält seine dritte Spalte (Updates) als Fachinhalt innerhalb des Main-Slots.
- E2 Zwei neue Dateien: `components/layout/DashboardShell.tsx` (Rahmen, Grid, Blobs, Main-Slot) und `components/layout/DashboardSidebar.tsx` (das heutige Home-`aside` inklusive `SidebarLink`, `SidebarNavItem`, Profilkopf, Main/Tools/Admin/Hilfe, Admin-Schalter, Partner-Auswahl bei Admin-Ansicht, FAQ, Tour neu starten). Prop `activeRoute: 'home' | 'analyse' | 'social' | 'uplink' | 'verwaltung' | 'overlay' | 'pricing'`, gesetzt in `App.tsx` aus dem vorhandenen Route-Switch.
- E3 Datenweg der Sidebar: neuer Hook `hooks/useDashboardProfile.ts` kapselt `useAuthStatus` plus Profil-Fetch (`fetchInternalHome` ohne Override, Query-Key ohne Streamer-Override, `staleTime` mindestens 5 min), damit die Shell auf jeder Route einmal lädt und Home seinen Override-Query für den Fachinhalt behält.
- E4 Analyse: `PlanProvider`, `Header`, `TabNavigation` bleiben als Kinder im Main-Slot. Der äußere Rahmen (App.tsx 299-308) fällt weg.
- E5 Redundante „Zurück zum Dashboard / zur Startseite"-Links in Verwaltung, Overlay, Social, Pricing entfallen, weil die Sidebar die Navigation übernimmt. Der Link „Zurück zur Verwaltung" im OverlayBuilder bleibt (Fachfluss).
- E6 Seitenköpfe (REQ-06): jede Route beginnt im Main-Slot mit einer `panel-card rounded-2xl p-5 md:p-6`-Kopfkarte, Eyebrow `text-[11px] font-bold uppercase tracking-[0.18em] text-primary`, Titel `display-font text-2xl font-extrabold text-white`, Untertitel `mt-2 max-w-2xl text-sm text-text-secondary`, Aktionen rechts. Vorlage ist die Home-Kopfkarte. Uplink hat das schon.
- E7 Demo-Shell: Admin-Gruppe nur bei `adminEligible`, wie heute auf Home. Keine neue Logik.
- E8 Keine Code-Kommentare. Bestehende Kommentare in angefassten Blöcken löschen.
- E9 Neuer Test `components/layout/DashboardShell.test.tsx` im Stil von `Uplink.layout.test.tsx` (Quelltext-Regex, Runner `node --import tsx --test`): prüft, dass `App.tsx` jede der sieben Routen in `DashboardShell` wrappt und keine Seite mehr `max-w-[` als Gesamtbreite oder `internal-home-vibe` selbst setzt. Rot-Gegenprobe: eine Route in App.tsx ohne Shell rendern, Test muss rot werden, Sabotage zurücknehmen.

## Milestones

### M1 Baseline
- Änderungen: keine.
- Befehl: `cd bot/dashboard_v2 && npm ci --no-audit --no-fund && npm test > /tmp/tb-dash-test-baseline.log 2>&1; echo exit=$?` und `npx tsc -b && npx vite build > /tmp/tb-dash-build-baseline.log 2>&1; echo exit=$?`
- Erwartung: passed/failed aus dem Log als Baseline in Status notieren.
- Stop: Build rot ohne eigene Änderung, dann Ursache melden statt weiterbauen.

### M2 Shell und Sidebar aus Home herauslösen
- Änderungen: `DashboardShell.tsx`, `DashboardSidebar.tsx`, `useDashboardProfile.ts`, `DashboardShell.test.tsx` neu; `InternalHomeLanding.tsx` gibt Rahmen, Blobs und `aside` ab; `App.tsx` rendert `<DashboardShell activeRoute="home"><InternalHome/></DashboardShell>`. Skeleton der Home-Seite verliert den Sidebar-Teil.
- Erwartung: Home sieht aus wie vorher. Neuer Test ist für die noch nicht umgestellten Routen rot und wird bis M4 grün.
- Befehl: `npm test > /tmp/tb-dash-test-m2.log 2>&1; echo exit=$?` plus `npx tsc -b && npx vite build`
- Stop: Admin-Schalter, Tour-Reset oder Partner-Auswahl funktionieren nicht mehr über die Shell.

### M3 Uplink, Verwaltung, Overlay
- Änderungen: `Uplink.tsx` verliert eigenes `SidebarLink` und Rahmen (Zeilen 59, 952-976), Main-Inhalt bleibt inkl. `data-section`-Marker; `Verwaltung.tsx` und `OverlayBuilder.tsx` (`OverlayBuilderFrame`) geben Rahmen ab, Kopf nach E6, Zurück-Links nach E5; `App.tsx` wrappt mit `activeRoute`.
- Befehl: wie M2, Log `/tmp/tb-dash-test-m3.log`
- Stop: `Uplink.layout.test.tsx` rot; dann nur die Hüllen-Muster anpassen (INV-03), nie die Fachmuster.

### M4 Analyse, Social Media, Preise
- Änderungen: `App.tsx` AnalyticsDashboard-Rahmen durch Shell ersetzen (E4); `SocialMediaAdmin.tsx` Rahmen und Kopf nach E6 (eigener `AuthBadge` bleibt als Aktion rechts im Kopf, `PlanProvider` bleibt); `Pricing.tsx` Rahmen ab, Kopf nach E6.
- Befehl: wie M2, Log `/tmp/tb-dash-test-m4.log`; jetzt alle Tests grün, Zahl passed notieren; Rot-Gegenprobe für E9 durchführen und Ist/Soll notieren.
- Stop: Tab-Deep-Link `/analyse?tab=...` oder Demo-Route rendert nicht.

### M5 Sichtprüfung
- Änderungen: keine.
- Befehl: `npx vite preview --port 4173` starten, Routen laut `preview/routes.ts` (`/dashboard`, `/`, `/social-media-admin`, `/uplink`, `/verwaltung`, `/overlay`, `/pricing`) mit dem Preview-Browser der Hauptsession öffnen; Screenshots je Route nach `.tasks/2026-09-04-dashboard-shell/screens/`.
- Erwartung: gleiche Sidebar, gleiche Breite, gleicher Hintergrund auf allen Routen.
- Stop: Layout-Sprung zwischen zwei Routen sichtbar.

### M6 Selbstprüfung und Übergabe
- `python3 /home/nathanael/Documents/claude-config/bin/diff-policy.py /home/nathanael/.worktrees/tb-dashboard-shell origin/main`
- Commits klein pro Milestone, Commit-Texte ohne Gedankenstriche, Status in PLAN.md nachführen.

## Abweichungen

- E2/E3, Partner-Auswahl (Admin): Der Admin-Streamer-Wechsler treibt ausschliesslich die Fachinhalt-Query von Home (`['internal-home', streamerOverride]`, InternalHomeLanding.tsx). Die Shell laedt ihr Profil laut E3 ohne Override (`['internal-home', null]`, useDashboardProfile.ts). Wuerde der Wechsler in die geteilte Shell-Sidebar wandern, muesste sein `selectedStreamer`-State plus Auto-Set- und URL-Sync-Effekt entweder auf 6 fremde Routen wirken (keine davon nutzt ihn) oder ueber einen neuen Cross-Route-Context an Home zurueckgereicht werden. REQ-02 zaehlt den Partner-Wechsler nicht zu den Sidebar-Inhalten (nur Profilkopf, Main, Tools, Admin-Schalter, Hilfe). Entscheidung: Wechsler bleibt in Home und rendert als Admin-Karte oben im Main-Slot; die Shell-Sidebar erfuellt REQ-02 vollstaendig. Kein Cross-Route-State, keine Wirkung auf INV-01-Fachinhalt der anderen Seiten.
- E3, Profilkopf-Identitaet (Admin): Da die Shell mit `null`-Override laedt, zeigt der Sidebar-Profilkopf im Admin-Modus das eigene Konto statt des ausgewaehlten Partners (vorher: ausgewaehlter Partner). Fuer Nicht-Admins unveraendert (Override ist dort ohnehin null). Von E3 so vorgezeichnet, REQ-02 legt die Identitaet nicht fest.

## Status

- M1: fertig. Baseline 171 pass, 0 fail (/tmp/tb-dash-test-baseline.log); tsc und vite build gruen.
- M2: fertig. Shell/Sidebar/Hook neu, Home gibt Rahmen und Sidebar ab, App wickelt Home in die Shell. 171 pass, 0 fail (/tmp/tb-dash-test-m2.log); tsc und vite build gruen.
- M3: fertig. Uplink, Verwaltung, Overlay geben Rahmen und Sidebar-Kopie ab; App wickelt sie in die Shell (activeRoute). Wicklung liegt in App.tsx (nicht in den Seiten), damit E9 sie prueft. 171 pass, 0 fail (/tmp/tb-dash-test-m3.log); tsc und vite build gruen.
- M4: offen
- M5: offen
- M6: offen
