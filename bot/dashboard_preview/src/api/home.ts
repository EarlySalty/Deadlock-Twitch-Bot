import { createPreviewChangelogEntry } from '../preview/fixtures';
import { PREVIEW_HOME_ROUTE, isPreviewLocalhost } from '../preview/routes';
import { buildApiUrl, fetchApi, fetchJson, sanitizeInternalRedirectUrl, withCookieCredentials } from './core';

const INTERNAL_HOME_LOGIN_FALLBACK = PREVIEW_HOME_ROUTE;
const INTERNAL_HOME_BLOCKED_OAUTH_PATHS = ['/twitch/raid/requirements'] as const;

function sanitizeInternalHomeOauthUrl(
  rawUrl: string | null | undefined,
  fallback = INTERNAL_HOME_LOGIN_FALLBACK
): string {
  const safeFallback = sanitizeInternalRedirectUrl(null, fallback);
  const sanitized = sanitizeInternalRedirectUrl(rawUrl, safeFallback);

  try {
    const parsed = new URL(sanitized, window.location.origin);
    if (INTERNAL_HOME_BLOCKED_OAUTH_PATHS.some((blockedPath) => blockedPath === parsed.pathname)) {
      return safeFallback;
    }
  } catch {
    return safeFallback;
  }

  return sanitized;
}

export interface CreateInternalHomeChangelogPayload {
  title?: string;
  content: string;
  entryDate?: string;
}

export interface InternalHomeOAuthStatus {
  connected?: boolean;
  status?: 'connected' | 'partial' | 'missing' | 'reauth' | 'error';
  needsReauth?: boolean;
  grantedScopes?: string[];
  missingScopes?: string[];
  reconnectUrl?: string | null;
  profileUrl?: string | null;
  lastCheckedAt?: string | null;
}

export interface InternalHomeDiscordStatus {
  connected?: boolean;
  status?: 'connected' | 'missing' | 'error' | string;
  connectUrl?: string | null;
  lastCheckedAt?: string | null;
}

export interface InternalHomeRaidStatus {
  active?: boolean;
  statusText?: string | null;
  note?: string | null;
  lastEventAt?: string | null;
}

export interface InternalHomeKpis30d {
  streams?: number;
  avgViewers?: number;
  followerDelta?: number;
  banKpi?: number;
}

export interface InternalHomeSession {
  id?: number | string;
  startedAt?: string | null;
  endedAt?: string | null;
  durationMinutes?: number | null;
  avgViewers?: number | null;
  peakViewers?: number | null;
  followerDelta?: number | null;
  title?: string | null;
  category?: string | null;
}

export interface InternalHomeActionEntry {
  id?: number | string;
  timestamp?: string | null;
  eventType?: string | null;
  statusLabel?: string | null;
  targetLogin?: string | null;
  targetId?: string | null;
  actorLogin?: string | null;
  reason?: string | null;
  summary?: string | null;
  title?: string | null;
  description?: string | null;
  metric?: string | null;
  viewerCount?: number | null;
  success?: boolean | null;
  severity?: 'success' | 'info' | 'warning' | 'critical' | string;
  source?: string | null;
}

export type InternalHomeImpactEntry = InternalHomeActionEntry;

export interface InternalHomeChangelogEntry {
  id?: number | string;
  entryDate?: string | null;
  title?: string | null;
  content?: string | null;
  createdAt?: string | null;
}

export interface InternalHomeChangelog {
  entries?: InternalHomeChangelogEntry[] | null;
  canWrite?: boolean;
  maxEntries?: number | null;
}

export interface InternalHomeData {
  greeting?: string | null;
  twitchLogin?: string | null;
  displayName?: string | null;
  loginUrl?: string | null;
  oauth?: InternalHomeOAuthStatus | null;
  discord?: InternalHomeDiscordStatus | null;
  raid?: InternalHomeRaidStatus | null;
  kpis30d?: InternalHomeKpis30d | null;
  recentStreams?: InternalHomeSession[] | null;
  actionLog?: InternalHomeActionEntry[] | null;
  impactFeed?: InternalHomeImpactEntry[] | null;
  changelog?: InternalHomeChangelog | null;
  generatedAt?: string | null;
}

interface InternalHomeRawOAuthStatus {
  connected?: boolean;
  status?: string;
  granted_scopes?: string[];
  missing_scopes?: string[];
  reconnect_url?: string | null;
  profile_url?: string | null;
  last_checked_at?: string | null;
}

