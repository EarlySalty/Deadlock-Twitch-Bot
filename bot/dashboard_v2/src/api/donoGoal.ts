import { fetchJson } from './core';

export interface DonoGoalSettings {
  login: string;
  configured: boolean;
  title: string;
  goal: number;
  currency: string;
  count_from: string | null;
  se_channel_id: string;
  connected: boolean;
}

export interface DonoGoalUpdate {
  title: string;
  goal: number;
  currency: string;
  se_channel_id: string;
  se_jwt?: string;
  clear_jwt?: boolean;
}

export interface DonoGoalTestResult {
  ok: boolean;
  error?: 'not_configured' | 'unauthorized' | 'unreachable';
}

export const DONO_GOAL_PATH = '/twitch/api/v2/streamer/dono-goal';

const jsonPost = (body?: unknown): RequestInit => ({
  method: 'POST',
  credentials: 'same-origin',
  headers: { 'Content-Type': 'application/json' },
  body: body === undefined ? undefined : JSON.stringify(body),
});

export function fetchDonoGoal(): Promise<DonoGoalSettings> {
  return fetchJson<DonoGoalSettings>(DONO_GOAL_PATH, { credentials: 'same-origin' });
}

export function saveDonoGoal(update: DonoGoalUpdate): Promise<DonoGoalSettings> {
  return fetchJson<DonoGoalSettings>(DONO_GOAL_PATH, jsonPost(update));
}

export function resetDonoGoal(): Promise<DonoGoalSettings> {
  return fetchJson<DonoGoalSettings>(`${DONO_GOAL_PATH}/reset`, jsonPost());
}

export function testDonoGoal(): Promise<DonoGoalTestResult> {
  return fetchJson<DonoGoalTestResult>(`${DONO_GOAL_PATH}/test`, jsonPost());
}
