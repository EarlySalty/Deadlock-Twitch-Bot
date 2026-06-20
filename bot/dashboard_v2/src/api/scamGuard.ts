/**
 * Conversation-Scam-Guard (Verwaltung) — JSON-API gegen
 * /twitch/api/v2/streamer/scam-guard/* (native Rust-Handler auf tb-dashboard).
 *
 * Auth: same-origin cookie (gleicher Mechanismus wie der Rest des Dashboards).
 * Der Guard beurteilt Erstschreiber per LLM auf aufgesetzte Scams; das Dashboard
 * steuert Einstellungen und arbeitet die Vorschlags-Queue ab. Bann/Rücknahme
 * laufen über die Internal-API (Proxy im Dashboard-Backend).
 */

const BASE = '/twitch/api/v2/streamer/scam-guard';

/** Verhalten bei hoher Sicherheit. Muss exakt zu VALID_MODES im Backend passen. */
export type ScamGuardMode = 'auto_ban' | 'timeout' | 'alert_only';

export interface ScamGuardSettings {
  enabled: boolean;
  mode: ScamGuardMode;
  /** Schwelle für die automatische Aktion (0–1, Default 0.90). */
  threshold: number;
  /** Schwelle, ab der ein Fall in die Vorschlags-Queue wandert (0–1, Default 0.70). */
  suggestion_floor: number;
}

/**
 * Ein Eintrag der Fall-Queue. `action_taken` unterscheidet, ob der Fall nur
 * vorgeschlagen ('suggested') oder bereits automatisch durchgesetzt wurde
 * ('banned' / 'timed_out'). Bereits durchgesetzte Fälle lassen sich über die
 * Detail-/Revoke-Route wieder zurücknehmen (echter Twitch-Unban via Bot).
 */
export interface ScamQueueItem {
  id: number;
  chatter_login: string;
  chatter_id: string | null;
  confidence: number;
  category: string;
  reasoning: string;
  action_taken: string;
  created_at: string;
}

/** Vollständiges Urteil inkl. Transkript-Auszug (Detail-Ansicht). */
export interface ScamVerdictDetail extends ScamQueueItem {
  verdict: string;
  transcript_snapshot: string;
}

/** Ergebnis der Bann-Aktion (HTTP 200, Logik-Ausgang im status-Feld). */
export type ScamEnforceStatus =
  | 'enforced'
  | 'ban_failed_no_mod'
  | 'not_eligible'
  | 'not_found';

/** Ergebnis der Rücknahme (HTTP 200, Logik-Ausgang im status-Feld). */
export type ScamRevokeStatus = 'revoked' | 'not_found';

export interface ScamActionResult {
  status: string;
  chatter_login?: string;
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    credentials: 'same-origin',
    ...init,
  });
  if (!response.ok) {
    // Fehlertext aus { error: "..." } ziehen, sonst HTTP-Code.
    const payload = (await response.json().catch(() => null)) as { error?: string } | null;
    throw new Error(payload?.error || `HTTP ${response.status}`);
  }
  return (await response.json()) as T;
}

export async function fetchScamSettings(): Promise<ScamGuardSettings> {
  return fetchJson(`${BASE}/settings`);
}

export async function saveScamSettings(settings: ScamGuardSettings): Promise<ScamGuardSettings> {
  return fetchJson(`${BASE}/settings`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settings),
  });
}

export async function fetchScamQueue(): Promise<ScamQueueItem[]> {
  const data = await fetchJson<{ queue: ScamQueueItem[] }>(`${BASE}/queue`);
  return data.queue ?? [];
}

export async function fetchScamVerdict(id: number): Promise<ScamVerdictDetail> {
  return fetchJson(`${BASE}/verdicts/${id}`);
}

export async function ignoreScamVerdict(id: number): Promise<void> {
  await fetchJson(`${BASE}/queue/${id}/ignore`, { method: 'POST' });
}

export async function banScamVerdict(id: number): Promise<ScamActionResult> {
  return fetchJson(`${BASE}/queue/${id}/ban`, { method: 'POST' });
}

export async function revokeScamVerdict(id: number): Promise<ScamActionResult> {
  return fetchJson(`${BASE}/verdicts/${id}/revoke`, { method: 'POST' });
}
