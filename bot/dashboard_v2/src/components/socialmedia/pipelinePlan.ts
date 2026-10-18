import type {
  DistributionPolicy, PipelineMode, PipelinePolicy, PolicyPreview, PolicyScope, ProductionHold, ReviewPolicy,
  SchedulePatch, ScheduleQueueApi, ScheduleQueueSnapshot, TextDefaults, WeeklySlot,
} from './autopilot/schedule-queue/api';
export const PLAN_PLATFORMS = [
  { id: 'youtube', label: 'YouTube' },
  { id: 'tiktok', label: 'TikTok' },
  { id: 'instagram', label: 'Instagram' },
] as const;

export const WEEKDAYS = ['Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag', 'Sonntag'];
const SUGGESTED_TIMES = ['18:00', '20:00', '12:00', '08:00', '22:00', '16:00', '10:00', '14:00'];

export const PATHS: { id: 'template' | 'ai'; title: string; text: string }[] = [
  { id: 'template', title: 'Weg A: Standard-Vorlage (schnell & kostenlos)', text: 'Der Clip wird mit deinem Layout und deinen Standardtexten hochgeladen.' },
  { id: 'ai', title: 'Weg B: Mit KI-Bearbeitung', text: 'Die KI schreibt die Texte und schneidet auf Wunsch auch den Clip.' },
];

export const AI_MODES: { id: PipelineMode; title: string; text: string }[] = [
  { id: 'upload_text', title: 'Nur KI-Texte', text: 'Der Clip bleibt unverändert. Die KI schreibt Titel, Beschreibung und Hashtags.' },
  { id: 'ai_edit', title: 'KI-Schnitt + Untertitel', text: 'Die KI schneidet den Clip, setzt Untertitel und schreibt die Texte.' },
];

export const REVIEWS: { id: ReviewPolicy; title: string; text: string }[] = [
  { id: 'off', title: 'Vollautomatisch', text: 'Fertige Clips werden ohne deine Freigabe eingeplant.' },
  { id: 'required', title: 'Freigabe nötig', text: 'Nichts geht raus, bevor du es freigibst.' },
  { id: 'veto_until_deadline', title: 'Veto bis Frist', text: 'Geht zur Frist automatisch raus, wenn du nicht stoppst.' },
];

export const SCOPES: { id: PolicyScope; title: string }[] = [
  { id: 'shared', title: 'Gemeinsamer Plan (Standard)' },
  { id: 'per_platform', title: 'Getrennt je Plattform' },
];

export const TAB_ORDER = ['tiktok', 'youtube', 'instagram'];

export const TEXT_SCOPES: { id: PolicyScope; title: string; text: string }[] = [
  { id: 'shared', title: 'Gleich für alle', text: 'Alle Plattformen bekommen dieselben Standardtexte.' },
  { id: 'per_platform', title: 'Je Plattform', text: 'Jede Plattform bekommt eigene Standardtexte. Leere Felder nutzen die gemeinsamen Texte.' },
];

export const HOLDS: Record<Exclude<ProductionHold, null>, string> = {
  disabled: 'Der Auto-Pilot ist ausgeschaltet.',
  paused: 'Pausiert. Laufende Produktionen werden fertiggestellt.',
  review_backlog: 'Es warten Clips auf deine Freigabe. Danach geht es weiter.',
  empty_pool: 'Keine passenden Clips verfügbar. Prüfe deine Clips oder den Zeitraum.',
  no_schedule: 'Füge mindestens eine Zeit hinzu.',
  outside_lead: 'Der nächste freie Termin liegt noch außerhalb des Vorlaufs.',
};

export const DISTRIBUTION_NOTES: Record<DistributionPolicy, string> = {
  same_clip: 'Aktiv: Derselbe Clip wird auf allen aktiven Plattformen veröffentlicht (empfohlen).',
  independent_clips: 'Aktiv: Jede Plattform zieht sich einen eigenen, anderen Clip aus dem Vorrat (verbraucht mehr Clips).',
};