interface InternalHomeRawDiscordStatus {
  connected?: boolean;
  status?: string;
  connect_url?: string | null;
  last_checked_at?: string | null;
}

interface InternalHomeRawRaidStatus {
  state?: string | null;
  read_only?: boolean;
}

interface InternalHomeRawProfile {
  twitch_login?: string | null;
  twitch_user_id?: string | null;
  display_name?: string | null;
}

interface InternalHomeRawKpis {
  streams_count?: number | null;
  avg_viewers?: number | null;
  follower_delta?: number | null;
  bot_bans_keyword_count?: number | null;
}

interface InternalHomeRawStream {
  started_at?: string | null;
  ended_at?: string | null;
  duration_seconds?: number | null;
  avg_viewers?: number | null;
  peak_viewers?: number | null;
  follower_delta?: number | null;
  title?: string | null;
}

interface InternalHomeRawImpactEvent {
  type?: string | null;
  event_type?: string | null;
  timestamp?: string | null;
  title?: string | null;
  summary?: string | null;
  status_label?: string | null;
  description?: string | null;
  metric?: string | null;
  target_login?: string | null;
  target_id?: string | null;
  moderator_login?: string | null;
  actor_login?: string | null;
  reason?: string | null;
  viewer_count?: number | null;
  success?: boolean | null;
  severity?: string | null;
  source?: string | null;
}

interface InternalHomeRawChangelogEntry {
  id?: number | string | null;
  entry_date?: string | null;
  title?: string | null;
  content?: string | null;
  created_at?: string | null;
}

interface InternalHomeRawResponse {
  profile?: InternalHomeRawProfile | null;
  status?: {
    oauth?: InternalHomeRawOAuthStatus | null;
    discord?: InternalHomeRawDiscordStatus | null;
    raid_status?: InternalHomeRawRaidStatus | null;
  } | null;
  kpis?: InternalHomeRawKpis | null;
  recent_streams?: InternalHomeRawStream[] | null;
  bot_impact?: {
    events?: InternalHomeRawImpactEvent[] | null;
    note?: string | null;
  } | null;
  bot_activity?: {
    events?: InternalHomeRawImpactEvent[] | null;
  } | null;
  changelog?: {
    entries?: InternalHomeRawChangelogEntry[] | null;
    can_write?: boolean;
    max_entries?: number | null;
  } | null;
  links?: {
    oauth_reconnect?: string | null;
    profile_status?: string | null;
    discord_connect?: string | null;
  } | null;
  generated_at?: string | null;
}

function toFiniteNumber(value: unknown): number | undefined {
  if (value === null || value === undefined) {
    return undefined;
  }
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return undefined;
  }
  return numeric;
}

