# Review: Einheitliche Dashboard-Shell

status: erledigt
datum: 2026-09-04
reviewer: frischer Agent (Opus 4.8), READ-ONLY, adversarial
basis: origin/main...HEAD (feat/dashboard-shell-einheitlich), 16 Dateien unter bot/dashboard_v2

## Urteil

MANGELLISTE (keine Freigabe). Ein MUSS: der neue Shell-Hook loest fuer eine
reale, unterstuetzte Admin-Sitzung einen Login-Redirect auf jeder Route aus und
bricht damit REQ-02/REQ-07/INV-04. Die reine Huelle (Sidebar-Gleichheit,
Rahmen, Kopf-Vereinheitlichung, Zurueck-Link-Abbau, Test) ist sauber umgesetzt;
die Sidebar ist byte-genau inhaltsgleich mit der alten Home-Sidebar
(BackgroundBlobs, SidebarLink-Klassen, Profilkopf, Main/Tools/Admin/Hilfe
identisch verglichen). Kein Fachinhalt verloren, keine Tests abgeschwaecht,
keine neuen Kommentare, kein Scope-Verstoss.

## Maengel

[MUSS] bot/dashboard_v2/src/hooks/useDashboardProfile.ts:9-14: Die Profil-Query
feuert `fetchInternalHome(null)` bedingungslos (`enabled: !loadingAuth`) fuer
JEDE Sitzung. Der Backend-Handler (rust/crates/tb-dashboard-api/src/handlers/
internal_home.rs:519-529, Arm `Admin { actor: None }`) antwortet ohne Override
mit 401 `streamer_session_required` inklusive `loginUrl`; core.ts:114-121
(`handleUnauthorizedResponse`) setzt daraufhin `window.location.href` und leitet
zur Twitch-Login-Seite um. `Admin { actor: None }` ist laut
rust/crates/tb-dashboard-api/src/auth/level.rs:52-56 der Discord-Master-Admin
(`master_dash_session`), der auf dem oeffentlichen Dashboard mit aktivem
Admin-Modus-Cookie Admin bleibt und in auth_status.rs:114-115 mit
`isAdmin:true, twitchLogin:null` gemeldet wird. Die alte Home-Seite fing genau
das mit `canRequestInternalHome` ab (nie `null`-Fetch fuer Admin/Localhost) und
begruendete es in einem Inline-Kommentar, den dieser Branch entfernt hat, ohne
den Guard zu ersetzen. Folge: der Operator wird beim Oeffnen jeder Dashboard-
Route zur Login-Seite geworfen, bevor der Partner-Wechsler ueberhaupt rendert.
Soll: die Shell-Profil-Query darf keinen Login-Redirect ausloesen, wenn die
Sitzung kein aufloesbares eigenes Konto hat (Query gaten, z. B. nicht feuern
wenn `isAdmin && !twitchLogin && !adminEligible`, oder den 401 im Shell-Pfad
schlucken und auf Platzhalter-Profil zurueckfallen). Der harmlose Fall
`Admin { actor: Some }` (Twitch-Admin, liefert eigenes Konto) muss weiter laden.

[SOLL] bot/dashboard_v2/src/hooks/useDashboardProfile.ts:16-28: Der Admin-Modus-
Schalter sitzt jetzt in der geteilten Sidebar und feuert von jeder Route. Die
Mutation invalidiert nur `['auth-status']` und canceled `['internal-home']`,
nicht die seiteneigenen Daten-Queries der aktuellen Route (z. B. Verwaltungs-
oder Social-Media-Daten). Von Home aus stimmte das, weil dort der Query-Key
ueber `streamerOverride` mitkippt; auf anderen Routen kann nach dem Umschalten
veralteter Nicht-Admin-Inhalt stehenbleiben, bis der Nutzer neu laedt. REQ-07
verlangt "wie heute von Home aus"; das ist fuer die aktuelle Route nur teilweise
erfuellt. Soll: nach Admin-Umschalten auch die aktive Seiten-Query invalidieren
(oder dokumentieren, dass der Schalter einen Reload ausloest).

