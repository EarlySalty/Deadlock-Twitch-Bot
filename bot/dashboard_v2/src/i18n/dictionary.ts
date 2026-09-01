/**
 * Zweisprachigkeit ohne Framework.
 *
 * Der deutsche Text ist selbst der Schluessel. Das hat zwei Folgen, die hier
 * beabsichtigt sind:
 *  - Fehlt eine Uebersetzung, steht automatisch der deutsche Text da. Es kann
 *    nie ein Schluessel oder eine leere Stelle in der Oberflaeche landen.
 *  - Seiten, die noch nicht angefasst wurden, funktionieren unveraendert
 *    weiter; sie ziehen einfach kein `t()` durch.
 *
 * Platzhalter sind `{name}` und werden von `translate` ersetzt.
 */

export type Language = 'de' | 'en';

export const LANGUAGES: Language[] = ['de', 'en'];

export const DEFAULT_LANGUAGE: Language = 'de';

/** Eine Wahl pro Browser, geteilt ueber alle Routen (/analyse, /social-media-admin). */
export const LANGUAGE_STORAGE_KEY = 'dashboard.language';

/** Fuer toLocaleString & Co., damit Datum und Zahlen mitwandern. */
export const LOCALES: Record<Language, string> = {
  de: 'de-DE',
  en: 'en-US',
};

export const LANGUAGE_LABELS: Record<Language, string> = {
  de: 'Deutsch',
  en: 'English',
};