export function planPreview(snapshot: ScheduleQueueSnapshot, policy: DistributionPolicy): PolicyPreview {
  return snapshot.policy_previews?.[policy] ?? {
    next_slot: snapshot.next_slot,
    next_slots: snapshot.next_slots ?? {},
    production_hold: snapshot.production_hold,
  };
}

export const PLAN_EN: Record<string, string> = {
  'Speichere zuerst deine Änderungen, damit der Testlauf sie verwendet.': 'Save your changes first so the test run uses them.',
  'Planen': 'Plan',
  'Schritt {number}': 'Step {number}',
  'Postingplan': 'Posting plan',
  'Wann soll gepostet werden?': 'When should clips be posted?',
  'Noch keine Plattform für Auto-Posting aktiv. Aktiviere sie unter Einstellungen bei den Konten.': 'No platform is active for auto-posting yet. Turn one on under Settings in the accounts list.',
  'Video-Vorbereitung': 'Video preparation',
  'Wie sollen Clips geschnitten & betextet werden?': 'How should clips be cut and captioned?',
  'Weg A: Standard-Vorlage (schnell & kostenlos)': 'Option A: Standard template (fast & free)',
  'Der Clip wird mit deinem Layout und deinen Standardtexten hochgeladen.': 'The clip is uploaded with your layout and your default text.',
  'Weg B: Mit KI-Bearbeitung': 'Option B: With AI editing',
  'Die KI schreibt die Texte und schneidet auf Wunsch auch den Clip.': 'AI writes the text and can also edit the clip.',
  'Was soll die KI übernehmen?': 'What should AI take care of?',
  'Nur KI-Texte': 'AI text only',
  'KI-Schnitt + Untertitel': 'AI edit + subtitles',
  'Die KI schneidet den Clip, setzt Untertitel und schreibt die Texte.': 'AI edits the clip, adds subtitles and writes the text.',
  'Standardtexte & Hashtags (Fallback)': 'Default text & hashtags (fallback)',
  'Kontrolle': 'Review',
  'Prüfen oder automatisch posten?': 'Review first or post automatically?',
  'Vollautomatisch': 'Fully automatic',
  'Freigabe nötig': 'Approval required',
  'Lege fest, wie deine Clips vorbereitet werden, wer sie freigibt und wann sie erscheinen.': 'Choose how your clips are prepared, who approves them and when they go out.',
  'Plan wird geladen.': 'Loading plan.',
  'Erneut laden': 'Reload',
  'Der Clip bleibt unverändert. Die KI schreibt Titel, Beschreibung und Hashtags.': 'The clip stays unchanged. AI writes the title, description and hashtags.',
  'Änderungen gelten für neue Clips.': 'Changes apply to new clips.',
  'Fertige Clips werden ohne deine Freigabe eingeplant.': 'Finished clips are scheduled without your approval.',
  'Nichts geht raus, bevor du es freigibst.': 'Nothing goes out before you approve it.',
  'Veto bis Frist': 'Veto until deadline',
  'Geht zur Frist automatisch raus, wenn du nicht stoppst.': 'Goes out automatically at the deadline unless you stop it.',
  'Clips, die schon auf deine Freigabe warten, bleiben dort, bis du entscheidest.': 'Clips already waiting for your approval stay there until you decide.',
  'Je Plattform': 'Per platform',
  'Zeitzone: {timezone}': 'Time zone: {timezone}',
  'Zeitzone in Verwaltung ändern': 'Change time zone in Settings',
  'Sommerzeit wird berücksichtigt. Verpasste Termine werden nicht nachgeholt.': 'Daylight saving time is handled. Missed times are not made up.',
  'Zeit hinzufügen': 'Add time',
  '{day}: Zeit hinzufügen': '{day}: add time',
  '{day} {time} entfernen': 'Remove {day} {time}',
  'Uhrzeit am {day}': 'Time on {day}',
  'Keine Zeit': 'No time',
  'Nächster Termin': 'Next time',
  'Gemeinsamer Plan (Standard)': 'Shared plan (default)',
  'Getrennt je Plattform': 'Separate per platform',
  'Postingplan-Art': 'Posting plan type',
  'Plattform wählen': 'Choose platform',
  'Vorlauf': 'Lead time',
  'h vor Termin': 'h before the slot',
  'Unterschiedliche Clips je Plattform posten': 'Post different clips on each platform',
  'Aktiv: Derselbe Clip wird auf allen aktiven Plattformen veröffentlicht (empfohlen).': 'Active: The same clip is published on all active platforms (recommended).',
  'Aktiv: Jede Plattform zieht sich einen eigenen, anderen Clip aus dem Vorrat (verbraucht mehr Clips).': 'Active: Each platform takes its own, different clip from the pool (uses more clips).',
  'Zeiten für {platform}': 'Times for {platform}',
  'Alle Plattformen': 'All platforms',
  'Nächster Termin je Plattform': 'Next slot per platform',
  '{slot}, gemeinsam': '{slot}, shared',
  'Noch kein Termin': 'No time yet',
  'Termin nicht verfügbar': 'Time unavailable',
  'Ungespeicherte Änderungen': 'Unsaved changes',
  'Speichern': 'Save',
  'Wird gespeichert': 'Saving',
  'Verwerfen': 'Discard',
  'Gespeichert.': 'Saved.',
  'Gespeicherten Stand laden': 'Load saved settings',
  'Jede Zeit braucht eine Uhrzeit.': 'Every time needs a clock time.',
  '{day} {time} ist doppelt.': '{day} {time} is listed twice.',
  'Die Zeiten aller Plattformen wurden zusammengeführt. Mit „Verwerfen“ kannst du das rückgängig machen.': 'The times of all platforms were merged. Use “Discard” to undo this.',
  'Der Vorlauf muss zwischen 0 und 720 Stunden liegen.': 'Lead time must be between 0 and 720 hours.',
  'Bitte melde dich erneut an.': 'Please sign in again.',
  'Du hast keinen Zugriff auf diesen Kanal.': 'You do not have access to this channel.',
  'Der Plan ist gerade nicht verfügbar. Bitte versuche es später erneut.': 'The plan is unavailable right now. Please try again later.',
  'Die Verbindung ist unterbrochen. Bitte versuche es erneut.': 'The connection was interrupted. Please try again.',
  'Die Änderung konnte nicht gespeichert werden. Bitte versuche es erneut.': 'The change could not be saved. Please try again.',
  'Die Einstellungen wurden inzwischen woanders geändert. Lade den gespeicherten Stand und trage deine Änderung erneut ein.': 'The settings were changed elsewhere in the meantime. Load the saved settings and enter your change again.',
  'Ein Teil ist gespeichert, der Rest nicht. Speichere erneut oder lade den gespeicherten Stand.': 'Part of it was saved, the rest was not. Save again or load the saved settings.',
  'Der Auto-Pilot ist ausgeschaltet.': 'Autopilot is off.',
  'Pausiert. Laufende Produktionen werden fertiggestellt.': 'Paused. Running productions will finish.',
  'Es warten Clips auf deine Freigabe. Danach geht es weiter.': 'Clips are waiting for your approval. It continues afterwards.',
  'Keine passenden Clips verfügbar. Prüfe deine Clips oder den Zeitraum.': 'No matching clips available. Check your clips or the date range.',
  'Füge mindestens eine Zeit hinzu.': 'Add at least one time.',
  'Der nächste freie Termin liegt noch außerhalb des Vorlaufs.': 'The next free time is still outside the lead time.',
  'Werden genutzt, wenn ein Clip keinen eigenen Titel, keine Beschreibung oder keine Hashtags hat.': 'Used when a clip has no title, description or hashtags of its own.',
  'Gleich für alle': 'Same for all',
  'Alle Plattformen bekommen dieselben Standardtexte.': 'All platforms get the same default text.',
  'Jede Plattform bekommt eigene Standardtexte. Leere Felder nutzen die gemeinsamen Texte.': 'Each platform gets its own default text. Empty fields use the shared text.',
  'Für alle Plattformen': 'For all platforms',
  'Titel': 'Title',
  'Beschreibung': 'Description',
  'Hashtags': 'Hashtags',
  'Mit Leerzeichen trennen, zum Beispiel #deadlock #clips': 'Separate with spaces, for example #deadlock #clips',
  'Montag': 'Monday',
  'Dienstag': 'Tuesday',
  'Mittwoch': 'Wednesday',
  'Donnerstag': 'Thursday',
  'Freitag': 'Friday',
  'Samstag': 'Saturday',
  'Sonntag': 'Sunday',
};