[SOLL] bot/dashboard_v2/tests/dashboardShell.test.ts:36-42: Die Breiten-Pruefung
(`doesNotMatch max-w-[`) laeuft nur ueber die sechs Seiten-Dateien. Der Rahmen
der Analyse-Route liegt in App.tsx (Funktion `AnalyticsDashboard`), nicht in
einer Seiten-Datei, und wird von keiner Assertion auf Eigenbreite geprueft. Ein
erneutes `max-w-[1700px]` dort bliebe gruen. REQ-03 ist fuer die Analyse-Route
nicht test-abgesichert. Soll: App.tsx in die Breiten-Pruefung aufnehmen
(ausserhalb der Shell-Konstante).

[SOLL] bot/dashboard_v2/src/pages/InternalHomeLanding.tsx (relokierter Admin-
Auswahlblock, Optionstext des Partner-Selects und die Ueberschriften "Partner
auswaehlen" / "moechtest"): Beim Verschieben in den Main-Slot wandern
vorbestehende Gedankenstriche und ae/oe-Ersatzschreibweisen mit. Nicht neu
eingefuehrt, aber die Zeilen wurden angefasst; da nutzersichtbar (Admin-
Ansicht), gehoeren Gedankenstriche raus und echte Umlaute rein, wenn ohnehin
editiert wird. Kein Blocker.

## Nicht-Beanstandungen (geprueft, in Ordnung)

- REQ-01/03: Alle sieben Routen in App.tsx in `DashboardShell` gewickelt; Shell
  traegt `internal-home-vibe`, `max-w-[2200px]`, Sidebar-Spalte 220px, Main-Slot.
  Keine Seite setzt mehr Eigenbreite oder `internal-home-vibe` (Test gruen, 23/23
  lokal verifiziert).
- REQ-02: DashboardSidebar identisch zur alten Home-Sidebar (Profilkopf, Main,
  Tools, Admin-Schalter samt Hinweis, Hilfe); `activeRoute` markiert korrekt.
  Partner-Wechsler bewusst in Home-Main (Abweichung E2/E3, contract-konform, da
  REQ-02 ihn nicht listet).
- REQ-04: Sidebar-Mobilverhalten (`flex overflow-x-auto ... lg:block`) byte-gleich
  zur Home-Vorlage; Uplinks abweichende Alt-Sidebar ist entfallen, wie REQ-04
  (Home als Massstab) verlangt.
- REQ-05: Analyse behaelt PlanProvider, Header, TabNavigation, `analyticsTabHref`;
  nur der aeussere Rahmen ist durch die Shell ersetzt.
- INV-01: Kein Fachinhalt verloren. Home behaelt seine dritte Updates-Spalte als
  verschachteltes Grid im Main-Slot; Uplink-`data-section`-Marker,
  OBS-Schritte, Warteliste unveraendert (`id="uplink-main"` entfernt, aber
  nirgends referenziert).
- INV-03: Uplink.layout.test.tsx unveraendert und weiter gruen (prueft nur
  Uplink-Inhalt, nie Rahmen/Sidebar); keine Test-Abschwaechung.
- INV-05/Scope: Nur Dateien im erlaubten Bereich plus package.json (Amendment)
  und tests/dashboardShell.test.ts (Ansage). Kein Rust/Migration/Caddy/routes.ts
  angefasst.
- INV-06: Keine neuen Code-Kommentare; alte erklaerende Kommentare (Avatar-
  Fallback, Admin-Mutation, Verwaltungs-Hero) beim Verschieben entfernt.
- Kein Doppel-Fetch: Nicht-Admins teilen den Query-Key `['internal-home', null]`
  zwischen Shell und Seiten-Query (React-Query-Dedup), eine Anfrage.
- Preview-Modus maskiert den MUSS-Fall (Fixtures statt Backend), deshalb in
  M4/M5 nicht aufgefallen.

## Runde 2

status: FREIGABE
basis: git diff 0aa2716b..HEAD (5 Commits: 151c343f, 1957c5d1, 577abc38, ccd84a7d, 19014e77)

Urteil: Alle vier Befunde geschlossen, keine neuen Blocker. Freigabe.

(a) MUSS geschlossen. useDashboardProfile.ts:10-13 fuehrt
`canRequestInternalHome = !loadingAuth && !isLocalhostAdmin && !isAdminWithoutOwnLogin`
ein und haengt die Query daran (`enabled: canRequestInternalHome`,
useDashboardProfile.ts:19). Gegen die Backend-Arme geprueft:
Discord-Master-Admin (`Admin { actor: None }`, auth_status.rs:114-115:
isAdmin=true/twitchLogin=null) und Localhost-Admin werden gegatet, kein
`fetchInternalHome(null)`, kein 401, keine Umleitung. Normaler Partner und
Twitch-Admin ohne Admin-Modus (twitchLogin gesetzt) laden ihr eigenes Konto
weiter. Profilkopf-Fallback ohne Fetch ist sauber
(useDashboardProfile.ts:37-39: displayName faellt auf 'Admin', kein Crash).
Nach dem Aktivieren des Admin-Modus wird die Query disabled, die spaetere
invalidateQueries() refetcht sie nicht mehr, also auch dort kein Nach-Toggle-401.

(b) `queryClient.invalidateQueries()` ohne Filter (DashboardSidebar.tsx:206-209)
loest KEINE Endlosschleife aus: die Mutation ist rein knopfgetrieben, kein
Effekt reagiert auf auth-status und mutiert erneut (die authStatus-Effekte in
InternalHomeLanding rufen nur setSelectedStreamer/replaceState). Kostet ein
seitenweites Neuladen aller aktiven Queries (leichtes Flackern), was der
gewuenschten Routendaten-Auffrischung entspricht. Vertretbar.

(c) Tests tragen. Analytics-Breitentest (dashboardShell.test.ts:55-72) schneidet
den AnalyticsDashboard-Block aus App.tsx und prueft `doesNotMatch max-w-[` und
`internal-home-vibe`, wird bei Rueckfall rot. Gate-Test (dashboardShell.test.ts:49-52)
wird rot, wenn `enabled: canRequestInternalHome` zurueckgedreht oder die
Gate-Variable entfernt wird. 5/5 lokal gruen verifiziert.

(d) Umlaut-Commit fasst nur nutzersichtbare Strings an
(InternalHomeLanding.tsx:353-369: "auswählbar", "auswählen", "Wähle...möchtest",
Options-Text "Partner wählen" ohne Gedankenstriche). `value`, `key={channel.login}`,
`htmlFor`, `id` unveraendert; keine alte Schreibweise wird irgendwo referenziert
(grep ueber src/ und tests/ leer). Keine Keys, Identifier oder Tests gebrochen.

Nachrangig (kein Blocker):
- [SOLL] useDashboardProfile.ts:12: Der Gate ist etwas breiter als noetig. Ein
  Twitch-Admin MIT aktivem Admin-Modus (`Admin { actor: Some }`,
  auth_status.rs setzt dort twitchLogin=null) wuerde vom Backend sein eigenes
  Konto bekommen, wird aber jetzt mit-gegatet und zeigt den 'Admin'-Fallback
  statt Login/Avatar. Weicht von der dokumentierten Abweichung E3 ("eigenes
  Konto im Admin-Modus") ab, ist aber funktional harmlos. Enger ueber
  `adminEligible` trennbar, falls der eigene Kontokopf im Admin-Modus gewuenscht
  ist.
- [SOLL] dashboardShell.test.ts:49-52 prueft nur die Verdrahtung des Gates, nicht
  die Korrektheit seiner Bedingungen; ein aufgeweichter Gate (immer true) bliebe
  gruen. Reiner Rauchtest.