const EN: Record<string, string> = {
  'Social-Media-Bereich wählen': 'Choose social media section',
  'Clipstatus filtern': 'Filter clip status',
  Freigabemodus: 'Approval mode',
  // -- Seitenrahmen / App ------------------------------------------------
  'Dashboard-Fehler': 'Dashboard error',
  'Ein unerwarteter Fehler ist aufgetreten.': 'An unexpected error occurred.',
  'Erneut versuchen': 'Try again',
  'Demo-Daten': 'Demo data',
  'Nicht authentifiziert': 'Not authenticated',
  'Localhost (Admin)': 'Localhost (admin)',
  Admin: 'Admin',
  Partner: 'Partner',
  'Demo-Daten aus einem statischen Snapshot. Profilwechsel und Analysen laufen ausschließlich über den Demo-Namespace.':
    'Demo data from a static snapshot. Profile switching and analyses run exclusively through the demo namespace.',

  // -- Header -------------------------------------------------------------
  'Fokus: {focus}': 'Focus: {focus}',
  'Zeitraum: letzte {days} Tage': 'Range: last {days} days',
  Basis: 'Basic',
  Preview: 'Preview',
  'Demo-Profil': 'Demo profile',
  'Alle Streamer': 'All streamers',
  'Alle Partner': 'All partners',
  'Suchen…': 'Search…',
  'Weitere Streamer': 'More streamers',
  '(extern)': '(external)',

  // -- Tab-Navigation -----------------------------------------------------
  Übersicht: 'Overview',
  Streams: 'Streams',
  Publikum: 'Audience',
  Wachstum: 'Growth',
  Planung: 'Planning',
  'Was tun?': 'What now?',
  Monetization: 'Monetization',
  Beta: 'Beta',

  // -- Social-Media-Rahmen ------------------------------------------------
  'Alle Kanäle': 'All channels',
  'Dein Kanal': 'Your channel',
  'Social Media': 'Social media',
  'Freigabe für diesen Streamer entziehen': 'Revoke access for this streamer',
  'Diesen Streamer für das eigene Social-Media-Dashboard freischalten':
    'Give this streamer access to their own social media dashboard',
  Freigegeben: 'Access granted',
  Freigeben: 'Grant access',
  '— Streamer wählen —': '— Select streamer —',
  '← Analyse-Dashboard': '← Analytics dashboard',
  'Zugriff wird geprüft…': 'Checking access…',
  'Anmeldung wird geprüft…': 'Checking sign-in…',
  'Der Freigabestand wird geprüft. Änderungen sind gesperrt.':
    'Access status is being checked. Changes are locked.',
  'Freigabestand wird geprüft…': 'Checking access status…',
  'Die Streamer-Liste konnte nicht geladen werden.': 'The streamer list could not be loaded.',
  'Der Freigabestand konnte nicht geladen werden. Änderungen bleiben gesperrt.':
    'Access status could not be loaded. Changes remain locked.',
  'Der eigene Social-Media-Zugriff konnte nicht geprüft werden.':
    'Your social media access could not be checked.',
  'Freigabe wird gespeichert…': 'Saving access…',
  'Die Freigabe konnte nicht gespeichert werden.': 'Access could not be saved.',
  'Die Freigabe wurde gespeichert.': 'Access was saved.',
  'Zugriff konnte nicht geprüft werden': 'Access could not be checked',
  'Der Social-Media-Bereich bleibt sicherheitshalber gesperrt.':
    'The social media section remains locked for safety.',
  'Noch nicht freigeschaltet': 'Not enabled yet',
  'Social Media wird für deinen Kanal erst nach Freigabe aktiv. Melde dich bei EarlySalty, wenn du deine Clips hier aufbereiten möchtest.':
    'Social media becomes active for your channel once it has been enabled. Get in touch with EarlySalty if you would like to prepare your clips here.',
  'Lade Streamer-Liste…': 'Loading streamer list…',
  'Keine Streamer gefunden.': 'No streamers found.',

  // -- Social-Media-Seite -------------------------------------------------
  'Streamer auswählen': 'Select a streamer',
  'Wähle oben einen Streamer aus, um Layouts, Clips und Uploads zu verwalten.':
    'Pick a streamer above to manage layouts, clips and uploads.',
  'Clips automatisch posten': 'Post clips automatically',
  'Clips sicher vorbereiten': 'Prepare clips safely',
  'Clip-Pipeline': 'Clip pipeline',
  'Social Media für': 'Social media for',
  'Testbetrieb – Veröffentlichung pausiert': 'Test mode – publishing paused',
  'Clips können vollständig aufbereitet, geprüft und für später freigegeben werden. Es wird nichts an Plattformen gesendet.':
    'Clips can be fully prepared, reviewed and approved for later. Nothing is sent to any platform.',
  'Der Freigabemodus konnte nicht geladen werden. Freigaben bleiben gesperrt.':
    'The approval mode could not be loaded. Approvals remain locked.',
  'TikTok bleibt gesperrt, bis Sichtbarkeit, Interaktionen und Zustimmung pro Clip gewählt werden können.':
    'TikTok remains locked until visibility, interactions, and consent can be selected for each clip.',

  // -- Zeitplan, Freigabe-Modi, Kategorien, Vorrat ------------------------
  'Clip-Pool': 'Clip pool',
  Zeitplan: 'Schedule',
  Konten: 'Accounts',
  Kategorien: 'Categories',
  'Clips im Pool': 'Clips in pool',
  'Nur nach Freigabe': 'Only after approval',
  'Jeder Clip wartet auf dein Okay.': 'Every clip waits for your go-ahead.',
  'Einspruch bis zum Termin': 'Veto until the slot',
  'Clips werden eingeplant. Du kannst sie bis zum Posting stoppen.':
    'Clips get scheduled. You can stop them until they go out.',
  Vollautomatik: 'Fully automatic',
  'Clips werden ohne Sichtung freigegeben; Veröffentlichung folgt Betriebsmodus und Zeitplan.':
    'Clips are approved without review; publication follows the operating mode and schedule.',
  'Zeiten gelten in {tz}.': 'Times apply in {tz}.',
  'Automatisch posten': 'Post automatically',
  'Automatisch auf {platform} posten': 'Post automatically to {platform}',
  'Posts pro Woche': 'Posts per week',
  'Posts pro Woche für {platform}': 'Posts per week for {platform}',
  'Höchstens pro Tag': 'At most per day',
  'Höchstens pro Tag für {platform}': 'At most per day for {platform}',
  'Uhrzeiten, mit Komma getrennt': 'Times, comma separated',
  'Uhrzeiten für {platform}': 'Times for {platform}',
  'Nächster Post: {termin}': 'Next post: {termin}',
  'Mit Titel- und Hashtag-Vorschlägen.': 'With title and hashtag suggestions.',
  'Ohne automatische Vorschläge; Metadaten bleiben manuell bearbeitbar.':
    'No automatic suggestions; metadata remains editable manually.',
  'Vorrat reicht noch für {posts} Posts.': 'Enough clips left for {posts} posts.',
  '{clips} Clips im Pool.': '{clips} clips in the pool.',
  '{clips} Clips im Pool, das sind rund {tage} Tage bei {proWoche} Posts pro Woche.':
    '{clips} clips in the pool, about {tage} days at {proWoche} posts per week.',
  'Twitch-Clips werden automatisch eingesammelt und als prüfbare 9:16-Vorschau für Shorts, TikTok und Reels aufbereitet. Streamer-Layouts gelten als Standard, einzelne Clips lassen sich anpassen.':
    'Twitch clips are collected automatically and prepared as reviewable 9:16 previews for Shorts, TikTok and Reels. Streamer layouts act as defaults, while individual clips can be adjusted.',
  'Layout: Repo-Default aktiv': 'Layout: repo default active',
  'Layout: Streamer-Default': 'Layout: streamer default',
  'Vorschau · Freigabe · Zeitplan': 'Preview · approval · schedule',
  Pipeline: 'Pipeline',
  Analytics: 'Analytics',
  Einstellungen: 'Settings',

  // Clip-Status
  Wartend: 'Pending',
  Aufbereitet: 'Prepared',
  Freigabe: 'Approval',
  Bearbeitung: 'Editing',
  // Der Clip-Status. 'Freigegeben' ist bewusst NICHT dieser Schluessel: das ist
  // der Admin-Knopf fuer den Dashboard-Zugang mit anderer Uebersetzung.
  'Clip freigegeben': 'Approved',
  Übersprungen: 'Skipped',
  'Wird gepostet': 'Publishing',
  Teilveröffentlicht: 'Partly published',
  Veröffentlicht: 'Published',
  Verworfen: 'Discarded',
  Fehler: 'Failed',
  Alle: 'All',

  // Retention
  überfällig: 'overdue',
  heute: 'today',
  morgen: 'tomorrow',
  '{days} Tage': '{days} days',

  // KPIs
  'alle Stati': 'all states',
  'Vollständig veröffentlicht': 'Fully published',
  'im geladenen Clip-Pool': 'in the loaded clip pool',
  'Manuelle Uploads': 'Manual uploads',
  'MP4-Drops aus dem Editor': 'MP4 drops from the editor',
  'Früheste Frist der geladenen Clips': 'Earliest deadline among loaded clips',
  'Offene Clips bleiben erhalten': 'Open clips are retained',

  // Pipeline-Liste
  'Aktualisiere…': 'Refreshing…',
  '{count} Treffer': '{count} results',
  'Keine Clips für diesen Filter': 'No clips for this filter',
  'Clips werden geladen…': 'Loading clips…',
  'Die Clip-Liste konnte nicht geladen werden.': 'The clip list could not be loaded.',
  'Sobald neue Twitch-Clips eingehen oder du eine MP4 hochlädst, erscheinen sie hier.':
    'As soon as new Twitch clips arrive or you upload an MP4, they show up here.',
  'Speichern fehlgeschlagen: {message}': 'Saving failed: {message}',
  'Layout gespeichert.': 'Layout saved.',
  'Layout wird geladen…': 'Loading layout…',
  'Das gespeicherte Layout konnte nicht geladen werden.':
    'The saved layout could not be loaded.',
  'Default für {streamer} speichern': 'Save default for {streamer}',

  // Upload-Karte
  'MP4 hochladen': 'Upload MP4',
  'Bitte eine MP4-Datei wählen.': 'Please choose an MP4 file.',
  'Die MP4-Datei darf höchstens 200 MB groß sein.': 'The MP4 file must not exceed 200 MB.',
  'MP4 für {streamer} auswählen oder hier ablegen':
    'Choose an MP4 for {streamer} or drop it here',
  'Die Datei wird sicher gespeichert und anschließend mit dem Streamer-Default-Layout aufbereitet.':
    'The file is stored securely and then prepared using the streamer default layout.',
  'MP4 hier ablegen': 'Drop MP4 here',
  'MP4 hier ablegen oder auswählen für {streamer}':
    'Drop MP4 here or select one for {streamer}',
  'oder klicken zum Auswählen · max 200 MB': 'or click to browse · max 200 MB',
  'Datei wird unter': 'The file is stored in',
  'abgelegt und automatisch das Streamer-Default-Layout angewendet.':
    'and the streamer default layout is applied automatically.',
  'Upload läuft…': 'Uploading…',
  'Upload erfolgreich. Clip ist in der Pipeline.': 'Upload complete. The clip is in the pipeline.',
  'Fristprüfung ab 14 Tagen; offene Clips bleiben erhalten':
    'Retention check after 14 days; open clips are retained',
  'Auto-Apply: Streamer-Default-Layout': 'Auto-apply: streamer default layout',

  // Plattform-Namen im Metadaten-Panel
  'YouTube Shorts': 'YouTube Shorts',
  TikTok: 'TikTok',
  'Instagram Reels': 'Instagram Reels',

  // Einstellungen: Verbindungen
  Verbindungen: 'Connections',
  verbunden: 'connected',
  'nicht verbunden': 'not connected',
  Trennen: 'Disconnect',
  Verbinden: 'Connect',

  // Einstellungen: VOD-Archiv
  'VOD-Archiv': 'VOD archive',
  'Automatisch sichern': 'Save automatically',
  'Sichtbarkeit auf YouTube': 'Visibility on YouTube',
  '· YouTube erzwingt privat, bis das Google-Projekt auditiert ist':
    '· YouTube forces private until the Google project has passed its audit',
  Privat: 'Private',
  'Nicht gelistet': 'Unlisted',
  Öffentlich: 'Public',

  // Einstellungen: Sprache
  Sprache: 'Language',
  'Gilt für dieses Dashboard in diesem Browser. Nicht übersetzte Stellen bleiben auf Deutsch.':
    'Applies to this dashboard in this browser. Anything not translated stays in German.',

  // Clip-Karte
  Upload: 'Upload',
  'Keine Vorschau': 'No preview',
  'Original ansehen': 'Watch original',
  Twitch: 'Twitch',
  '{views} Aufrufe': '{views} views',
  'Override aktiv': 'Override active',
  'Status: {state}': 'Status: {state}',
  'Wartet auf Freigabe': 'Waiting for approval',
  'In Bearbeitung': 'Being edited',
  'Bereit zur Prüfung, sobald die Metadaten abgeschlossen sind.':
    'Ready for review once the metadata is complete.',
  Posten: 'Post',
  Bearbeiten: 'Edit',
  Überspringen: 'Skip',
  Original: 'Original',
  // Der Knopf an der Clip-Karte; 'Verworfen' oben ist der Status dazu.
  Verwerfen: 'Discard',
  Metadaten: 'Metadata',
  Layout: 'Layout',
  'Clip "{title}" verwerfen?': 'Discard clip "{title}"?',
  'Override speichern': 'Save override',
  Schließen: 'Close',
  'Override entfernen und Streamer-Default verwenden?':
    'Remove the override and use the streamer default?',
  'Override entfernen → Streamer-Default': 'Remove override → streamer default',

  // Plattformfreie Clip-Aufbereitung
  'Clip-Aufbereitung': 'Clip preparation',
  'Clip-Aufbereitung für {title}': 'Clip preparation for {title}',
  '{title}: {status}': '{title}: {status}',
  Aufbereitung: 'Preparation',
  Aufbereitungsfortschritt: 'Preparation progress',
  'Aufbereitungsstand wird geladen…': 'Loading preparation status…',
  'Aufbereitungsstand wird bei Sichtbarkeit geladen.':
    'Preparation status loads when the card becomes visible.',
  'Aufbereitungsstand konnte nicht geladen werden.': 'Preparation status could not be loaded.',
  Quelle: 'Source',
  Hochformat: 'Portrait render',
  Vorschau: 'Preview',
  'Clip aufbereiten': 'Prepare clip',
  'Neu rendern': 'Render again',
  'Aufbereitung läuft…': 'Preparing clip…',
  'Gerenderte 9:16-Vorschau prüfen': 'Review rendered 9:16 preview',
  Öffnen: 'Open',
  'Gerenderte Vorschau für {title}': 'Rendered preview for {title}',
  'Für diese Vorschau stehen noch keine synchronisierten Untertitel bereit.':
    'Synchronized captions are not available for this preview yet.',
  'MP4 herunterladen': 'Download MP4',
  'Die Vorschau ist fertig, aber nicht abrufbar. Bitte neu rendern.':
    'The preview is ready but cannot be opened. Please render it again.',
  'Die gerenderte Vorschau konnte nicht abgespielt werden. Bitte neu rendern.':
    'The rendered preview could not be played. Please render it again.',
  'Das Quellvideo ist nicht mehr erreichbar.': 'The source video is no longer available.',
  'Das Quellvideo konnte nicht vorbereitet werden.': 'The source video could not be prepared.',
  'Das Quellvideo konnte nicht heruntergeladen werden.': 'The source video could not be downloaded.',
  'Die Hochformat-Vorschau konnte nicht gerendert werden.':
    'The portrait preview could not be rendered.',
  'Die gerenderte Vorschau ist nicht mehr erreichbar.':
    'The rendered preview is no longer available.',
  'Die Clip-Aufbereitung ist fehlgeschlagen.': 'Clip preparation failed.',
  Läuft: 'Running',
  'Aufbereitung wartet auf den Start.': 'Preparation is waiting to start.',
  'Quellvideo wird vorbereitet.': 'The source video is being prepared.',
  'Quellvideo ist bereit.': 'The source video is ready.',
  'Hochformat-Vorschau wird gerendert.': 'The portrait preview is being rendered.',
  'Die prüfbare Vorschau ist bereit.': 'The reviewable preview is ready.',
  'Die Aufbereitung ist fehlgeschlagen.': 'Preparation failed.',
  'Für später freigeben': 'Approve for later',
  'Zur Veröffentlichung freigeben': 'Approve for publication',
  'Wähle mindestens eine Zielplattform.': 'Select at least one destination.',
  'Freigabemodus wird geladen…': 'Loading approval mode…',
  'Freigabemodus wird geladen.': 'Approval mode is loading.',
  'Bereite den Clip zuerst auf und prüfe die Vorschau.':
    'Prepare the clip and review the preview first.',
  'Mindestens ein laufender Upload konnte möglicherweise nicht mehr gestoppt werden. Prüfe die Zielplattformen.':
    'At least one running upload may no longer have been stoppable. Check the destination platforms.',
  'Ergebnis unklar': 'Unclear result',
  'Der Plattformaufruf wurde gestartet, aber sein Ergebnis konnte nicht sicher bestätigt werden. Prüfe zuerst die Zielplattform.':
    'The platform request started, but its result could not be confirmed safely. Check the destination platform first.',
  'Plattform-ID': 'Platform ID',
  'Hast du den Clip auf allen genannten Plattformen als veröffentlicht geprüft?':
    'Have you verified that the clip is published on every listed platform?',
  'Nach Plattformprüfung bestätigen': 'Confirm after platform check',
  'Nur die Verwaltung kann diesen Plattformstand bestätigen.':
    'Only an administrator can confirm this platform status.',
  'Die Abgleichdaten sind unvollständig. Lade die Clip-Liste neu.':
    'The reconciliation data is incomplete. Reload the clip list.',
  'Der Plattformstand wurde abgeglichen.': 'The platform status has been reconciled.',
  '{platform} wurde getrennt.': '{platform} was disconnected.',
  'Die geplante Veröffentlichung wurde gestoppt.': 'The scheduled publication was stopped.',
  'Der Clip wurde freigegeben.': 'The clip was approved.',
  'Der Clip wurde übersprungen.': 'The clip was skipped.',
  'Der Plattformstand konnte nicht abgeglichen werden.':
    'The platform status could not be reconciled.',
  'Die lokale Vorschau verändert keinen Plattformstand.':
    'The local preview does not change any platform status.',

  // -- Analytics-Tab ------------------------------------------------------
  'Phase 3 · Performance': 'Phase 3 · performance',
  'Analytics je Clip und Plattform': 'Analytics per clip and platform',
  'Clip für Analytics auswählen': 'Select a clip for analytics',
  'Kein Clip verfügbar': 'No clip available',
  'Veröffentlichte Clips werden geladen…': 'Loading published clips…',
  'Analytics werden geladen…': 'Loading analytics…',
  'Noch keine veröffentlichten Clips mit Plattform-ID vorhanden.':
    'No published clips with a platform ID yet.',
  'Tastaturhilfe: Diagramm mit Tab fokussieren, mit Pfeil links und rechts Werte durchgehen und mit Enter Details ein- oder ausblenden. Alle Werte stehen zusätzlich in der Datentabelle.':
    'Keyboard help: focus a chart with Tab, move through values with the left and right arrow keys, and show or hide details with Enter. Every value is also available in the data table.',
  'Balkendiagramm mit den Aufrufen auf YouTube, TikTok und Instagram für 24 Stunden, 7 Tage und 30 Tage.':
    'Bar chart of YouTube, TikTok, and Instagram views after 24 hours, 7 days, and 30 days.',
  'Liniendiagramm mit den Engagement-Raten auf YouTube, TikTok und Instagram für 24 Stunden, 7 Tage und 30 Tage.':
    'Line chart of YouTube, TikTok, and Instagram engagement rates after 24 hours, 7 days, and 30 days.',
  'Datentabelle zu den Diagrammen anzeigen': 'Show the charts as a data table',
  'Analytics-Werte des ausgewählten Clips': 'Analytics values for the selected clip',
  Zeitraum: 'Period',
  'YouTube-Aufrufe': 'YouTube views',
  'TikTok-Aufrufe': 'TikTok views',
  'Instagram-Aufrufe': 'Instagram views',
  'YouTube-Engagement': 'YouTube engagement',
  'TikTok-Engagement': 'TikTok engagement',
  'Instagram-Engagement': 'Instagram engagement',
  'Aufrufe nach Zeitraum': 'Views by period',
  'Engagement-Rate': 'Engagement rate',
  'LLM-Reports': 'LLM reports',
  Streamer: 'Streamer',
  'Wochenreport für {streamer}': 'Weekly report for {streamer}',
  Cross: 'Cross',
  'Monatsreport über alle Streamer': 'Monthly report across all streamers',
  'Report wird generiert…': 'Generating report…',
  'Der Report wurde erstellt.': 'The report was created.',
  'Letzter Streamer-Report': 'Latest streamer report',
  'Letzter Admin-DM-Stand': 'Latest admin DM',
  'Gespeicherte Reports': 'Saved reports',
  'Gespeicherte Reports werden geladen…': 'Loading saved reports…',
  '{count} Einträge': '{count} entries',
  'Noch keine Reports gespeichert.': 'No reports saved yet.',
  'Zeitraum: {from} bis {to}': 'Period: {from} to {to}',

  // -- Layout-Editor ------------------------------------------------------
  'Layout-Editor': 'Layout editor',
  'Vorschau-Clip': 'Preview clip',
  'Gilt nur für diesen Clip.': 'Applies to this clip only.',
  'Layout für {title} bearbeiten': 'Edit layout for {title}',
  'Metadaten für {title} bearbeiten': 'Edit metadata for {title}',
  'Ohne Bild (Muster)': 'No image (pattern)',
  'Der Ausschnitt gilt danach für alle Clips dieses Kanals.':
    'The crop then applies to every clip on this channel.',
  PiP: 'PiP',
  Stacked: 'Stacked',
  'Cam an': 'Cam on',
  'Cam aus': 'Cam off',
  'Quelle · Twitch-Bild': 'Source · Twitch frame',
  'Was aus dem Twitch-Bild ausgeschnitten wird.': 'What gets cropped out of the Twitch frame.',
  'Game-Ausschnitt': 'Game crop',
  'Cam-Ausschnitt': 'Cam crop',
  'Ziel · Hochformat 9:16': 'Target · portrait 9:16',
  'Wo der Cam-Ausschnitt im fertigen Video landet.':
    'Where the cam crop ends up in the finished video.',
  'Cam ist aus: das Game füllt das ganze Bild.': 'Cam is off: the game fills the whole frame.',
  'Cam-Kachel frei ziehen und an den Ecken skalieren: {box}.':
    'Drag the cam tile freely and scale it at the corners: {box}.',
  'Cam-Streifen oben, Höhe an der Unterkante ziehen: {height} von maximal {max} px.':
    'Cam band on top, drag the bottom edge for its height: {height} of {max} px max.',
  'Auf Default zurücksetzen': 'Reset to default',
  Zurücksetzen: 'Reset',
  'Als Standard speichern': 'Save as default',
  'Speichert…': 'Saving…',
  'Twitch-Bild 16:9 · {width}×{height}': 'Twitch frame 16:9 · {width}×{height}',
  'Hochformat · {width}×{height}': 'Portrait · {width}×{height}',
  'Höhe {height}': 'Height {height}',
  Game: 'Game',
  'Game füllt das Bild': 'Game fills the frame',
  'Cam-Streifen': 'Cam band',
  'Cam-Kachel': 'Cam tile',

  // -- Enrichment-Panel ---------------------------------------------------
  Wartet: 'Waiting',
  Transkribiert: 'Transcribing',
  'Wörterbuch-Korrektur': 'Dictionary pass',
  'LLM-Hashtags': 'LLM hashtags',
  Fertig: 'Done',
  'Automatik nicht verfügbar': 'Automation unavailable',
  'Neu generieren': 'Regenerate',
  'Enrichment-Panel schließen': 'Close enrichment panel',
  'Metadaten konnten nicht geladen werden.': 'Metadata could not be loaded.',
  'Metadaten werden geladen…': 'Loading metadata…',
  'Speichere oder verwirf zuerst deine Änderungen.': 'Save or discard your changes first.',
  'Die automatische Anreicherung ist für diesen Kanal noch nicht verfügbar. Du kannst die Metadaten manuell bearbeiten oder es später erneut versuchen.':
    'Automatic enrichment is not available for this channel yet. You can edit the metadata manually or try again later.',
  Titel: 'Title',
  'Erkannte Begriffe': 'Detected terms',
  'Transkript anzeigen': 'Show transcript',
  'Ungesicherte Änderungen': 'Unsaved changes',
  'Synchron mit Server': 'In sync with server',
  Speichern: 'Save',
  'Gespeichert.': 'Saved.',
  Beschreibung: 'Description',
  Hashtags: 'Hashtags',
  '{count} · Ziel {target}': '{count} · target {target}',
  '{platform}-Title…': '{platform} title…',
  'Kurze Beschreibung für {platform}…': 'Short description for {platform}…',
  'Hashtag eingeben + Enter…': 'Type a hashtag + Enter…',
  'Hashtag #{tag} entfernen': 'Remove hashtag #{tag}',
  'Korrigiere die markierten Plattformgrenzen, bevor du speicherst.':
    'Correct the highlighted platform limits before saving.',
  'Der Titel darf höchstens {count} Zeichen lang sein.':
    'The title may contain at most {count} characters.',
  'Die Beschreibung darf höchstens {count} Zeichen lang sein.':
    'The description may contain at most {count} characters.',
  'Für {platform} sind höchstens {count} Hashtags erlaubt.':
    '{platform} allows at most {count} hashtags.',
  'Ein Hashtag darf höchstens {count} Zeichen lang sein.':
    'A hashtag may contain at most {count} characters.',
  'Pfeiltasten verschieben. Umschalttaste plus Pfeiltasten ändert die Größe. Alt macht kleine Schritte.':
    'Arrow keys move the frame. Shift plus arrow keys resize it. Alt uses small steps.',
  'Pfeil hoch und runter ändert die Höhe. Alt macht kleine Schritte.':
    'The up and down arrow keys change the height. Alt uses small steps.',
  'Tastatur: Rahmen fokussieren, mit Pfeiltasten verschieben und mit Umschalttaste plus Pfeiltasten skalieren.':
    'Keyboard: focus a frame, move it with the arrow keys, and resize it with Shift plus the arrow keys.',

  // -- Kategorien (deutsch geseedet, Anzeige laeuft ueber den Schluessel) ----
  Deadlock: 'Deadlock',
  'Andere Spiele': 'Other games',

  // -- Zeitplan: Zeitzone, Kadenz, Feldpruefung -----------------------------
  'Zeitzone des Kanals': 'Channel time zone',
  'Gilt, sobald Auto-Posting an ist.': 'Applies as soon as auto-posting is on.',
  'Mindestens eine Uhrzeit angeben.': 'Enter at least one time.',
  'Höchstens zwölf Uhrzeiten.': 'Twelve times at most.',
  'Uhrzeiten im Format 18:00 angeben.': 'Enter times as 18:00.',
  'Diese Uhrzeit gibt es nicht.': 'That time does not exist.',
  'Bitte eine Zahl angeben.': 'Please enter a number.',
  'Bitte eine ganze Zahl angeben.': 'Please enter a whole number.',
  'Zwischen 0 und 70 Posts pro Woche angeben.':
    'Enter between 0 and 70 posts per week.',
  'Zwischen 0 und 10 Posts pro Tag angeben.':
    'Enter between 0 and 10 posts per day.',

  // -- Nachschub aus Twitch -------------------------------------------------
  'Clips jetzt holen': 'Fetch clips now',
  '{count} Clips von Twitch geholt.': 'Fetched {count} clips from Twitch.',

  // -- Karten ohne geladenen Stand ------------------------------------------
  'Gespeicherter Stand nicht abrufbar': 'Saved settings could not be loaded',
  'Solange bleibt diese Karte gesperrt, damit nichts Falsches gespeichert wird. Bitte die Seite neu laden.':
    'Until then this card stays locked so nothing wrong gets saved. Please reload the page.',
  'Zustand unbekannt': 'State unknown',
  'Verbindungen werden geladen…': 'Loading connections…',

  // -- Rueckmeldung nach dem OAuth-Umweg ------------------------------------
  '{platform} ist jetzt verbunden.': '{platform} is connected now.',
  'Das Konto ist jetzt verbunden.': 'The account is connected now.',
  'Verbinden hat nicht geklappt': 'Connecting did not work',
  'Die Plattform hat die Verbindung abgelehnt.': 'The platform refused the connection.',
  'Die Antwort der Plattform passte nicht zur Anfrage. Bitte neu verbinden.':
    'The reply from the platform did not match the request. Please connect again.',
  'Der Zugang konnte nicht abgeholt werden. Bitte neu verbinden.':
    'The access token could not be fetched. Please connect again.',
  'Die Verbindung konnte nicht abgeschlossen werden. Bitte neu verbinden.':
    'The connection could not be completed. Please connect again.',

  // -- Verbindungen: Ablauf und Sammelverbindung ----------------------------
  'Zugang abgelaufen, bitte neu verbinden': 'Access expired, please reconnect',
  'nutzt die Sammelverbindung': 'uses the shared connection',
  'Veröffentlichung wartet auf Plattformfreigabe.':
    'Publishing is waiting for platform approval.',
  'Zugang läuft am {datum} ab.': 'Access expires on {datum}.',
  'Neu verbinden': 'Reconnect',
  'Trennen: {platform} für {streamer}': 'Disconnect: {platform} for {streamer}',
  'Verbinden: {platform} für {streamer}': 'Connect: {platform} for {streamer}',
  'Neu verbinden: {platform} für {streamer}': 'Reconnect: {platform} for {streamer}',
  '{platform} für {streamer} trennen?': 'Disconnect {platform} for {streamer}?',
  '{platform} für {streamer} trennen? Der Kanal nutzt die Sammelverbindung.':
    'Disconnect {platform} for {streamer}? This channel uses the shared connection.',

  // -- Geplante Posts und Veto ----------------------------------------------
  Eingeplant: 'Scheduled',
  'Doch nicht posten': 'Do not post after all',
  '{count} geplante Posts gestoppt.': 'Stopped {count} scheduled posts.',
  'Auf {platforms} passiert nichts, dort steht die Kadenz auf null.':
    'Nothing happens on {platforms}, the cadence there is set to zero.',
  'Gestoppt, aber {count} Plattform war schon durch.':
    'Stopped, but {count} platform had already gone out.',

  // -- Reports --------------------------------------------------------------

  // -- Fehlermeldungen (stabile Codes aus dem API-Modul) --------------------
  'Dafür fehlt deinem Zugang die Berechtigung.':
    'Your account is not allowed to do that.',
  'Dieser Kanal ist für Social Media noch nicht freigeschaltet.':
    'This channel has not been enabled for social media yet.',
  'Für diese Aktion fehlt der Kanal.': 'This action needs a channel.',
  'Diesen Kanal gibt es nicht.': 'That channel does not exist.',
  'Diese Entscheidung passt nicht mehr zum Zustand des Clips.':
    'That decision no longer matches the state of the clip.',
  'Diese Plattformen stehen auf null Posts und sind damit ausgeschaltet: {details}. Stell im Zeitplan eine Kadenz ein oder gib eine andere Plattform frei.':
    'These platforms are set to zero posts and are therefore switched off: {details}. Set a cadence in the schedule or approve a different platform.',
  'Die Entscheidung konnte nicht gespeichert werden.': 'The decision could not be saved.',
  'Der geplante Post konnte nicht gestoppt werden.':
    'The scheduled post could not be stopped.',
  'Diesen Clip gibt es nicht mehr.': 'That clip no longer exists.',
  'Die Verbindung konnte nicht getrennt werden.': 'The connection could not be removed.',
  'Der Verbindungsstatus ist gerade nicht abrufbar.':
    'The connection status cannot be loaded right now.',
  'Das Speichern hat nicht geklappt.': 'Saving did not work.',
  'Der Zeitplan konnte nicht gespeichert werden.': 'The schedule could not be saved.',
  'Dieses Layout ist nicht gültig.': 'This layout is not valid.',
  'Diese Sichtbarkeit gibt es nicht.': 'That visibility does not exist.',
  'Diese Eingabe konnte das Backend nicht verarbeiten.':
    'The backend could not process this input.',
  'Twitch antwortet gerade nicht. Bitte später erneut versuchen.':
    'Twitch is not responding right now. Please try again later.',
  'Der Clip konnte nicht eingereiht werden.': 'The clip could not be queued.',
  'Der Report konnte nicht erzeugt werden.': 'The report could not be generated.',
  'Die Datei ist zu groß, höchstens 200 MB.': 'The file is too large, 200 MB at most.',
  'Falsches Dateiformat, bitte eine MP4 wählen.': 'Wrong file format, please pick an MP4.',
  'Dieser Clip liegt schon im Pool.': 'This clip is already in the pool.',
  'Ein anderes Video wird gerade verarbeitet. Bitte versuche es gleich erneut.':
    'Another video is being processed. Please try again shortly.',
  'Die Upload-Daten sind unvollständig oder ungültig.':
    'The upload data is incomplete or invalid.',
  'Ein Textfeld enthält ungültige Zeichen.': 'A text field contains invalid characters.',
  'Ein Textfeld ist zu lang.': 'A text field is too long.',
  'Die MP4-Datei ist leer.': 'The MP4 file is empty.',
  'Bitte wähle eine MP4-Datei aus.': 'Please choose an MP4 file.',
  'Ein Formularfeld wurde mehrfach gesendet.': 'A form field was sent more than once.',
  'Das Formular enthält ein unbekanntes Feld.': 'The form contains an unknown field.',
  'Es werden nur MP4-Videos unterstützt.': 'Only MP4 videos are supported.',
  'Die Datei ist kein gültiges MP4-Video.': 'The file is not a valid MP4 video.',
  'Das Video muss eine positive Laufzeit haben.': 'The video must have a positive duration.',
  'Das Video darf höchstens 300 Sekunden lang sein.':
    'The video must not exceed 300 seconds.',
  'Die Bildrate des Videos wird nicht unterstützt.':
    'The video frame rate is not supported.',
  'Die Videoauflösung wird nicht unterstützt.': 'The video resolution is not supported.',
  'Der Kanalname ist ungültig.': 'The channel name is invalid.',
  'Der Upload konnte nicht sicher gespeichert werden.':
    'The upload could not be stored safely.',
  'Der Speicherstand des Uploads ist unklar. Bitte nicht erneut hochladen und zuerst den Clip-Pool prüfen.':
    'The upload storage state is uncertain. Do not upload it again; check the clip pool first.',
  'Der Upload ist fehlgeschlagen.': 'The upload failed.',
  'Für diese Plattform fehlt die Clip-Freigabe.':
    'This clip is not approved for the platform.',
  'Die Clip-Freigabe konnte nicht sicher geprüft werden.':
    'The clip approval could not be verified safely.',
  'Der Clip wurde seit der Freigabe geändert. Bitte Vorschau erneut prüfen und freigeben.':
    'The clip changed after approval. Please review and approve the preview again.',
  'Dieser Clip wurde nicht zur Veröffentlichung freigegeben.':
    'This clip was not approved for publication.',
  'Veröffentlichungen sind im Testbetrieb ausgeschaltet.':
    'Publications are disabled in test mode.',
  'Der Freigabemodus konnte nicht sicher geprüft werden.':
    'The release mode could not be verified safely.',
  'Die Veröffentlichung auf dieser Plattform ist noch nicht freigeschaltet.':
    'Publishing to this platform has not been enabled yet.',
  'TikTok braucht für diesen Clip eigene Veröffentlichungseinstellungen und eine ausdrückliche Zustimmung.':
    'TikTok requires dedicated publishing settings and explicit consent for this clip.',
  'Das vorbereitete Video erfüllt die Plattformregeln nicht.':
    'The prepared video does not meet the platform rules.',
  'Die Plattform hat die Veröffentlichung abgelehnt.':
    'The platform rejected the publication.',
  'Die Plattform hat den Clip möglicherweise schon angenommen. Erst abgleichen, nicht erneut veröffentlichen.':
    'The platform may already have accepted the clip. Reconcile it first; do not publish it again.',
  'Dieser ältere Uploadversuch muss vor einem neuen Versuch manuell abgeglichen werden.':
    'This older upload attempt must be reconciled manually before another attempt.',
  'Der Plattformstand wurde manuell abgeglichen.':
    'The platform status was reconciled manually.',
  'Der vorhandene Plattformstand konnte nicht sicher übernommen werden.':
    'The existing platform status could not be adopted safely.',
  'Der Clip wurde verworfen.': 'The clip was discarded.',
  'Für diese Plattform fehlt eine gültige Verbindung.':
    'This platform does not have a valid connection.',
  'Der zum Clip gehörende Kanal fehlt.': 'The channel belonging to this clip is missing.',
  'Der bisherige Veröffentlichungsstand konnte nicht sicher geprüft werden.':
    'The existing publication status could not be verified safely.',
  'Die Aufbereitung ist gerade belegt. Bitte später erneut versuchen.':
    'Preparation is currently busy. Please try again later.',
  'Die Clip-Quelle ist nicht mehr verfügbar.': 'The clip source is no longer available.',
  'Die gespeicherte Clip-Adresse ist ungültig.': 'The stored clip address is invalid.',
  'Die Clip-Quelle konnte nicht geladen werden.': 'The clip source could not be loaded.',
  'Der automatische Abruf ist sicherheitshalber gesperrt. Bitte später erneut versuchen.':
    'Automatic retrieval is locked for safety. Please try again later.',
  'Die Hochformat-Vorschau konnte nicht erstellt werden.':
    'The vertical preview could not be created.',
  'Das gespeicherte Clip-Layout ist ungültig.': 'The stored clip layout is invalid.',
  'Diese Plattform ist im Zeitplan pausiert.': 'This platform is paused in the schedule.',
  'Das hat nicht geklappt.': 'That did not work.',
};

