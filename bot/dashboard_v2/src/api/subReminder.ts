export interface SubReminderSettings { enabled: boolean; available: boolean }
const BASE = '/twitch/api/v2/streamer/sub-reminder-settings';
export async function subReminderSettings(enabled?: boolean): Promise<SubReminderSettings> {
  const response = await fetch(BASE, {
    credentials: 'same-origin',
    ...(enabled === undefined ? {} : {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled }),
    }),
  });
  if (!response.ok) throw new Error('Die Einstellung konnte nicht gespeichert oder geladen werden.');
  return response.json();
}
