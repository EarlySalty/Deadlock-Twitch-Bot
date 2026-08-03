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

/**
 * `true`, wenn die Discord-Streamer-Rolle noch beim Streamer liegt. `revoked`
 * ist der Erfolgsfall, `skipped:no_discord_link` heißt: es gibt gar keine
 * Verknüpfung, also nichts zu entziehen. Alles andere — kein Guild-Kandidat,
 * Broker-Fehler, Port fehlt — bleibt offen und muss sichtbar sein.
 */
export function roleNeedsAttention(discordRole: string | undefined): boolean {
  return discordRole !== 'revoked' && discordRole !== 'skipped:no_discord_link';
}

/** Kurzer deutscher Klartext für den Rollen-Ausgang. */
export function roleLabel(discordRole: string | undefined): string {
  // Fehlendes Feld heißt nicht „nichts zu tun": eine ältere Server-Version
  // hat den Ausgang schlicht nicht gemeldet — das ist ungeklärt, nicht okay.
  if (!discordRole) return 'ACHTUNG — Ausgang unbekannt (keine Angabe vom Server)';
  if (discordRole === 'skipped:no_discord_link') {
    return 'kein Discord-Account verknüpft';
  }
  if (discordRole === 'revoked') return 'entzogen';
  if (discordRole.startsWith('skipped:')) {
    return `ACHTUNG — bleibt bestehen (${discordRole.slice('skipped:'.length)})`;
  }
  if (discordRole.startsWith('failed:')) {
    return `ACHTUNG — Entzug fehlgeschlagen (${discordRole.slice('failed:'.length)})`;
  }
  return `ACHTUNG — unklarer Ausgang (${discordRole})`;
}
