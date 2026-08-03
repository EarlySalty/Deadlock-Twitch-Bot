/**
 * Streamer-Selbstbedienung: Bot vom eigenen Kanal trennen.
 *
 * Der Login kommt aus der Session, nicht aus dem Request — geschickt wird nur
 * die abgetippte Bestätigung. Die Antwort meldet jeden Teilschritt einzeln,
 * damit ein halb erledigter Lauf im UI nicht wie ein Erfolg aussieht.
 */

/** `removed` | `not_moderator` | `no_token` | `unknown_channel` | `unavailable` | `failed` */
export type UnmodOutcome = string;

export interface DisconnectBotResponse {
  ok: boolean;
  login: string;
  unmod: UnmodOutcome;
  unmod_detail?: string;
  departnered: boolean;
  opt_out: boolean;
  discord_role?: string;
  message: string;
}

const ENDPOINT = '/twitch/api/v2/streamer/disconnect-bot';

export async function disconnectBot(confirmLogin: string): Promise<DisconnectBotResponse> {
  const response = await fetch(ENDPOINT, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ confirm_login: confirmLogin }),
  });
  let body: Partial<DisconnectBotResponse> & { error?: string } = {};
  try {
    body = await response.json();
  } catch {
    // Antwort ohne JSON — unten als Fehler behandelt.
  }
  if (!response.ok) {
    throw new Error(body?.message || body?.error || `HTTP ${response.status}`);
  }
  return body as DisconnectBotResponse;
}

/**
 * `true`, wenn der Moderator-Entzug NICHT durchlief. Der Streamer muss dann
 * selbst unmodden — deshalb wird dieser Fall im UI als Warnung gezeigt und
 * nicht unter „erledigt“ verbucht.
 */
export function unmodNeedsAttention(outcome: UnmodOutcome): boolean {
  return outcome !== 'removed' && outcome !== 'not_moderator';
}
