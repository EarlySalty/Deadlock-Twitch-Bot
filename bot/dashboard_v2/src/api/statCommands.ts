export const STAT_COMMANDS = [
  { command: 'rank', label: '!rank', description: 'Dein aktueller Rang.' },
  { command: 'wins', label: '!wins', description: 'Deine Siege.' },
  { command: 'winrate', label: '!winrate', description: 'Deine Siegquote.' },
  { command: 'mmr', label: '!mmr', description: 'Deine Rangentwicklung. Gilt auch für !climb.' },
  { command: 'live', label: '!live', description: 'Infos zu deinem laufenden Match.' },
  { command: 'lastmatch', label: '!lastmatch', description: 'Dein letztes Match. Gilt auch für !last.' },
  { command: 'streak', label: '!streak', description: 'Deine aktuelle Sieg- oder Niederlagenserie.' },
  { command: 'mostplayed', label: '!mostplayed', description: 'Dein meistgespielter Held. Gilt auch für !main.' },
] as const;

export type StatCommand = typeof STAT_COMMANDS[number]['command'];
export type StatCommandSettings = Record<StatCommand, boolean>;
const BASE = '/twitch/api/v2/streamer/stat-command-settings';

async function fetchJson<T>(init: RequestInit = {}): Promise<T> {
  const response = await fetch(BASE, { credentials: 'same-origin', ...init });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return await response.json() as T;
}

export function fetchStatCommandSettings(): Promise<{ commands: StatCommandSettings }> {
  return fetchJson();
}

export function toggleStatCommand(command: StatCommand, enabled: boolean): Promise<{ ok: boolean; command: StatCommand; enabled: boolean }> {
  return fetchJson({ method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ command, enabled }) });
}
