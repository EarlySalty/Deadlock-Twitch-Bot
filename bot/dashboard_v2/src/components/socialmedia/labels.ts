import type {
  ApprovalMode,
  ApprovalState,
  ClipStatus,
  EnrichmentStatus,
  SocialMediaReportKind,
} from '@/types/socialMedia';

export type LabelTone = 'orange' | 'messing' | 'success' | 'warning' | 'danger' | 'muted';

export const TONE_BADGE: Record<LabelTone, string> = {
  orange: 'bg-orange/15 text-orange border-orange/35',
  messing: 'bg-accent/15 text-accent border-accent/35',
  success: 'bg-success/15 text-success border-success/35',
  warning: 'bg-warning/15 text-warning border-warning/35',
  danger: 'bg-danger/15 text-danger border-danger/35',
  muted: 'bg-bg/60 text-text-secondary border-border',
};

export const STATUS_LABELS: Record<ClipStatus, { label: string; tone: LabelTone }> = {
  pending: { label: 'Wartend', tone: 'muted' },
  enriched: { label: 'Aufbereitet', tone: 'messing' },
  awaiting_approval: { label: 'Freigabe', tone: 'orange' },
  approved: { label: 'Clip freigegeben', tone: 'success' },
  editing: { label: 'Bearbeitung', tone: 'warning' },
  skipped: { label: 'Übersprungen', tone: 'muted' },
  publishing: { label: 'Wird gepostet', tone: 'orange' },
  published_partial: { label: 'Teilveröffentlicht', tone: 'warning' },
  published_all: { label: 'Veröffentlicht', tone: 'success' },
  discarded: { label: 'Verworfen', tone: 'muted' },
  failed: { label: 'Fehler', tone: 'danger' },
};

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

export const APPROVAL_STATE_LABELS: Record<ApprovalState, string> = {
  awaiting_approval: 'Wartet auf Freigabe',
  approved: 'Clip freigegeben',
  skipped: 'Übersprungen',
  editing: 'In Bearbeitung',
};

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

export const STATUS_META: Record<EnrichmentStatus, { label: string; tone: LabelTone }> = {
  pending: { label: 'Wartet', tone: 'muted' },
  transcribing: { label: 'Transkribiert', tone: 'orange' },
  correcting: { label: 'Wörterbuch-Korrektur', tone: 'orange' },
  llm: { label: 'LLM-Hashtags', tone: 'messing' },
  done: { label: 'Fertig', tone: 'success' },
  failed: { label: 'Fehler', tone: 'danger' },
  skipped_no_key: { label: 'API-Key fehlt', tone: 'muted' },
};

export const REPORT_KIND_LABELS: Record<SocialMediaReportKind, string> = {
  streamer: 'Streamer',
  cross: 'Cross',
  admin: 'Admin',
};

export const ZUGRIFF_LABELS = {
  granted: 'Freigegeben',
  grant: 'Freigeben',
} as const;

export const KATEGORIE_LABELS: Record<string, string> = {
  deadlock: 'Deadlock',
  other: 'Andere Spiele',
};

export function kategorieLabel(categoryKey: string, displayName: string): string {
  return KATEGORIE_LABELS[categoryKey] ?? displayName;
}

export type SocialMediaView = 'heute' | 'clips' | 'planen' | 'vod' | 'einstellungen';
export type SocialMediaViewTarget = SocialMediaView | 'freigabe' | 'veroeffentlicht' | 'pool' | 'plan' | 'library' | 'layout' | 'konten' | 'archiv' | 'ai';

export const SOCIAL_MEDIA_TABS: Array<{ id: SocialMediaView; label: string }> = [
  { id: 'heute', label: 'Heute' },
  { id: 'planen', label: 'Planen' },
  { id: 'clips', label: 'Clips' },
  { id: 'vod', label: 'VOD-Archiv' },
  { id: 'einstellungen', label: 'Einstellungen' },
];

export const PLATFORM_LABELS: Record<string, string> = {
  youtube: 'YouTube',
  tiktok: 'TikTok',
  instagram: 'Instagram',
};

export const UPLOAD_WAIT_TEXTE: Record<string, string> = {
  connection_missing: '{platform} nicht verbunden. Öffne Verbindungen und verbinde dein Konto.',
  connection_incomplete: 'Die Verbindung zu {platform} ist unvollständig. Öffne Verbindungen und verbinde dein Konto erneut.',
  platform_unavailable: '{platform} ist in dieser Beta noch nicht verfügbar.',
  connection_required: 'Der Upload auf {platform} wartet auf eine verfügbare Verbindung. Öffne Verbindungen und prüfe dein Konto.',
};

