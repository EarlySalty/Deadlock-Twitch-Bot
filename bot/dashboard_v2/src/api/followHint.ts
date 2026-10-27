export interface FollowHintSettings {
  enabled: boolean;
  message: string;
  default_message: string;
  max_chars: number;
}

const BASE = '/twitch/api/v2/streamer/follow-hint-settings';

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, { credentials: 'same-origin', ...init });
  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    try {
      const body = await response.json();
      if (body?.error) message = String(body.error);
    } catch {
      message = `HTTP ${response.status}`;
    }
    throw new Error(message);
  }
  return (await response.json()) as T;
}

export function fetchFollowHintSettings(): Promise<FollowHintSettings> {
  return fetchJson(BASE);
}

export function saveFollowHintSettings(enabled: boolean, message: string): Promise<FollowHintSettings> {
  return fetchJson(BASE, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ enabled, message }),
  });
}

const TWITCH_LOGIN_MAX_CHARS = 25;
const TWITCH_CHAT_MAX_CHARS = 500;

export function normalizeFollowHint(raw: string): string {
  return raw.split(/\s+/).filter(Boolean).join(' ');
}

export function countChars(text: string): number {
  return Array.from(text).length;
}

export function followHintTooLong(raw: string, maxChars: number): boolean {
  const normalized = normalizeFollowHint(raw);
  if (!normalized) return false;
  if (countChars(normalized) > maxChars) return true;
  return countChars(previewFollowHint(normalized, normalized, 'x'.repeat(TWITCH_LOGIN_MAX_CHARS))) > TWITCH_CHAT_MAX_CHARS;
}

export function previewFollowHint(template: string, fallback: string, login: string): string {
  const text = template.trim() || fallback;
  return text.includes('{user}') ? text.split('{user}').join(login) : `@${login} ${text}`;
}
