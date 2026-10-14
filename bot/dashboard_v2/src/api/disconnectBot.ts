import { tr } from '../i18n/dictionary';
import { withLanguageHeader } from './languageHeader';

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
  const response = await fetch(
    ENDPOINT,
    withLanguageHeader(ENDPOINT, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ confirm_login: confirmLogin }),
    })
  );
  const body = (await response.json().catch(() => ({}))) as Partial<DisconnectBotResponse> & {
    error?: string;
  };
  if (!response.ok) {
    throw new Error(body?.message || body?.error || `HTTP ${response.status}`);
  }
  return body as DisconnectBotResponse;
}

export function unmodNeedsAttention(outcome: UnmodOutcome): boolean {
  return outcome !== 'removed' && outcome !== 'not_moderator';
}

export function roleNeedsAttention(discordRole: string | undefined): boolean {
  return discordRole !== 'revoked' && discordRole !== 'skipped:no_discord_link';
}

export function roleLabel(discordRole: string | undefined): string {
  if (!discordRole) return tr('ACHTUNG — Ausgang unbekannt (keine Angabe vom Server)');
  if (discordRole === 'skipped:no_discord_link') {
    return tr('kein Discord-Account verknüpft');
  }
  if (discordRole === 'revoked') return tr('entzogen');
  if (discordRole.startsWith('skipped:')) {
    return tr('ACHTUNG — bleibt bestehen ({reason})', {
      reason: discordRole.slice('skipped:'.length),
    });
  }
  if (discordRole.startsWith('failed:')) {
    return tr('ACHTUNG — Entzug fehlgeschlagen ({reason})', {
      reason: discordRole.slice('failed:'.length),
    });
  }
  return tr('ACHTUNG — unklarer Ausgang ({outcome})', { outcome: discordRole });
}