// API des Server-Admin-Dashboards (Multi-Server Paket A).
// Eigene Session (Discord-Login, Recht "Server verwalten"), eigene Endpunkte.
// Die Rechte prueft ausschliesslich der Server; diese Datei kennt nur die
// erlaubten Bereiche und schickt nie etwas anderes.
import { ApiHttpError } from './httpError';

export const SERVER_ADMIN_ROUTE = '/twitch/server-admin';
export const SERVER_ADMIN_LOGIN_URL = '/twitch/server-admin/auth/login';
export const SERVER_ADMIN_LOGOUT_URL = '/twitch/server-admin/auth/logout';
const API = '/twitch/api/server-admin';

/** Abschnitte des Panels (Anker auf der Seite, Sidebar-Navigation). */
export type ServerAdminSection = 'overview' | 'streamers' | 'channels' | 'language' | 'raids';
export const SERVER_ADMIN_SECTIONS: ReadonlyArray<ServerAdminSection> = [
  'overview',
  'streamers',
  'channels',
  'language',
  'raids',
];

export interface GuildSettings {
  guildId: string;
  name: string;
  language: 'de' | 'en';
  announceChannelId: string | null;
  alertChannelId: string | null;
  streamerRoleId: string | null;
  pingRoleId: string | null;
  promoInviteUrl: string | null;
  autoPartnerDefault: boolean;
  crossServerRaids: boolean;
  activeStreamers: number;
  inactiveStreamers: number;
}

export interface ServerAdminMe {
  principal: 'guild_admin';
  displayName: string;
  csrfToken: string;
  guilds: GuildSettings[];
  permittedAreas: string[];
}

export interface GuildStreamer {
  twitchUserId: string;
  twitchLogin: string | null;
  active: boolean;
  source: string;
  joinedAt: string;
  deactivatedAt: string | null;
}

/** Nur diese Felder kennt die Einstellungs-API. */
export type GuildSettingsPatch = Partial<
  Pick<
    GuildSettings,
    | 'language'
    | 'announceChannelId'
    | 'alertChannelId'
    | 'streamerRoleId'
    | 'pingRoleId'
    | 'promoInviteUrl'
    | 'autoPartnerDefault'
    | 'crossServerRaids'
  >
>;

export const ALLOWED_PATCH_FIELDS: ReadonlyArray<keyof GuildSettingsPatch> = [
  'language',
  'announceChannelId',
  'alertChannelId',
  'streamerRoleId',
  'pingRoleId',
  'promoInviteUrl',
  'autoPartnerDefault',
  'crossServerRaids',
];

export class ServerAdminLoginRequired extends ApiHttpError {
  constructor() {
    super('server_admin_login_required', 401);
    this.name = 'ServerAdminLoginRequired';
  }
}

async function request<T>(path: string, init: RequestInit = {}, csrfToken?: string | null): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (init.body) headers['Content-Type'] = 'application/json';
  if (csrfToken) headers['X-CSRF-Token'] = csrfToken;
  const response = await fetch(`${API}${path}`, {
    credentials: 'same-origin',
    ...init,
    headers,
  });
  if (response.status === 401) {
    throw new ServerAdminLoginRequired();
  }
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as { error?: string; message?: string } | null;
    throw new ApiHttpError(body?.error || body?.message || `HTTP ${response.status}`, response.status);
  }
  return response.json() as Promise<T>;
}

export function fetchServerAdminMe(): Promise<ServerAdminMe> {
  return request<ServerAdminMe>('/me');
}

export function fetchGuildStreamers(guildId: string): Promise<{ streamers: GuildStreamer[] }> {
  return request(`/guilds/${encodeURIComponent(guildId)}/streamers`);
}

export function patchGuildSettings(
  guildId: string,
  patch: GuildSettingsPatch,
  csrfToken: string | null,
): Promise<GuildSettings> {
  const body: Record<string, unknown> = {};
  for (const field of ALLOWED_PATCH_FIELDS) {
    if (field in patch) body[field] = patch[field];
  }
  return request<GuildSettings>(
    `/guilds/${encodeURIComponent(guildId)}/settings`,
    { method: 'PATCH', body: JSON.stringify(body) },
    csrfToken,
  );
}

export function setStreamerActive(
  guildId: string,
  twitchUserId: string,
  active: boolean,
  csrfToken: string | null,
): Promise<{ ok: boolean; active: boolean }> {
  return request(
    `/guilds/${encodeURIComponent(guildId)}/streamers/${encodeURIComponent(twitchUserId)}`,
    { method: 'POST', body: JSON.stringify({ active }) },
    csrfToken,
  );
}

/** Discord-Snowflake oder leer (= Feld leeren). */
export function isSnowflakeOrEmpty(value: string): boolean {
  const trimmed = value.trim();
  return trimmed === '' || /^[1-9][0-9]{5,19}$/.test(trimmed);
}

export function errorText(error: unknown): string {
  if (error instanceof ApiHttpError) {
    switch (error.message) {
      case 'guild_forbidden':
        return 'Für diesen Server hast du keine Verwaltungsrechte.';
      case 'area_forbidden':
        return 'Dieser Bereich wird zentral verwaltet und ist hier gesperrt.';
      case 'invalid_field':
        return 'Ein Wert ist ungültig. Bitte prüfe IDs und Einladungslink.';
      case 'invalid_csrf':
        return 'Sitzung abgelaufen. Bitte lade die Seite neu.';
      default:
        return error.message;
    }
  }
  return 'Unbekannter Fehler.';
}
