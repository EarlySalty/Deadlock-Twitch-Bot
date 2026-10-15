import type {
  DistributionPolicy, PipelineMode, PipelinePolicy, PolicyScope, ProductionHold, ReviewPolicy,
  SchedulePatch, ScheduleQueueApi, ScheduleQueueSnapshot, WeeklySlot,
} from './autopilot/schedule-queue/api';
export const PLAN_PLATFORMS = [
  { id: 'youtube', label: 'YouTube' },
  { id: 'tiktok', label: 'TikTok' },
  { id: 'instagram', label: 'Instagram' },
] as const;

export const WEEKDAYS = ['Montag', 'Dienstag', 'Mittwoch', 'Donnerstag', 'Freitag', 'Samstag', 'Sonntag'];
const SUGGESTED_TIMES = ['18:00', '20:00', '12:00', '08:00', '22:00', '16:00', '10:00', '14:00'];

export const MODES: { id: PipelineMode; title: string; text: string }[] = [
  { id: 'upload', title: 'Nur hochladen', text: 'Der Clip geht unverändert raus, mit deinem Titel und deinen Standardtexten.' },
  { id: 'upload_text', title: 'Hochladen + KI-Texte', text: 'Der Clip bleibt unverändert. Die KI schreibt Titel, Beschreibung und Hashtags.' },
  { id: 'ai_edit', title: 'KI-Videoschnitt', text: 'Die KI schneidet den Clip und schreibt die Texte.' },
];

export const REVIEWS: { id: ReviewPolicy; title: string; text: string }[] = [
  { id: 'off', title: 'Aus', text: 'Fertige Clips werden ohne deine Freigabe eingeplant.' },
  { id: 'required', title: 'Freigabe nötig', text: 'Nichts geht raus, bevor du es freigibst.' },
  { id: 'veto_until_deadline', title: 'Veto bis Frist', text: 'Geht zur Frist automatisch raus, wenn du nicht stoppst.' },
];

export const SCOPES: { id: PolicyScope; title: string; text: string }[] = [
  { id: 'shared', title: 'Gemeinsam', text: 'Alle Plattformen posten zu denselben Zeiten.' },
  { id: 'per_platform', title: 'Je Plattform', text: 'Jede Plattform bekommt eigene Zeiten.' },
];

export const DISTRIBUTIONS: { id: DistributionPolicy; title: string; text: string }[] = [
  { id: 'same_clip', title: 'Gleicher Clip auf allen Plattformen', text: 'Jeder Clip erscheint auf allen ausgewählten Plattformen, jeweils zum nächsten freien Termin.' },
  { id: 'independent_clips', title: 'Jede Plattform eigene Clips', text: 'Jede Plattform bekommt ihre eigenen Clips.' },
];

export const HOLDS: Record<Exclude<ProductionHold, null>, string> = {
  disabled: 'Der Auto-Pilot ist ausgeschaltet.',
  paused: 'Pausiert. Laufende Produktionen werden fertiggestellt.',
  review_backlog: 'Es warten Clips auf deine Freigabe. Danach geht es weiter.',
  empty_pool: 'Keine passenden Clips verfügbar. Prüfe deine Clips oder den Zeitraum.',
  no_schedule: 'Füge mindestens eine Zeit hinzu.',
  outside_lead: 'Der nächste freie Termin liegt noch außerhalb des Vorlaufs.',
};