export type PlanText = (text: string, params?: Record<string, string | number>) => string;

export function planText(language: string): PlanText {
  return (text, params) => {
    const base = language === 'en' ? PLAN_EN[text] ?? text : text;
    return params ? base.replace(/\{(\w+)\}/g, (match, key: string) => key in params ? String(params[key]) : match) : base;
  };
}

export interface PlanDraft {
  lead_time_hours: number;
  target_platforms: string[];
  pipeline_mode: PipelineMode;
  review_policy: ReviewPolicy;
  schedule_scope: PolicyScope;
  distribution_policy: DistributionPolicy;
  metadata_scope: PolicyScope;
  texts: Record<string, TextDraft>;
  shared: WeeklySlot[];
  per_platform: Record<string, WeeklySlot[]>;
}

export interface TextDraft {
  title: string;
  description: string;
  hashtags: string;
}

export const SHARED_TEXT = 'shared';

function textDraft(text: TextDefaults): TextDraft {
  return { title: text.title ?? '', description: text.description ?? '', hashtags: (text.hashtags ?? []).map(tag => `#${tag}`).join(' ') };
}

export function parseHashtags(value: string): string[] {
  return [...new Set(value.split(/[\s,]+/).map(tag => tag.replace(/^#+/, '').trim()).filter(Boolean))];
}

function textDefaults(text: TextDraft): TextDefaults {
  return { title: text.title.trim(), description: text.description.trim(), hashtags: parseHashtags(text.hashtags) };
}

function emptyText(text: TextDefaults): boolean {
  return !text.title && !text.description && text.hashtags.length === 0;
}

function textsFromPolicy(policy: PipelinePolicy): Record<string, TextDraft> {
  const defaults = policy.text_defaults ?? { shared: { title: '', description: '', hashtags: [] }, per_platform: {} };
  return {
    [SHARED_TEXT]: textDraft(defaults.shared),
    ...Object.fromEntries(Object.entries(defaults.per_platform ?? {}).map(([platform, text]) => [platform, textDraft(text)])),
  };
}

function policyTexts(texts: Record<string, TextDraft>): PipelinePolicy['text_defaults'] {
  const shared = textDefaults(texts[SHARED_TEXT] ?? { title: '', description: '', hashtags: '' });
  const per_platform = Object.fromEntries(Object.entries(texts)
    .filter(([platform]) => platform !== SHARED_TEXT)
    .map(([platform, text]) => [platform, textDefaults(text)] as const)
    .filter(([, text]) => !emptyText(text))
    .sort(([a], [b]) => a.localeCompare(b)));
  return { shared, per_platform };
}

export interface PlanState {
  snapshot: ScheduleQueueSnapshot;
  policy: PipelinePolicy;
}

export function normalizeSlots(slots: WeeklySlot[]): WeeklySlot[] {
  const sorted = [...slots].sort((a, b) => a.weekday - b.weekday || a.local_time.localeCompare(b.local_time));
  return sorted.filter((slot, index) => index === 0 || slot.weekday !== sorted[index - 1].weekday || slot.local_time !== sorted[index - 1].local_time);
}

function normalizePerPlatform(value: Record<string, WeeklySlot[]>): Record<string, WeeklySlot[]> {
  return Object.fromEntries(Object.entries(value).map(([platform, slots]) => [platform, normalizeSlots(slots)]).filter(([, slots]) => slots.length > 0).sort(([a], [b]) => String(a).localeCompare(String(b))));
}

export function planDraft({ snapshot, policy }: PlanState): PlanDraft {
  return {
    lead_time_hours: snapshot.settings.lead_time_hours,
    target_platforms: [...snapshot.settings.target_platforms],
    pipeline_mode: policy.pipeline_mode,
    review_policy: policy.review_policy,
    schedule_scope: policy.schedule_scope,
    distribution_policy: policy.distribution_policy,
    metadata_scope: policy.metadata_scope,
    texts: textsFromPolicy(policy),
    shared: structuredClone(snapshot.settings.schedule_slots),
    per_platform: structuredClone(policy.platform_schedule_slots),
  };
}

function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function schedulePatch(base: PlanState, draft: PlanDraft): SchedulePatch {
  const settings = base.snapshot.settings;
  const patch: SchedulePatch = {};
  if (draft.lead_time_hours !== settings.lead_time_hours) patch.lead_time_hours = draft.lead_time_hours;
  if (!sameJson(normalizeSlots(draft.shared), normalizeSlots(settings.schedule_slots))) patch.schedule_slots = normalizeSlots(draft.shared);
  const aiWanted = draft.pipeline_mode === 'ai_edit';
  if (draft.pipeline_mode !== base.policy.pipeline_mode && aiWanted !== settings.ai_editing_enabled) patch.ai_editing_enabled = aiWanted;
  return patch;
}

export function policyChange(base: PlanState, draft: PlanDraft): PipelinePolicy | null {
  const policy = base.policy;
  const perPlatform = normalizePerPlatform(draft.per_platform);
  const texts = policyTexts(draft.texts);
  const unchanged = draft.pipeline_mode === policy.pipeline_mode && draft.review_policy === policy.review_policy
    && draft.schedule_scope === policy.schedule_scope && draft.distribution_policy === policy.distribution_policy
    && sameJson(perPlatform, normalizePerPlatform(policy.platform_schedule_slots))
    && draft.metadata_scope === policy.metadata_scope && sameJson(texts, policyTexts(textsFromPolicy(policy)));
  if (unchanged) return null;
  return {
    ...policy,
    pipeline_mode: draft.pipeline_mode,
    review_policy: draft.review_policy,
    schedule_scope: draft.schedule_scope,
    distribution_policy: draft.distribution_policy,
    platform_schedule_slots: perPlatform,
    metadata_scope: draft.metadata_scope,
    text_defaults: texts,
  };
}

export function isDirty(base: PlanState, draft: PlanDraft): boolean {
  return Object.keys(schedulePatch(base, draft)).length > 0 || policyChange(base, draft) !== null;
}

function editedLists(draft: PlanDraft): { platform?: string; slots: WeeklySlot[] }[] {
  if (draft.schedule_scope === 'shared') return [{ slots: draft.shared }];
  return draft.target_platforms.map(platform => ({ platform, slots: draft.per_platform[platform] ?? [] }));
}

const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

function slotProblems(slots: WeeklySlot[], text: PlanText): string[] {
  const problems: string[] = [];
  if (slots.some(slot => !TIME_PATTERN.test(slot.local_time))) problems.push(text('Jede Zeit braucht eine Uhrzeit.'));
  const seen = new Set<string>();
  for (const slot of slots) {
    const key = `${slot.weekday}-${slot.local_time}`;
    if (seen.has(key)) problems.push(text('{day} {time} ist doppelt.', { day: text(WEEKDAYS[slot.weekday - 1]), time: slot.local_time }));
    seen.add(key);
  }
  return problems;
}

export function planPlatformLabel(platform: string): string {
  return PLAN_PLATFORMS.find(item => item.id === platform)?.label ?? platform;
}

export function validateDraft(draft: PlanDraft, text: PlanText): string[] {
  const problems: string[] = [];
  if (!Number.isFinite(draft.lead_time_hours) || draft.lead_time_hours < 0 || draft.lead_time_hours > 720) problems.push(text('Der Vorlauf muss zwischen 0 und 720 Stunden liegen.'));
  for (const { platform, slots } of editedLists(draft)) {
    for (const problem of slotProblems(slots, text)) {
      problems.push(platform ? `${planPlatformLabel(platform)}: ${problem}` : problem);
    }
  }
  return [...new Set(problems)];
}

export function invalidPlatforms(draft: PlanDraft): string[] {
  return editedLists(draft).flatMap(({ platform, slots }) => platform && slotProblems(slots, value => value).length ? [platform] : []);
}

export function normalizeTime(value: string): string {
  const match = value.trim().match(/^(\d{1,2})(?:[:.]?(\d{2}))?$/);
  if (!match) return value.trim();
  return `${match[1].padStart(2, '0')}:${match[2] ?? '00'}`;
}

export function addTime(slots: WeeklySlot[], weekday: number): WeeklySlot[] {
  const used = new Set(slots.filter(slot => slot.weekday === weekday).map(slot => slot.local_time));
  const local_time = SUGGESTED_TIMES.find(time => !used.has(time)) ?? '00:00';
  return [...slots, { weekday, local_time }];
}

export function switchScope(draft: PlanDraft, scope: PolicyScope): PlanDraft {
  if (scope === draft.schedule_scope) return draft;
  if (scope === 'per_platform') {
    const per_platform = { ...draft.per_platform };
    for (const platform of draft.target_platforms) {
      if (!per_platform[platform]?.length) per_platform[platform] = structuredClone(draft.shared);
    }
    return { ...draft, schedule_scope: 'per_platform', per_platform };
  }
  const union = normalizeSlots(draft.target_platforms.flatMap(platform => draft.per_platform[platform] ?? []));
  return { ...draft, schedule_scope: 'shared', shared: union.length ? union : draft.shared };
}

export function scopeMergeChanges(draft: PlanDraft): boolean {
  if (draft.schedule_scope !== 'per_platform') return false;
  const merged = switchScope(draft, 'shared').shared;
  return draft.target_platforms.some(platform => !sameJson(normalizeSlots(draft.per_platform[platform] ?? []), normalizeSlots(merged)));
}

export interface SaveOutcome {
  state: PlanState;
  failure?: unknown;
  partial: boolean;
}

export async function savePlan(api: ScheduleQueueApi, owner: string | undefined, base: PlanState, draft: PlanDraft): Promise<SaveOutcome> {
  const patch = schedulePatch(base, draft);
  const policy = policyChange(base, draft);
  let state = base;
  let saved = false;
  try {
    if (Object.keys(patch).length) {
      state = { ...state, snapshot: await api.save(patch, owner) };
      saved = true;
    }
    if (policy) {
      state = { ...state, policy: await api.savePolicy(policy, owner) };
      saved = true;
      state = await api.load(owner).then(snapshot => ({ ...state, snapshot }), () => state);
    }
    return { state, partial: false };
  } catch (failure) {
    return { state, failure, partial: saved && isDirty(state, draft) };
  }
}

export async function loadPlan(api: ScheduleQueueApi, owner?: string, signal?: AbortSignal): Promise<PlanState> {
  const [snapshot, policy] = await Promise.all([api.load(owner, signal), api.loadPolicy(owner, signal)]);
  return { snapshot, policy };
}

export function failureText(error: unknown): string {
  const status = typeof error === 'object' && error !== null && 'status' in error ? Number((error as { status: unknown }).status) : 0;
  const message = error instanceof Error ? error.message : '';
  if (status === 401) return 'Bitte melde dich erneut an.';
  if (status === 403) return 'Du hast keinen Zugriff auf diesen Kanal.';
  if (message.includes('zwischenzeitlich geändert')) return 'Die Einstellungen wurden inzwischen woanders geändert. Lade den gespeicherten Stand und trage deine Änderung erneut ein.';
  if (status === 400 && message && !message.startsWith('Server-Fehler')) return message;
  if (status >= 500) return 'Der Plan ist gerade nicht verfügbar. Bitte versuche es später erneut.';
  if (error instanceof TypeError) return 'Die Verbindung ist unterbrochen. Bitte versuche es erneut.';
  return 'Die Änderung konnte nicht gespeichert werden. Bitte versuche es erneut.';
}
