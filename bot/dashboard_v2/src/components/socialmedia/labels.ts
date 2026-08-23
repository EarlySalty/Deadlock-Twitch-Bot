/**
 * Beschriftungen der Social-Media-Oberflaeche an einer Stelle.
 *
 * Warum ein eigenes Modul: die Tabellen wurden vorher in zwei Dateien doppelt
 * gepflegt. Dabei ist ein deutscher Text einmal als Clip-Status und einmal als
 * Admin-Knopf benutzt worden, mit zwei verschiedenen englischen Uebersetzungen.
 * Ein Text darf genau eine Bedeutung haben, deshalb steht jede Tabelle hier
 * einmal und wird importiert statt kopiert.
 *
 * Der deutsche Text ist zugleich der Uebersetzungsschluessel. Jeder Eintrag
 * hier muss also in `dictionaryFor('en')` stehen; der Vertragstest
 * `tests/socialMediaContract.test.ts` prueft das.
 */
import type {
  ApprovalMode,
  ApprovalState,
  ClipStatus,
  EnrichmentStatus,
} from '@/types/socialMedia';

export type LabelTone = 'orange' | 'teal' | 'success' | 'warning' | 'danger' | 'muted';

export const TONE_BADGE: Record<LabelTone, string> = {
  orange: 'bg-orange/15 text-orange border-orange/35',
  teal: 'bg-teal/15 text-teal border-teal/35',
  success: 'bg-success/15 text-success border-success/35',
  warning: 'bg-warning/15 text-warning border-warning/35',
  danger: 'bg-danger/15 text-danger border-danger/35',
  muted: 'bg-bg/60 text-text-secondary border-border',
};

/** Status eines Clips in der Pipeline. */
export const STATUS_LABELS: Record<ClipStatus, { label: string; tone: LabelTone }> = {
  pending: { label: 'Wartend', tone: 'muted' },
  enriched: { label: 'Aufbereitet', tone: 'teal' },
  awaiting_approval: { label: 'Freigabe', tone: 'orange' },
  // Bewusst nicht 'Freigegeben': das ist der Admin-Knopf fuer den Zugang zum
  // Dashboard und hat eine andere Uebersetzung.
  approved: { label: 'Clip freigegeben', tone: 'success' },
  editing: { label: 'Bearbeitung', tone: 'warning' },
  skipped: { label: 'Übersprungen', tone: 'muted' },
  publishing: { label: 'Wird gepostet', tone: 'orange' },
  published_partial: { label: 'Teilveröffentlicht', tone: 'warning' },
  published_all: { label: 'Veröffentlicht', tone: 'success' },
  discarded: { label: 'Verworfen', tone: 'muted' },
  failed: { label: 'Fehler', tone: 'danger' },
};

/** Der Filter ueber dem Clip-Pool. Beschriftung kommt aus STATUS_LABELS, damit
 *  ein Status nicht an zwei Stellen anders heisst. */
export const ALLE_LABEL = 'Alle';

export const STATUS_FILTER_IDS: Array<ClipStatus | 'all'> = [
  'pending',
  'enriched',
  'awaiting_approval',
  'published_all',
  'discarded',
  'all',
];

export function statusFilterLabel(id: ClipStatus | 'all'): string {
  return id === 'all' ? ALLE_LABEL : STATUS_LABELS[id].label;
}

/** Zustand der Freigabe-Entscheidung eines Clips. */
export const APPROVAL_STATE_LABELS: Record<ApprovalState, string> = {
  awaiting_approval: 'Wartet auf Freigabe',
  approved: 'Clip freigegeben',
  skipped: 'Übersprungen',
  editing: 'In Bearbeitung',
};

/** Beschriftung und Erklaerung der drei Freigabe-Modi. */
export const APPROVAL_MODE_TEXTE: Record<ApprovalMode, { label: string; hinweis: string }> = {
  manual: {
    label: 'Nur nach Freigabe',
    hinweis: 'Jeder Clip wartet auf dein Okay.',
  },
  veto_window: {
    label: 'Einspruch bis zum Termin',
    hinweis: 'Clips werden eingeplant. Du kannst sie bis zum Posting stoppen.',
  },
  full_auto: {
    label: 'Vollautomatik',
    hinweis: 'Clips gehen ohne Sichtung raus.',
  },
};

/** Stand der Metadaten-Aufbereitung eines Clips. */
export const STATUS_META: Record<EnrichmentStatus, { label: string; tone: LabelTone }> = {
  pending: { label: 'Wartet', tone: 'muted' },
  transcribing: { label: 'Transkribiert', tone: 'orange' },
  correcting: { label: 'Wörterbuch-Korrektur', tone: 'orange' },
  llm: { label: 'LLM-Hashtags', tone: 'teal' },
  done: { label: 'Fertig', tone: 'success' },
  failed: { label: 'Fehler', tone: 'danger' },
  skipped_no_key: { label: 'API-Key fehlt', tone: 'muted' },
};

/** Zugang zum eigenen Dashboard, nicht zu verwechseln mit dem Clip-Status. */
export const ZUGRIFF_LABELS = {
  granted: 'Freigegeben',
  grant: 'Freigeben',
} as const;

