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
  'Social Media für': 'Social media for',

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
  'Clips gehen ohne Sichtung raus.': 'Clips go out without review.',
  'Zeiten gelten in {tz}.': 'Times apply in {tz}.',
  'Automatisch posten': 'Post automatically',
  'Posts pro Woche': 'Posts per week',
  'Höchstens pro Tag': 'At most per day',
  'Uhrzeiten, mit Komma getrennt': 'Times, comma separated',
  'Nächster Post: {termin}': 'Next post: {termin}',
  'Mit Titel- und Hashtag-Vorschlägen.': 'With title and hashtag suggestions.',
  'Ohne Vorschläge, Clip geht so raus.': 'No suggestions, the clip goes out as is.',
  'Vorrat reicht noch für {posts} Posts.': 'Enough clips left for {posts} posts.',
  '{clips} Clips im Pool.': '{clips} clips in the pool.',
  '{clips} Clips im Pool, das sind rund {tage} Tage bei {proWoche} Posts pro Woche.':
    '{clips} clips in the pool, about {tage} days at {proWoche} posts per week.',
  'Twitch-Clips werden automatisch eingesammelt, vertikal aufbereitet und für YT Shorts / TikTok / Reels vorbereitet. Layouts pro Streamer als Default, pro Clip override-bar, 14-Tage-Retention.':
    'Twitch clips are collected automatically, converted to vertical format and prepared for YT Shorts, TikTok and Reels. Layouts are a per-streamer default, can be overridden per clip, and are kept for 14 days.',
  'Layout: Repo-Default aktiv': 'Layout: repo default active',
  'Layout: Streamer-Default': 'Layout: streamer default',
  'Phase 3 · Analytics + LLM-Reports': 'Phase 3 · analytics + LLM reports',
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
  'Heute veröffentlicht': 'Published today',
  'über alle Plattformen': 'across all platforms',
  'Manuelle Uploads': 'Manual uploads',
  'MP4-Drops aus dem Editor': 'MP4 drops from the editor',
  'Nächste Retention': 'Next retention',
  '14-Tage-Lifecycle': '14-day lifecycle',

  // Pipeline-Liste
  'Aktualisiere…': 'Refreshing…',
  '{count} Treffer': '{count} results',
  'Keine Clips für diesen Filter': 'No clips for this filter',
  'Sobald neue Twitch-Clips eingehen oder du eine MP4 hochlädst, erscheinen sie hier.':
    'As soon as new Twitch clips arrive or you upload an MP4, they show up here.',
  'Speichern fehlgeschlagen: {message}': 'Saving failed: {message}',
  'Layout gespeichert.': 'Layout saved.',
  'Default für {streamer} speichern': 'Save default for {streamer}',

  // Upload-Karte
  'MP4 hochladen': 'Upload MP4',
  'Bitte eine MP4-Datei wählen.': 'Please choose an MP4 file.',
  'MP4 hier ablegen': 'Drop MP4 here',
  'oder klicken zum Auswählen · max 200 MB': 'or click to browse · max 200 MB',
  'Datei wird unter': 'The file is stored in',
  'abgelegt und automatisch das Streamer-Default-Layout angewendet.':
    'and the streamer default layout is applied automatically.',
  'Upload läuft…': 'Uploading…',
  'Upload erfolgreich. Clip ist in der Pipeline.': 'Upload complete. The clip is in the pipeline.',
  'Retention: 14 Tage ab Erstellung': 'Retention: 14 days from creation',
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
  Twitch: 'Twitch',
  '{views} Views': '{views} views',
  'Override aktiv': 'Override active',
  Approval: 'Approval',
  'Status: {state}': 'Status: {state}',
  'Wartet auf Freigabe': 'Waiting for approval',
  'In Bearbeitung': 'Being edited',
  'Wird nach abgeschlossenem Enrichment per DM freigegeben.':
    'Approved by DM once enrichment has finished.',
  Posten: 'Post',
  Bearbeiten: 'Edit',
  Skip: 'Skip',
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

  // -- Analytics-Tab ------------------------------------------------------
  'Phase 3 · Performance': 'Phase 3 · performance',
  'Analytics je Clip und Plattform': 'Analytics per clip and platform',
  'Noch keine veroeffentlichten Clips mit Plattform-ID vorhanden.':
    'No published clips with a platform ID yet.',
  'Views nach Bucket': 'Views by bucket',
  'Engagement-Rate': 'Engagement rate',
  'LLM-Reports': 'LLM reports',
  Streamer: 'Streamer',
  'Wochenreport fuer {streamer}': 'Weekly report for {streamer}',
  Cross: 'Cross',
  'Monatsreport ueber alle Streamer': 'Monthly report across all streamers',
  'Report wird generiert…': 'Generating report…',
  'Letzter Streamer-Report': 'Latest streamer report',
  'Letzter Admin-DM-Stand': 'Latest admin DM',
  'Gespeicherte Reports': 'Saved reports',
  '{count} Eintraege': '{count} entries',
  'Noch keine Reports gespeichert.': 'No reports saved yet.',
  'Zeitraum: {from} bis {to}': 'Period: {from} to {to}',

  // -- Layout-Editor ------------------------------------------------------
  'Layout-Editor': 'Layout editor',
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
  'API-Key fehlt': 'API key missing',
  'Neu generieren': 'Regenerate',
  'Enrichment-Panel schließen': 'Close enrichment panel',
  'Enrichment wurde übersprungen, weil kein LLM-Key gesetzt ist ({keys}). Setze einen Key und drücke „Neu generieren".':
    'Enrichment was skipped because no LLM key is set ({keys}). Set a key and press "Regenerate".',
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

  // -- Nachschub aus Twitch -------------------------------------------------
  'Clips jetzt holen': 'Fetch clips now',
  '{count} Clips von Twitch geholt.': 'Fetched {count} clips from Twitch.',

  // -- Karten ohne geladenen Stand ------------------------------------------
  'Gespeicherter Stand nicht abrufbar': 'Saved settings could not be loaded',
  'Solange bleibt diese Karte gesperrt, damit nichts Falsches gespeichert wird. Bitte die Seite neu laden.':
    'Until then this card stays locked so nothing wrong gets saved. Please reload the page.',
  'Zustand unbekannt': 'State unknown',

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
  'Zugang läuft am {datum} ab.': 'Access expires on {datum}.',
  'Neu verbinden': 'Reconnect',
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
  'Der Upload ist fehlgeschlagen.': 'The upload failed.',
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