const TRANSLATIONS: Record<Language, Record<string, string>> = {
  de: {},
  en: EN,
};

export type TranslateParams = Record<string, string | number>;

/**
 * Uebersetzt und setzt Platzhalter ein. Ohne Treffer bleibt der deutsche
 * Ausgangstext stehen, deshalb gibt es keinen Leerzustand.
 */
export function translate(
  language: Language,
  text: string,
  params?: TranslateParams,
): string {
  const translated = TRANSLATIONS[language]?.[text] ?? text;
  if (!params) return translated;
  return translated.replace(/\{(\w+)\}/g, (match, key: string) =>
    key in params ? String(params[key]) : match,
  );
}

export function isLanguage(value: unknown): value is Language {
  return value === 'de' || value === 'en';
}

/**
 * Die Wahl liegt im Browser, nicht in der Datenbank: sie ist eine Anzeigesache
 * und soll ohne Backend-Umbau ueber alle Routen dieses Bundles gelten.
 * Gesperrter Speicher (privates Fenster) darf die Oberflaeche nicht kippen,
 * deshalb faellt beides still auf Deutsch beziehungsweise auf "nicht gemerkt".
 */
export function readStoredLanguage(): Language {
  if (typeof window === 'undefined') return DEFAULT_LANGUAGE;
  try {
    const stored = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
    return isLanguage(stored) ? stored : DEFAULT_LANGUAGE;
  } catch {
    return DEFAULT_LANGUAGE;
  }
}

export function storeLanguage(language: Language): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
  } catch {
    // Nicht speichern zu koennen ist kein Grund, die Umschaltung zu blocken.
  }
}

/** Nur fuer Tests und Werkzeuge: das rohe Woerterbuch einer Sprache. */
export function dictionaryFor(language: Language): Record<string, string> {
  return TRANSLATIONS[language] ?? {};
}