export const PLAN_EN: Record<string, string> = {
  'Planen': 'Plan',
  'Lege fest, wie deine Clips vorbereitet werden, wer sie freigibt und wann sie erscheinen.': 'Choose how your clips are prepared, who approves them and when they go out.',
  'Plan wird geladen.': 'Loading plan.',
  'Erneut laden': 'Reload',
  'Was passiert mit deinen Clips?': 'What happens to your clips?',
  'Nur hochladen': 'Upload only',
  'Der Clip geht unverändert raus, mit deinem Titel und deinen Standardtexten.': 'The clip goes out unchanged, with your title and default text.',
  'Hochladen + KI-Texte': 'Upload + AI text',
  'Der Clip bleibt unverändert. Die KI schreibt Titel, Beschreibung und Hashtags.': 'The clip stays unchanged. AI writes the title, description and hashtags.',
  'KI-Videoschnitt': 'AI video edit',
  'Die KI schneidet den Clip und schreibt die Texte.': 'AI edits the clip and writes the text.',
  'Änderungen gelten für neue Clips.': 'Changes apply to new clips.',
  'Vor Veröffentlichung prüfen': 'Review before publishing',
  'Aus': 'Off',
  'Fertige Clips werden ohne deine Freigabe eingeplant.': 'Finished clips are scheduled without your approval.',
  'Freigabe nötig': 'Approval required',
  'Nichts geht raus, bevor du es freigibst.': 'Nothing goes out before you approve it.',
  'Veto bis Frist': 'Veto until deadline',
  'Geht zur Frist automatisch raus, wenn du nicht stoppst.': 'Goes out automatically at the deadline unless you stop it.',
  'Clips, die schon auf deine Freigabe warten, bleiben dort, bis du entscheidest.': 'Clips already waiting for your approval stay there until you decide.',
  'Zielplattformen': 'Target platforms',
  'nicht verbunden': 'not connected',
  'Verbinde fehlende Plattformen unter Konten.': 'Connect missing platforms under Accounts.',
  'Noch keine Zielplattform ausgewählt. Ohne Zielplattform wird nichts veröffentlicht.': 'No target platform selected yet. Nothing is published without one.',
  'Auto-Pilot': 'Autopilot',
  'Auto-Pilot einschalten': 'Turn on autopilot',
  'Neue Produktionen pausieren': 'Pause new productions',
  'Vorlauf in Stunden': 'Lead time in hours',
  'Wann wird gepostet?': 'When to post?',
  'Gemeinsam': 'Shared',
  'Alle Plattformen posten zu denselben Zeiten.': 'All platforms post at the same times.',
  'Je Plattform': 'Per platform',
  'Jede Plattform bekommt eigene Zeiten.': 'Each platform gets its own times.',
  'Zeitzone: {timezone}': 'Time zone: {timezone}',
  'Zeitzone in Verwaltung ändern': 'Change time zone in Settings',
  'Sommerzeit wird berücksichtigt. Verpasste Termine werden nicht nachgeholt.': 'Daylight saving time is handled. Missed times are not made up.',
  'Wähle zuerst eine Zielplattform aus.': 'Select a target platform first.',
  'Zeit hinzufügen': 'Add time',
  '{day}: Zeit hinzufügen': '{day}: add time',
  '{day} {time} entfernen': 'Remove {day} {time}',
  'Uhrzeit am {day}': 'Time on {day}',
  'Keine Zeit': 'No time',
  'Welche Zeiten sollen für alle gelten?': 'Which times should apply to all platforms?',
  'Die Plattformen haben unterschiedliche Zeiten. Wähle aus, welche du behalten willst.': 'The platforms have different times. Choose which ones to keep.',
  'Zeiten von {platforms}': 'Times from {platforms}',
  'Keine Zeiten': 'No times',
  'Abbrechen': 'Cancel',
  'Verteilung der Clips': 'Clip distribution',
  'Gleicher Clip auf allen Plattformen': 'Same clip on all platforms',
  'Jeder Clip erscheint auf allen ausgewählten Plattformen, jeweils zum nächsten freien Termin.': 'Each clip goes out on all selected platforms, each at its next free time.',
  'Jede Plattform eigene Clips': 'Each platform gets its own clips',
  'Jede Plattform bekommt ihre eigenen Clips.': 'Each platform gets its own clips.',
  'Nächster Termin': 'Next time',
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
  enabled: boolean;
  paused: boolean;
  lead_time_hours: number;
  target_platforms: string[];
  pipeline_mode: PipelineMode;
  review_policy: ReviewPolicy;
  schedule_scope: PolicyScope;
  distribution_policy: DistributionPolicy;
  shared: WeeklySlot[];
  per_platform: Record<string, WeeklySlot[]>;
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
    enabled: snapshot.settings.enabled,
    paused: snapshot.settings.paused,
    lead_time_hours: snapshot.settings.lead_time_hours,
    target_platforms: [...snapshot.settings.target_platforms],
    pipeline_mode: policy.pipeline_mode,
    review_policy: policy.review_policy,
    schedule_scope: policy.schedule_scope,
    distribution_policy: policy.distribution_policy,
    shared: structuredClone(snapshot.settings.schedule_slots),
    per_platform: structuredClone(policy.platform_schedule_slots),
  };
}

function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function samePlatforms(a: string[], b: string[]): boolean {
  return sameJson([...a].sort(), [...b].sort());
}