/**
 * Die Kategorien kommen deutsch aus der Datenbank ('Deadlock', 'Andere
 * Spiele'). Damit die englische Oberflaeche nicht deutsch bleibt, laeuft die
 * Anzeige ueber den Schluessel statt ueber `display_name`.
 */
export const KATEGORIE_LABELS: Record<string, string> = {
  deadlock: 'Deadlock',
  other: 'Andere Spiele',
};

/** Fuer unbekannte Kategorien bleibt der Name aus der Datenbank stehen. */
export function kategorieLabel(categoryKey: string, displayName: string): string {
  return KATEGORIE_LABELS[categoryKey] ?? displayName;
}

/** Die vier Bereiche der Seite, in der Reihenfolge des Clip-Weges. */
export type SocialMediaView = 'konten' | 'plan' | 'pool' | 'veroeffentlicht';

export const SOCIAL_MEDIA_TABS: Array<{ id: SocialMediaView; label: string }> = [
  { id: 'pool', label: 'Clip-Pool' },
  { id: 'plan', label: 'Zeitplan' },
  { id: 'veroeffentlicht', label: 'Veröffentlicht' },
  { id: 'konten', label: 'Konten' },
];

/** Markennamen, deshalb ohne Uebersetzung. */
export const PLATFORM_LABELS: Record<string, string> = {
  youtube: 'YouTube',
  tiktok: 'TikTok',
  instagram: 'Instagram',
};

/**
 * Meldungen, die direkt am Eingabefeld stehen. Als Tabelle, damit der
 * Vertragstest sie mitprueft; ein Satz mitten im Code faellt dabei durch.
 */
export const FELD_FEHLER = {
  zeitLeer: 'Mindestens eine Uhrzeit angeben.',
  zuVieleZeiten: 'Höchstens zwölf Uhrzeiten.',
  zeitFormat: 'Uhrzeiten im Format 18:00 angeben.',
  zeitUngueltig: 'Diese Uhrzeit gibt es nicht.',
  keineZahl: 'Bitte eine Zahl angeben.',
} as const;

/**
 * Fehlercodes des Backends in Saetze, die ein Streamer versteht. Der rohe Code
 * (`invalid_decision`) hat in der Oberflaeche nichts verloren.
 */
export const FEHLER_TEXTE: Record<string, string> = {
  // Zugang
  admin_required: 'Dafür fehlt deinem Zugang die Berechtigung.',
  partner_access_required: 'Dieser Kanal ist für Social Media noch nicht freigeschaltet.',
  streamer_required: 'Für diese Aktion fehlt der Kanal.',
  unknown_streamer: 'Diesen Kanal gibt es nicht.',
  // Freigabe und Veto
  invalid_decision: 'Diese Entscheidung passt nicht mehr zum Zustand des Clips.',
  approval_decision_failed: 'Die Entscheidung konnte nicht gespeichert werden.',
  approval_cancel_failed: 'Der geplante Post konnte nicht gestoppt werden.',
  clip_not_found: 'Diesen Clip gibt es nicht mehr.',
  // Verbindungen
  disconnect_failed: 'Die Verbindung konnte nicht getrennt werden.',
  platform_status_failed: 'Der Verbindungsstatus ist gerade nicht abrufbar.',
  // Speichern
  save_failed: 'Das Speichern hat nicht geklappt.',
  schedule_failed: 'Der Zeitplan konnte nicht gespeichert werden.',
  invalid_layout: 'Dieses Layout ist nicht gültig.',
  invalid_privacy: 'Diese Sichtbarkeit gibt es nicht.',
  invalid_payload: 'Diese Eingabe konnte das Backend nicht verarbeiten.',
  // Clips holen und einreihen
  twitch_api_unavailable: 'Twitch antwortet gerade nicht. Bitte später erneut versuchen.',
  queue_failed: 'Der Clip konnte nicht eingereiht werden.',
  report_generation_failed: 'Der Report konnte nicht erzeugt werden.',
  // Upload
  upload_too_large: 'Die Datei ist zu groß, höchstens 200 MB.',
  upload_wrong_format: 'Falsches Dateiformat, bitte eine MP4 wählen.',
  upload_duplicate: 'Dieser Clip liegt schon im Pool.',
  duplicate_clip_id: 'Dieser Clip liegt schon im Pool.',
  upload_failed: 'Der Upload ist fehlgeschlagen.',
  // Letzte Zuflucht
  unbekannt: 'Das hat nicht geklappt.',
};

/**
 * Uebersetzten Satz zu einem Fehler holen. Unbekannte Codes fallen auf die
 * Servermeldung zurueck, damit nie eine leere Fehlerzeile entsteht.
 */
export function fehlerText(
  error: unknown,
  t: (text: string, params?: Record<string, string | number>) => string,
): string | null {
  if (!error) return null;
  const code = (error as { code?: string }).code;
  if (code && FEHLER_TEXTE[code]) return t(FEHLER_TEXTE[code]);
  const message = (error as { message?: string }).message;
  if (message && FEHLER_TEXTE[message]) return t(FEHLER_TEXTE[message]);
  // Ein blosser Code ("invalid_decision") ist keine Meldung fuer Menschen.
  if (message && !/^[a-z0-9_]+$/.test(message)) return message;
  return t(FEHLER_TEXTE.unbekannt);
}