function mapImpactEntry(
  entry: InternalHomeRawImpactEvent,
  index: number
): InternalHomeActionEntry {
  const eventType = String(entry.event_type || entry.type || '').toLowerCase();
  const timestamp = String(entry.timestamp || '') || null;
  const target = String(entry.target_login || '').trim() || null;
  const targetId = String(entry.target_id || '').trim() || null;
  const actorLogin =
    String(entry.actor_login || entry.moderator_login || '').trim() || null;
  const reason = String(entry.reason || '').trim();
  const metric = String(entry.metric || '').trim();
  const description = String(entry.description || '').trim();
  const summary = String(entry.summary || '').trim();
  const title = String(entry.title || '').trim();
  const viewers = toFiniteNumber(entry.viewer_count);
  const severity = String(entry.severity || '').trim().toLowerCase();
  const source = String(entry.source || '').trim().toLowerCase() || null;
  const normalizedSeverity =
    severity === 'critical' || severity === 'warning' || severity === 'success' || severity === 'info'
      ? severity
      : undefined;

  if (eventType === 'ban' || eventType === 'ban_keyword_hit') {
    return {
      id: `ban-${index}`,
      timestamp,
      eventType: 'ban',
      statusLabel: entry.status_label || '[BANNED]',
      targetLogin: target,
      targetId,
      actorLogin,
      reason: reason || null,
      summary: summary || reason || (actorLogin ? `Mod: @${actorLogin}` : 'Ban ausgeführt'),
      title: title || (target ? `Ban gegen @${target}` : 'Ban ausgeführt'),
      description: description || null,
      metric: metric || null,
      severity: normalizedSeverity || 'warning',
      source,
    };
  }

  if (eventType === 'unban') {
    return {
      id: `unban-${index}`,
      timestamp,
      eventType: 'unban',
      statusLabel: entry.status_label || '[UNBANNED]',
      targetLogin: target,
      targetId,
      actorLogin,
      reason: reason || null,
      summary: summary || reason || (actorLogin ? `Unban durch @${actorLogin}` : 'Unban ausgeführt'),
      title: title || (target ? `Unban für @${target}` : 'Unban ausgeführt'),
      description: description || null,
      metric: metric || null,
      severity: normalizedSeverity || 'success',
      source,
    };
  }

  if (eventType === 'raid' || eventType === 'raid_history') {
    const success = entry.success !== false;
    return {
      id: `raid-${index}`,
      timestamp,
      eventType: 'raid',
      statusLabel: entry.status_label || '[RAID]',
      targetLogin: target,
      targetId,
      actorLogin,
      reason: reason || null,
      summary:
        summary || reason || (success ? 'Raid erfolgreich ausgeführt' : 'Raid nicht erfolgreich'),
      title: title || (target ? `Raid zu @${target}` : 'Raid-Aktivität'),
      description: description || null,
      metric:
        metric || (viewers !== undefined ? `${viewers.toLocaleString('de-DE')} Viewer` : null),
      viewerCount: viewers ?? null,
      success,
      severity: success ? 'info' : 'warning',
      source,
    };
  }

  return {
    id: `event-${index}`,
    timestamp,
    eventType: eventType || 'event',
    statusLabel: entry.status_label || '[EVENT]',
    targetLogin: target,
    targetId,
    actorLogin,
    reason: reason || null,
    summary: summary || description || reason || 'Neues Bot-Ereignis',
    title: title || 'Bot Update',
    description: description || null,
    metric: metric || null,
    viewerCount: viewers ?? null,
    success: entry.success ?? null,
    severity: normalizedSeverity || 'info',
    source,
  };
}

function mapChangelogEntry(
  entry: InternalHomeRawChangelogEntry,
  index: number
): InternalHomeChangelogEntry {
  return {
    id: entry.id ?? `changelog-${index}`,
    entryDate: entry.entry_date || null,
    title: entry.title || null,
    content: entry.content || null,
    createdAt: entry.created_at || null,
  };
}