export function schedulePatch(base: PlanState, draft: PlanDraft): SchedulePatch {
  const settings = base.snapshot.settings;
  const patch: SchedulePatch = {};
  if (draft.enabled !== settings.enabled) patch.enabled = draft.enabled;
  if (draft.paused !== settings.paused) patch.paused = draft.paused;
  if (draft.lead_time_hours !== settings.lead_time_hours) patch.lead_time_hours = draft.lead_time_hours;
  if (!samePlatforms(draft.target_platforms, settings.target_platforms)) patch.target_platforms = PLAN_PLATFORMS.map(platform => platform.id).filter(id => draft.target_platforms.includes(id));
  if (!sameJson(normalizeSlots(draft.shared), normalizeSlots(settings.schedule_slots))) patch.schedule_slots = normalizeSlots(draft.shared);
  if (draft.pipeline_mode !== base.policy.pipeline_mode && (draft.pipeline_mode === 'ai_edit') !== settings.ai_editing_enabled) patch.ai_editing_enabled = draft.pipeline_mode === 'ai_edit';
  return patch;
}

export function policyChange(base: PlanState, draft: PlanDraft): PipelinePolicy | null {
  const policy = base.policy;
  const perPlatform = normalizePerPlatform(draft.per_platform);
  const unchanged = draft.pipeline_mode === policy.pipeline_mode && draft.review_policy === policy.review_policy
    && draft.schedule_scope === policy.schedule_scope && draft.distribution_policy === policy.distribution_policy
    && sameJson(perPlatform, normalizePerPlatform(policy.platform_schedule_slots));
  if (unchanged) return null;
  return {
    ...policy,
    pipeline_mode: draft.pipeline_mode,
    review_policy: draft.review_policy,
    schedule_scope: draft.schedule_scope,
    distribution_policy: draft.distribution_policy,
    platform_schedule_slots: perPlatform,
  };
}

export function isDirty(base: PlanState, draft: PlanDraft): boolean {
  return Object.keys(schedulePatch(base, draft)).length > 0 || policyChange(base, draft) !== null;
}

function editedLists(draft: PlanDraft): WeeklySlot[][] {
  if (draft.schedule_scope === 'shared') return [draft.shared];
  return draft.target_platforms.map(platform => draft.per_platform[platform] ?? []);
}

export function validateDraft(draft: PlanDraft, text: PlanText): string[] {
  const problems: string[] = [];
  if (!Number.isFinite(draft.lead_time_hours) || draft.lead_time_hours < 0 || draft.lead_time_hours > 720) problems.push(text('Der Vorlauf muss zwischen 0 und 720 Stunden liegen.'));
  for (const slots of editedLists(draft)) {
    if (slots.some(slot => !/^\d{2}:\d{2}$/.test(slot.local_time))) problems.push(text('Jede Zeit braucht eine Uhrzeit.'));
    const seen = new Set<string>();
    for (const slot of slots) {
      const key = `${slot.weekday}-${slot.local_time}`;
      if (seen.has(key)) problems.push(text('{day} {time} ist doppelt.', { day: text(WEEKDAYS[slot.weekday - 1]), time: slot.local_time }));
      seen.add(key);
    }
  }
  return [...new Set(problems)];
}

export function addTime(slots: WeeklySlot[], weekday: number): WeeklySlot[] {
  const used = new Set(slots.filter(slot => slot.weekday === weekday).map(slot => slot.local_time));
  const local_time = SUGGESTED_TIMES.find(time => !used.has(time)) ?? '00:00';
  return [...slots, { weekday, local_time }];
}

export interface ScopeSwitch {
  draft: PlanDraft;
  choices?: { platforms: string[]; slots: WeeklySlot[] }[];
}

export function switchScope(draft: PlanDraft, scope: PolicyScope): ScopeSwitch {
  if (scope === draft.schedule_scope) return { draft };
  if (scope === 'per_platform') {
    const per_platform = { ...draft.per_platform };
    for (const platform of draft.target_platforms) {
      if (!per_platform[platform]?.length) per_platform[platform] = structuredClone(draft.shared);
    }
    return { draft: { ...draft, schedule_scope: 'per_platform', per_platform } };
  }
  const choices: { platforms: string[]; slots: WeeklySlot[] }[] = [];
  for (const platform of draft.target_platforms) {
    const slots = normalizeSlots(draft.per_platform[platform] ?? []);
    const match = choices.find(choice => sameJson(choice.slots, slots));
    if (match) match.platforms.push(platform);
    else choices.push({ platforms: [platform], slots });
  }
  if (choices.length > 1) return { draft, choices };
  return { draft: { ...draft, schedule_scope: 'shared', shared: choices[0]?.slots ?? draft.shared } };
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
      state = { ...state, snapshot: await api.load(owner) };
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
