import { admin, postAdminJson } from '@/api/client';

export type ObserverClip = { clip_db_id: number; login: string; title: string; clip_url: string; compared: boolean };
export type ObserverTrial = {
  id: number;
  comparison_id: number;
  observer: string;
  model_item: string;
  thinking_item: string;
  status: 'queued' | 'running' | 'answered' | 'failed';
  failure_code: string | null;
  answer: string | null;
  elapsed_ms: number | null;
  started_at: string | null;
  finished_at: string | null;
  rating: number | null;
  rating_note: string | null;
  rated_at: string | null;
};
export type ObserverComparison = {
  id: number;
  clip_db_id: number;
  login: string;
  title: string;
  clip_url: string;
  status: 'queued' | 'expanded' | 'failed';
  failure_code: string | null;
  requested_by: string;
  created_at: string;
  trials: ObserverTrial[];
};
export type ObserverScore = {
  observer: string;
  model_item: string;
  thinking_item: string;
  trials: number;
  answered: number;
  failed: number;
  pending: number;
  rated: number;
  avg_rating: number | null;
  avg_elapsed_ms: number | null;
};
export type ObserverReport = { variants: number; clips: ObserverClip[]; comparisons: ObserverComparison[]; scoreboard: ObserverScore[] };

export const observerKeys = { report: ['social-ai', 'observer-compare'] as const };

function requireReport(value: unknown): ObserverReport {
  const data = value as Partial<ObserverReport> | null;
  if (!data || typeof data.variants !== 'number' || !Array.isArray(data.clips) || !Array.isArray(data.comparisons) || !Array.isArray(data.scoreboard)
    || !data.comparisons.every(comparison => Array.isArray(comparison?.trials))) {
    throw new Error('Der Modellvergleich konnte nicht bestätigt werden. Bitte lade den Stand neu.');
  }
  return data as ObserverReport;
}

export async function fetchObserverReport(signal?: AbortSignal) {
  return requireReport(await admin<unknown>('/social-ai/observer-compare', { cache: 'no-store', signal }));
}

export async function startComparison(clipDbId: number) {
  return requireReport(await postAdminJson<unknown, { clip_db_id: number }>('/social-ai/observer-compare', { clip_db_id: clipDbId }));
}

export async function rateTrial(trialId: number, rating: number, note: string) {
  const data = requireReport(await postAdminJson<unknown, { rating: number; note: string }>(`/social-ai/observer-compare/trials/${trialId}/rating`, { rating, note }));
  const trial = data.comparisons.flatMap(comparison => comparison.trials).find(trial => trial.id === trialId);
  if (trial && trial.rating !== rating) throw new Error('Die Bewertung wurde nicht bestätigt. Bitte lade den Stand neu.');
  return data;
}