export async function fetchInternalHome(streamer?: string | null): Promise<InternalHomeData> {
  const raw = await fetchApi<InternalHomeRawResponse>('/internal-home', {
    ...(streamer ? { streamer } : {}),
  });
  const profile = raw.profile || {};
  const status = raw.status || {};
  const oauth = status.oauth || {};
  const discord = status.discord || {};
  const raidStatus = status.raid_status || {};
  const kpis = raw.kpis || {};
  const links = raw.links || {};
  const missingScopes = oauth.missing_scopes || [];
  const loginUrl = sanitizeInternalHomeOauthUrl(
    links.oauth_reconnect || null,
    INTERNAL_HOME_LOGIN_FALLBACK
  );
  const reconnectUrl = sanitizeInternalHomeOauthUrl(
    oauth.reconnect_url || links.oauth_reconnect || null,
    INTERNAL_HOME_LOGIN_FALLBACK
  );
  const profileStatusUrl =
    oauth.profile_url || links.profile_status || oauth.reconnect_url || links.oauth_reconnect || null;
  const profileUrl = missingScopes.length > 0
    ? reconnectUrl
    : sanitizeInternalHomeOauthUrl(profileStatusUrl, INTERNAL_HOME_LOGIN_FALLBACK);
  const discordConnected = Boolean(discord.connected);
  const discordStatusRaw = String(discord.status || '').trim().toLowerCase();
  const discordStatus = discordStatusRaw || (discordConnected ? 'connected' : 'missing');
  const rawDiscordConnectUrl = discord.connect_url || links.discord_connect || null;
  const discordConnectUrl = rawDiscordConnectUrl
    ? sanitizeInternalRedirectUrl(rawDiscordConnectUrl, INTERNAL_HOME_LOGIN_FALLBACK)
    : null;

  const impactEvents = (raw.bot_impact?.events || []).map(mapImpactEntry);
  const activityEvents = (raw.bot_activity?.events || []).map(mapImpactEntry);
  const actionLog =
    activityEvents.length > 0 ? activityEvents : impactEvents;
  const note = String(raw.bot_impact?.note || '').trim();
  if (note) {
    actionLog.push({
      id: 'impact-note',
      timestamp: raw.generated_at || null,
      eventType: 'note',
      statusLabel: '[INFO]',
      summary: note,
      title: 'Hinweis',
      description: note,
      severity: 'info',
    });
  }

  const connected = Boolean(oauth.connected);
  const oauthStatus = String(oauth.status || '').toLowerCase();
  const needsReauth = Boolean((oauth as { needs_reauth?: boolean }).needs_reauth) || oauthStatus === 'reauth';
  const normalizedOauthStatus: InternalHomeOAuthStatus['status'] =
    oauthStatus === 'connected' || oauthStatus === 'partial' || oauthStatus === 'missing' || oauthStatus === 'reauth'
      ? oauthStatus
      : needsReauth
        ? 'reauth'
        : connected
        ? 'connected'
        : missingScopes.length > 0
          ? 'missing'
          : 'partial';

  return {
    greeting: profile.display_name
      ? `Willkommen zurück, ${profile.display_name}`
      : profile.twitch_login
        ? `Willkommen zurück, ${profile.twitch_login}`
        : null,
    twitchLogin: profile.twitch_login || null,
    displayName: profile.display_name || profile.twitch_login || null,
    loginUrl,
    oauth: {
      connected,
      status: normalizedOauthStatus,
      needsReauth,
      grantedScopes: oauth.granted_scopes || [],
      missingScopes,
      reconnectUrl,
      profileUrl,
      lastCheckedAt: oauth.last_checked_at || raw.generated_at || null,
    },
    discord: {
      connected: discordConnected,
      status: discordStatus,
      connectUrl: discordConnectUrl,
      lastCheckedAt: discord.last_checked_at || raw.generated_at || null,
    },
    raid: {
      active: String(raidStatus.state || '').toLowerCase() === 'active',
      statusText: String(raidStatus.state || '').toLowerCase() === 'active' ? 'Auto-Raid: Aktiv' : 'Auto-Raid: Unbekannt',
      note: raidStatus.read_only ? 'Raid-Status ist schreibgeschützt (read-only).' : null,
      lastEventAt: impactEvents[0]?.timestamp || null,
    },
    kpis30d: {
      streams: toFiniteNumber(kpis.streams_count),
      avgViewers: toFiniteNumber(kpis.avg_viewers),
      followerDelta: toFiniteNumber(kpis.follower_delta),
      banKpi: toFiniteNumber(kpis.bot_bans_keyword_count),
    },
    recentStreams: (raw.recent_streams || []).map((stream, index) => ({
      id: `stream-${index}`,
      startedAt: stream.started_at || null,
      endedAt: stream.ended_at || null,
      durationMinutes:
        stream.duration_seconds === null || stream.duration_seconds === undefined
          ? null
          : Math.round(Number(stream.duration_seconds) / 60),
      avgViewers: toFiniteNumber(stream.avg_viewers),
      peakViewers: toFiniteNumber(stream.peak_viewers),
      title: stream.title || null,
      category: null,
      followerDelta: toFiniteNumber(stream.follower_delta),
    })),
    actionLog,
    impactFeed: actionLog,
    changelog: {
      entries: (raw.changelog?.entries || []).map(mapChangelogEntry),
      canWrite: raw.changelog?.can_write !== false,
      maxEntries: toFiniteNumber(raw.changelog?.max_entries),
    },
    generatedAt: raw.generated_at || null,
  };
}

export async function createInternalHomeChangelogEntry(
  payload: CreateInternalHomeChangelogPayload
): Promise<InternalHomeChangelogEntry> {
  if (isPreviewLocalhost()) {
    return createPreviewChangelogEntry(payload);
  }

  const url = buildApiUrl('/internal-home/changelog');
  const body = await fetchJson<{
    id?: number | string;
    entry_date?: string | null;
    title?: string | null;
    content?: string | null;
    created_at?: string | null;
    error?: string;
    message?: string;
  }>(
    url,
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        title: payload.title || '',
        content: payload.content || '',
        entry_date: payload.entryDate || null,
      }),
    }),
    { loginFallback: INTERNAL_HOME_LOGIN_FALLBACK }
  );

  return {
    id: body?.id ?? undefined,
    entryDate: body?.entry_date || null,
    title: body?.title || null,
    content: body?.content || null,
    createdAt: body?.created_at || null,
  };
}
