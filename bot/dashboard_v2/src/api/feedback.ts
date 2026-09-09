import { buildApiUrl, fetchApi, fetchJson, withCookieCredentials } from './core';

export type FeedbackKind = 'feedback' | 'feature';
export type FeedbackStatus = 'received' | 'reviewing' | 'planned' | 'in_progress' | 'done' | 'declined' | 'answered';
export const feedbackStatusLabels: Record<FeedbackStatus, string> = {
  received: 'Eingegangen', reviewing: 'In Prüfung', planned: 'Geplant',
  in_progress: 'In Arbeit', done: 'Umgesetzt', declined: 'Derzeit nicht vorgesehen', answered: 'Beantwortet',
};
export function feedbackStatuses(kind: FeedbackKind): FeedbackStatus[] {
  return (Object.keys(feedbackStatusLabels) as FeedbackStatus[]).filter(status => kind === 'feedback' || status !== 'answered');
}
export interface FeedbackEntry {
  id: number; kind: FeedbackKind; title: string; body: string; area: string;
  status: FeedbackStatus; result_path: string | null; revision: number; bundled: boolean;
  decision_reason: string | null;
  roadmap: { id: number; title: string; status: string } | null;
  created_at: string; observed_at: string; unread: boolean;
  owner_id?: string; author_label?: string; admin_unread?: boolean;
  duplicate_of?: number | null; roadmap_id?: number | null;
  own_status?: FeedbackStatus; own_result_path?: string | null;
  own_decision_reason?: string | null;
}
export interface FeedbackDetail extends FeedbackEntry {
  events: { id: number; status: FeedbackStatus; reply: string; result_path: string | null; decision_reason: string | null; created_at: string }[];
}
export interface FeedbackSubmission { kind: FeedbackKind; title: string; body: string; area: string }
export interface FeedbackUpdate {
  expected_revision: number; status: FeedbackStatus; reply: string;
  result_path: string | null; roadmap_id: number | null; duplicate_of: number | null;
  decision_reason: string | null;
}
export interface FeedbackCounts { total: number; unread: number }
export interface FeedbackList { items: FeedbackEntry[]; next: number | null }
export interface RoadmapChoice { id: number; title: string; status: string }

export function feedbackHref(kind?: FeedbackKind, area?: string): string {
  const params = new URLSearchParams();
  if (kind) params.set('typ', kind);
  if (area) params.set('bereich', area.slice(0, 80));
  return `/twitch/feedback${params.size ? `?${params}` : ''}`;
}
export function validFeedbackResultPath(path: string): boolean {
  if (/[?%\\]/.test(path) || [...path].some(char => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127) || path.length > 200) return false;
  const [base, anchor = '', extra] = path.split('#');
  return extra === undefined && ['/twitch/dashboard', '/twitch/verwaltung', '/twitch/uplink', '/analyse', '/twitch/abbo', '/social-media'].includes(base)
    && /^[a-zA-Z0-9_-]*$/.test(anchor);
}
/** Bleibt nach Netzfehlern gleich; eine bewusste Textänderung ist ein neuer Auftrag. */
export function submissionAttempt<T>(payload: T, previous?: { fingerprint: string; requestId: string }, newId = () => crypto.randomUUID()) {
  const fingerprint = JSON.stringify(payload);
  return previous?.fingerprint === fingerprint ? previous : { fingerprint, requestId: newId() };
}
export function fetchFeedback(inbox = false, before?: number) {
  return fetchApi<FeedbackList>('/feedback', { inbox, ...(before ? { before } : {}) });
}
export function fetchFeedbackDetail(id: number) { return fetchApi<FeedbackDetail>(`/feedback/${id}`); }
export function fetchFeedbackCounts(inbox = false) { return fetchApi<FeedbackCounts>('/feedback/counts', { inbox }); }
async function write<T>(path: string, payload: unknown, csrfToken?: string | null): Promise<T> {
  return fetchJson<T>(buildApiUrl(path), withCookieCredentials({
    method: 'POST', headers: { 'Content-Type': 'application/json', ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}) },
    body: JSON.stringify(payload),
  }));
}
export function submitFeedback(body: FeedbackSubmission, requestId: string, csrfToken?: string | null) {
  return write<{ id: number; saved: boolean }>('/feedback', { ...body, request_id: requestId }, csrfToken);
}
export function updateFeedback(id: number, body: FeedbackUpdate, requestId: string, csrfToken?: string | null) {
  return write<{ ok: boolean }>(`/feedback/${id}/update`, { ...body, request_id: requestId }, csrfToken);
}
export function markFeedbackRead(id: number, observedAt: string, inbox: boolean, csrfToken?: string | null) {
  return write<{ ok: boolean }>(`/feedback/${id}/read`, { observed_at: observedAt, inbox }, csrfToken);
}
export async function fetchFeedbackRoadmap(): Promise<RoadmapChoice[]> {
  const data = await fetchApi<Record<string, RoadmapChoice[]>>('/roadmap');
  return Object.values(data).flat();
}