export function uploadWaitText(
  reason: string | null | undefined,
  platform: string,
  t: (text: string, params?: Record<string, string | number>) => string,
): string {
  return t(UPLOAD_WAIT_TEXTE[reason ?? ''] ?? UPLOAD_WAIT_TEXTE.connection_required, {
    platform: PLATFORM_LABELS[platform] ?? platform,
  });
}

export const FELD_FEHLER = {
  zeitLeer: 'Mindestens eine Uhrzeit angeben.',
  zuVieleZeiten: 'Höchstens zwölf Uhrzeiten.',
  zeitFormat: 'Uhrzeiten im Format 18:00 angeben.',
  zeitUngueltig: 'Diese Uhrzeit gibt es nicht.',
  keineZahl: 'Bitte eine Zahl angeben.',
} as const;

export const FEHLER_TEXTE: Record<string, string> = {
  admin_required: 'Dafür fehlt deinem Zugang die Berechtigung.',
  partner_access_required: 'Dieser Kanal ist für Social Media noch nicht freigeschaltet.',
  streamer_required: 'Für diese Aktion fehlt der Kanal.',
  unknown_streamer: 'Diesen Kanal gibt es nicht.',
  invalid_decision: 'Diese Entscheidung passt nicht mehr zum Zustand des Clips.',
  only_paused_platforms:
    'Diese Plattformen stehen auf null Posts und sind damit ausgeschaltet: {details}. Stell im Zeitplan eine Kadenz ein oder gib eine andere Plattform frei.',
  approval_decision_failed: 'Die Entscheidung konnte nicht gespeichert werden.',
  approval_cancel_failed: 'Der geplante Post konnte nicht gestoppt werden.',
  clip_not_found: 'Diesen Clip gibt es nicht mehr.',
  disconnect_failed: 'Die Verbindung konnte nicht getrennt werden.',
  own_connection_required: 'Dieser Kanal nutzt ein gemeinsames Konto. Verbinde ein eigenes Konto, um es hier verwalten zu können.',
  platform_unavailable: 'Diese Plattform ist in dieser Beta noch nicht verfügbar.',
  platform_status_failed: 'Der Verbindungsstatus ist gerade nicht abrufbar.',
  provider_error: 'Die Plattform hat die Verbindung abgelehnt.',
  invalid_callback: 'Die Antwort der Plattform passte nicht zur Anfrage. Bitte neu verbinden.',
  token_exchange_failed: 'Der Zugang konnte nicht abgeholt werden. Bitte neu verbinden.',
  callback_failed: 'Die Verbindung konnte nicht abgeschlossen werden. Bitte neu verbinden.',
  save_failed: 'Das Speichern hat nicht geklappt.',
  schedule_failed: 'Der Zeitplan konnte nicht gespeichert werden.',
  invalid_layout: 'Dieses Layout ist nicht gültig.',
  invalid_privacy: 'Diese Sichtbarkeit gibt es nicht.',
  invalid_payload: 'Diese Eingabe konnte das Backend nicht verarbeiten.',
  twitch_api_unavailable: 'Twitch antwortet gerade nicht. Bitte später erneut versuchen.',
  queue_failed: 'Der Clip konnte nicht eingereiht werden.',
  pipeline_unified: 'Dieser Kanal veröffentlicht nur noch über den Autopiloten.',
  pipeline_busy: 'Für diesen Clip läuft auf dieser Plattform schon eine Veröffentlichung.',
  report_generation_failed: 'Der Report konnte nicht erzeugt werden.',
  upload_too_large: 'Die Datei ist zu groß, höchstens 200 MB.',
  upload_wrong_format: 'Falsches Dateiformat, bitte eine MP4 wählen.',
  upload_duplicate: 'Dieser Clip liegt schon im Pool.',
  duplicate_clip_id: 'Dieser Clip liegt schon im Pool.',
  upload_failed: 'Der Upload ist fehlgeschlagen.',
  unbekannt: 'Das hat nicht geklappt.',
};

export function plattformnamenLesbar(details: string): string {
  return details
    .split(',')
    .map((teil) => teil.trim())
    .filter((teil) => teil !== '')
    .map((teil) => PLATFORM_LABELS[teil.toLowerCase()] ?? teil)
    .join(', ');
}

export function fehlerText(
  error: unknown,
  t: (text: string, params?: Record<string, string | number>) => string,
): string | null {
  if (!error) return null;
  const code = (error as { code?: string }).code;
  const message = (error as { message?: string }).message;
  if (code && FEHLER_TEXTE[code]) {
    const details = message && message !== code ? plattformnamenLesbar(message) : '';
    return t(FEHLER_TEXTE[code], { details });
  }
  if (message && FEHLER_TEXTE[message]) return t(FEHLER_TEXTE[message]);
  if (message && !/^[a-z0-9_]+$/.test(message)) return message;
  return t(FEHLER_TEXTE.unbekannt);
}
