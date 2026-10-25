import { withCookieCredentials } from './core';
import type { RecordingConsent, RecordingDecision } from '@/components/socialmedia/recording/recordingPolicy';

const BASE = '/twitch/social-media/api/recording';

function writeHeaders(csrfToken: string | null | undefined): Record<string, string> {
  return {
    Accept: 'application/json',
    'Content-Type': 'application/json',
    ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
  };
}

export async function fetchRecordingConsent(): Promise<RecordingConsent> {
  const response = await fetch(`${BASE}/consent`, withCookieCredentials({ headers: { Accept: 'application/json' } }));
  if (!response.ok) throw new Error(`http_${response.status}`);
  const data = (await response.json()) as Partial<RecordingConsent>;
  const decision = data.decision === 'granted' || data.decision === 'declined' ? data.decision : null;
  return { eligible: data.eligible === true, decision };
}

export async function saveRecordingConsent(decision: RecordingDecision, csrfToken: string | null | undefined): Promise<void> {
  const response = await fetch(`${BASE}/consent`, withCookieCredentials({
    method: 'POST',
    headers: writeHeaders(csrfToken),
    body: JSON.stringify({ decision }),
  }));
  if (!response.ok) throw new Error(`http_${response.status}`);
}

export function postRecordingChunk(body: string, keepalive: boolean, csrfToken: string | null | undefined): Promise<Response> {
  return fetch(`${BASE}/chunk`, withCookieCredentials({
    method: 'POST',
    keepalive,
    headers: writeHeaders(csrfToken),
    body,
  }));
}
