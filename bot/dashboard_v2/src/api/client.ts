// API Client for Twitch Analytics Dashboard

import type {
  AIHistoryEntry,
  AdsSchedule,
  MonetizationStats,
  CategoryTimings,
  CategoryActivitySeries,
  DashboardOverview,
  MonthlyStats,
  WeekdayStats,
  HourlyHeatmapData,
  CalendarHeatmapData,
  ChatAnalytics,
  ViewerOverlap,
  TagPerformance,
  TagPerformanceExtended,
  TagAnalysisResponse,
  TitlePerformance,
  TitlePerformanceResponse,
  RankingEntry,
  SessionEvent,
  StreamSession,
  TimeRange,
  CategoryComparison,
  WatchTimeDistribution,
  FollowerFunnel,
  AudienceInsights,
  ViewerTimelinePoint,
  CategoryLeaderboard,
  CoachingData,
  LurkerAnalysis,
  RaidRetention,
  RaidAnalytics,
  ViewerProfiles,
  AudienceSharing,
  ViewerDirectory,
  ViewerDetail,
  ViewerSegments,
  ViewerSortField,
  ViewerFilterType,
  ChatHypeTimeline,
  ChatContentAnalysis,
  ChatSocialGraph,
  ExpOverview,
  ExpGameBreakdown,
  ExpGameTransition,
  ExpGrowthCurve,
  AIAnalysisResult,
} from '@/types/analytics';
import { dashboardRuntimeConfig } from '../runtimeConfig';

const API_BASE = dashboardRuntimeConfig.apiBase;
const INTERNAL_REDIRECT_PREFIX = '/twitch';
const INTERNAL_HOME_LOGIN_FALLBACK = '/twitch/auth/login?next=%2Ftwitch%2Fdashboard';
const INTERNAL_HOME_DISCORD_CONNECT_FALLBACK = '/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard';
const DASHBOARD_V2_LOGIN_FALLBACK = '/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2';
const INTERNAL_HOME_BLOCKED_OAUTH_PATHS = ['/twitch/raid/requirements'] as const;
const PARTNER_TOKEN_STORAGE_KEY = 'partner_token';
let partnerTokenMemory: string | null = null;

function getPartnerTokenStorage(): Storage | null {
  if (typeof window === 'undefined') {
    return null;
  }
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

function readPartnerToken(): string | null {
  const storage = getPartnerTokenStorage();
  if (storage) {
    const stored = storage.getItem(PARTNER_TOKEN_STORAGE_KEY);
    return stored ? stored.trim() || null : null;
  }
  return partnerTokenMemory ? partnerTokenMemory.trim() || null : null;
}

function writePartnerToken(token: string | null): void {
  const normalized = String(token || '').trim();
  const storage = getPartnerTokenStorage();
  if (storage) {
    if (normalized) {
      storage.setItem(PARTNER_TOKEN_STORAGE_KEY, normalized);
    } else {
      storage.removeItem(PARTNER_TOKEN_STORAGE_KEY);
    }
    return;
  }
  partnerTokenMemory = normalized || null;
}

export function getPartnerToken(): string | null {
  return readPartnerToken();
}

export function capturePartnerTokenFromUrl(search?: string): string | null {
  if (typeof window === 'undefined') {
    return null;
  }
  const urlParams = new URLSearchParams(search ?? window.location.search ?? '');
  const token = urlParams.get('partner_token')?.trim() || '';
  if (!token) {
    return readPartnerToken();
  }
  writePartnerToken(token);
  return token;
}

export function scrubPartnerTokenFromUrl(): void {
  if (typeof window === 'undefined' || !window.location || !window.history) {
    return;
  }
  const nextUrl = new URL(window.location.href);
  if (!nextUrl.searchParams.has('partner_token')) {
    return;
  }
  nextUrl.searchParams.delete('partner_token');
  const cleanUrl = `${nextUrl.pathname}${nextUrl.search ? nextUrl.search : ''}${nextUrl.hash}`;
  window.history.replaceState({}, '', cleanUrl);
}

function bootstrapPartnerTokenFromUrl(): void {
  if (typeof window === 'undefined') {
    return;
  }
  const token = capturePartnerTokenFromUrl(window.location.search);
  if (token) {
    scrubPartnerTokenFromUrl();
  }
}

bootstrapPartnerTokenFromUrl();

function isAllowedInternalRedirectPath(pathname: string): boolean {
  return pathname === INTERNAL_REDIRECT_PREFIX || pathname.startsWith(`${INTERNAL_REDIRECT_PREFIX}/`);
}

function sanitizeInternalRedirectUrl(rawUrl: string | null | undefined, fallback: string): string {
  const fallbackCandidate = (fallback || '').trim();
  let safeFallback = DASHBOARD_V2_LOGIN_FALLBACK;
  if (fallbackCandidate && fallbackCandidate.startsWith('/') && !fallbackCandidate.startsWith('//')) {
    try {
      const parsedFallback = new URL(fallbackCandidate, window.location.origin);
      if (
        parsedFallback.origin === window.location.origin &&
        isAllowedInternalRedirectPath(parsedFallback.pathname)
      ) {
        safeFallback = `${parsedFallback.pathname}${parsedFallback.search}${parsedFallback.hash}`;
      }
    } catch {
      safeFallback = DASHBOARD_V2_LOGIN_FALLBACK;
    }
  }

  const candidate = (rawUrl || '').trim();
  if (!candidate) {
    return safeFallback;
  }

  if (!candidate.startsWith('/') || candidate.startsWith('//') || candidate.includes('\\')) {
    return safeFallback;
  }

  try {
    const parsed = new URL(candidate, window.location.origin);
    if (parsed.origin !== window.location.origin) {
      return safeFallback;
    }
    const normalized = `${parsed.pathname}${parsed.search}${parsed.hash}`;
    if (!isAllowedInternalRedirectPath(parsed.pathname)) {
      return safeFallback;
    }
    return normalized;
  } catch {
    return safeFallback;
  }
}

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

function withPartnerHeaders(headers: HeadersInit = {}): Headers {
  const resolved = new Headers(headers);
  const token = getPartnerToken();
  if (token) {
    resolved.set('X-Partner-Token', token);
  }
  return resolved;
}

// Helper to build URL with params
export function buildApiUrl(
  endpoint: string,
  params: Record<string, string | number | boolean> = {}
): string {
  const url = new URL(`${API_BASE}${endpoint}`, window.location.origin);
  Object.entries(params).forEach(([key, value]) => {
    if (value !== undefined && value !== null) {
      url.searchParams.set(key, String(value));
    }
  });
  return url.toString();
}

function getBrowserTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

// Generic fetch wrapper
export async function fetchApi<T>(endpoint: string, params: Record<string, string | number | boolean> = {}, timeoutMs?: number): Promise<T> {
  const url = buildApiUrl(endpoint, params);

  const abortCtrl = timeoutMs ? new AbortController() : null;
  const timer = abortCtrl ? setTimeout(() => abortCtrl.abort(), timeoutMs!) : null;

  const response = await fetch(url, {
    headers: withPartnerHeaders({ 'Accept': 'application/json' }),
    signal: abortCtrl?.signal,
  }).finally(() => {
    if (timer) clearTimeout(timer);
  });

  if (response.status === 401) {
    const unauthorized = await response.json().catch(() => null) as
      | { error?: string; loginUrl?: string }
      | null;
    if (unauthorized?.loginUrl) {
      window.location.href = sanitizeInternalRedirectUrl(
        unauthorized.loginUrl,
        DASHBOARD_V2_LOGIN_FALLBACK
      );
      throw new Error('Redirecting to Twitch login');
    }
    throw new Error(unauthorized?.error || 'Unauthorized');
  }

  if (!response.ok) {
    const error = await response.json().catch(() => null);
    throw new Error(error?.error || `Server-Fehler (HTTP ${response.status})`);
  }

  return response.json();
}

// API Functions

export async function fetchOverview(
  streamer: string | null,
  days: TimeRange
): Promise<DashboardOverview> {
  return fetchApi<DashboardOverview>('/overview', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchMonthlyStats(
  streamer: string | null,
  months: number = 12
): Promise<MonthlyStats[]> {
  return fetchApi<MonthlyStats[]>('/monthly-stats', {
    streamer: streamer || '',
    months,
  });
}

export async function fetchWeekdayStats(
  streamer: string | null,
  days: TimeRange
): Promise<WeekdayStats[]> {
  return fetchApi<WeekdayStats[]>('/weekly-stats', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchHourlyHeatmap(
  streamer: string | null,
  days: TimeRange
): Promise<HourlyHeatmapData[]> {
  return fetchApi<HourlyHeatmapData[]>('/hourly-heatmap', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchCalendarHeatmap(
  streamer: string | null,
  days: number = 365
): Promise<CalendarHeatmapData[]> {
  return fetchApi<CalendarHeatmapData[]>('/calendar-heatmap', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchChatAnalytics(
  streamer: string | null,
  days: TimeRange
): Promise<ChatAnalytics> {
  return fetchApi<ChatAnalytics>('/chat-analytics', {
    streamer: streamer || '',
    days,
    timezone: getBrowserTimezone(),
  });
}

export async function fetchViewerOverlap(
  streamer: string | null,
  limit: number = 20
): Promise<ViewerOverlap[]> {
  return fetchApi<ViewerOverlap[]>('/viewer-overlap', {
    streamer: streamer || '',
    limit,
  });
}

export async function fetchTagAnalysis(
  days: TimeRange,
  limit: number = 30
): Promise<TagPerformance[]> {
  return fetchApi<TagPerformance[]>('/tag-analysis', {
    days,
    limit,
  });
}

export async function fetchRankings(
  metric: 'viewers' | 'growth' | 'retention' | 'chat',
  days: TimeRange,
  limit: number = 20,
  excludeExternal = true
): Promise<RankingEntry[]> {
  return fetchApi<RankingEntry[]>('/rankings', {
    metric,
    days,
    limit,
    ...(excludeExternal && { exclude_external: '1' }),
  });
}

export async function fetchSessionDetail(
  sessionId: number
): Promise<StreamSession & { timeline: { minute: number; viewers: number }[]; chatters: { login: string; messages: number }[] }> {
  return fetchApi(`/session/${sessionId}`);
}

export async function fetchSessionEvents(
  sessionId: number
): Promise<SessionEvent> {
  return fetchApi<SessionEvent>(`/session/${sessionId}/events`);
}

export async function fetchStreamerList(): Promise<{ login: string; isPartner: boolean }[]> {
  return fetchApi<{ login: string; isPartner: boolean }[]>('/streamers');
}

// Auth Status
export interface AuthStatus {
  authenticated: boolean;
  level: 'localhost' | 'admin' | 'partner' | 'none';
  demoMode: boolean;
  isAdmin: boolean;
  isLocalhost: boolean;
  canViewAllStreamers: boolean;
  twitchLogin?: string | null;
  displayName?: string | null;
  csrfToken?: string | null;
  csrf_token?: string | null;
  permissions: {
    viewAllStreamers: boolean;
    viewComparison: boolean;
    viewChatAnalytics: boolean;
    viewOverlap: boolean;
  };
  plan?: {
    planId: string | null;
    planName: string | null;
    tier: import('../types/billing').PlanTier;
    isExtended: boolean;
    expiresAt: string | null;
    source: string | null;
    entitlements: import('../types/billing').EntitlementId[];
  } | null;
}

export async function fetchAuthStatus(): Promise<AuthStatus> {
  return fetchApi<AuthStatus>('/auth-status');
}

// Internal Home
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
  const discordConnectUrl = sanitizeInternalRedirectUrl(
    discord.connect_url || links.discord_connect || null,
    INTERNAL_HOME_DISCORD_CONNECT_FALLBACK
  );

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

export interface CreateInternalHomeChangelogPayload {
  title?: string;
  content: string;
  entryDate?: string;
}

export async function createInternalHomeChangelogEntry(
  payload: CreateInternalHomeChangelogPayload
): Promise<InternalHomeChangelogEntry> {
  const url = buildApiUrl('/internal-home/changelog');
  const response = await fetch(url, {
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
  });

  if (response.status === 401) {
    const unauthorized = await response.json().catch(() => null) as
      | { error?: string; loginUrl?: string }
      | null;
    if (unauthorized?.loginUrl) {
      window.location.href = sanitizeInternalRedirectUrl(
        unauthorized.loginUrl,
        '/twitch/auth/login?next=%2Ftwitch%2Fdashboard'
      );
      throw new Error('Redirecting to Twitch login');
    }
    throw new Error(unauthorized?.error || 'Unauthorized');
  }

  const body = await response.json().catch(() => null) as
    | {
        id?: number | string;
        entry_date?: string | null;
        title?: string | null;
        content?: string | null;
        created_at?: string | null;
        error?: string;
        message?: string;
      }
    | null;

  if (!response.ok) {
    throw new Error(body?.message || body?.error || `Server-Fehler (HTTP ${response.status})`);
  }

  return {
    id: body?.id ?? undefined,
    entryDate: body?.entry_date || null,
    title: body?.title || null,
    content: body?.content || null,
    createdAt: body?.created_at || null,
  };
}

// Category Comparison
export async function fetchCategoryComparison(
  streamer: string | null,
  days: TimeRange,
  excludeExternal = true
): Promise<CategoryComparison> {
  return fetchApi('/category-comparison', {
    streamer: streamer || '',
    days,
    ...(excludeExternal && { exclude_external: '1' }),
  });
}

// Watch Time Distribution
export async function fetchWatchTimeDistribution(
  streamer: string | null,
  days: TimeRange
): Promise<WatchTimeDistribution> {
  return fetchApi<WatchTimeDistribution>('/watch-time-distribution', {
    streamer: streamer || '',
    days,
  });
}

// Follower Funnel
export async function fetchFollowerFunnel(
  streamer: string | null,
  days: TimeRange
): Promise<FollowerFunnel> {
  return fetchApi<FollowerFunnel>('/follower-funnel', {
    streamer: streamer || '',
    days,
  });
}

// Extended Tag Analysis with Trends
export async function fetchTagAnalysisExtended(
  streamer: string | null,
  days: TimeRange,
  limit: number = 20
): Promise<TagAnalysisResponse> {
  const raw = await fetchApi<TagAnalysisResponse | TagPerformanceExtended[]>('/tag-analysis-extended', {
    streamer: streamer || '',
    days,
    limit,
  });
  // Handle both old (array) and new (object with tags/peerBenchmark) response formats
  if (Array.isArray(raw)) {
    return { tags: raw, peerBenchmark: null };
  }
  return raw;
}

// Title Performance Analysis
export async function fetchTitlePerformance(
  streamer: string | null,
  days: TimeRange,
  limit: number = 20
): Promise<TitlePerformanceResponse> {
  const raw = await fetchApi<TitlePerformanceResponse | TitlePerformance[]>('/title-performance', {
    streamer: streamer || '',
    days,
    limit,
  });
  // Handle both old (array) and new (object with titles/peerBenchmark) response formats
  if (Array.isArray(raw)) {
    return { titles: raw, peerBenchmark: null };
  }
  return raw;
}

// Combined Audience Insights (all in one call)
export async function fetchAudienceInsights(
  streamer: string | null,
  days: TimeRange
): Promise<AudienceInsights> {
  return fetchApi<AudienceInsights>('/audience-insights', {
    streamer: streamer || '',
    days,
  });
}

// Audience Demographics
export interface AudienceDemographicsResponse {
  viewerTypes: { label: string; percentage: number }[];
  activityPattern: 'weekend-heavy' | 'weekday-focused' | 'balanced';
  primaryLanguage: string;
  languageConfidence: number;
  peakActivityHours: number[];
  peakHoursMethod?: string;
  chatPenetrationPct?: number | null;
  chatPenetrationReliable?: boolean;
  messagesPer100ViewerMinutes?: number | null;
  viewerMinutes?: number;
  legacyInteractionActivePerAvgViewer?: number | null;
  interactiveRate: number;
  // Legacy fields (temporary compatibility)
  interactionRateActivePerViewer?: number;
  interactionRateActivePerAvgViewer?: number | null;
  interactionRateReliable?: boolean;
  loyaltyScore: number;
  timezone?: string;
  dataQuality?: {
    confidence: 'very_low' | 'low' | 'medium' | 'high';
    sessions?: number;
    method?: 'no_data' | 'low_coverage' | 'real_samples' | string;
    peakMethod?: 'no_data' | 'low_coverage' | 'real_samples' | string;
    coverage?: number;
    sampleCount?: number;
    peakSessionCount?: number;
    peakSessionsWithActivity?: number;
    interactiveSampleCount?: number;
    interactionCoverage?: number;
    chattersCoverage?: number;
    chattersApiCoverage?: number;
    passiveViewerSamples?: number;
    sessionsWithChat?: number;
    chatSessionCoverage?: number;
    viewerSampleCount?: number;
    viewerMinutesSource?: 'real_samples' | 'low_coverage' | string;
  };
}

export async function fetchAudienceDemographics(
  streamer: string | null,
  days: TimeRange
): Promise<AudienceDemographicsResponse> {
  return fetchApi<AudienceDemographicsResponse>('/audience-demographics', {
    streamer: streamer || '',
    days,
    timezone: getBrowserTimezone(),
  });
}

// Viewer Timeline (bucketed stats_tracked data)
export async function fetchViewerTimeline(
  streamer: string | null,
  days: number
): Promise<ViewerTimelinePoint[]> {
  return fetchApi<ViewerTimelinePoint[]>('/viewer-timeline', {
    streamer: streamer || '',
    days,
  });
}

// Coaching Data
export async function fetchCoaching(
  streamer: string,
  days: TimeRange
): Promise<CoachingData> {
  return fetchApi<CoachingData>('/coaching', {
    streamer,
    days,
  });
}

// Category Timings (Median-basiert, outlier-resistent)
export async function fetchCategoryTimings(
  days: TimeRange,
  source: 'category' | 'tracked' = 'category'
): Promise<CategoryTimings> {
  return fetchApi<CategoryTimings>('/category-timings', { days, source });
}

export async function fetchCategoryActivitySeries(
  days: TimeRange
): Promise<CategoryActivitySeries> {
  return fetchApi<CategoryActivitySeries>('/category-activity-series', { days });
}

// Monetization & Hype Train
export async function fetchMonetization(
  streamer: string | null,
  days: TimeRange
): Promise<MonetizationStats> {
  return fetchApi<MonetizationStats>('/monetization', {
    streamer: streamer || '',
    days,
  });
}

// Ads Schedule
export async function fetchAdsSchedule(
  streamer: string
): Promise<AdsSchedule> {
  return fetchApi<AdsSchedule>('/ads-schedule', {
    streamer,
  });
}

// Lurker Analysis
export async function fetchLurkerAnalysis(
  streamer: string | null,
  days: TimeRange
): Promise<LurkerAnalysis> {
  return fetchApi<LurkerAnalysis>('/lurker-analysis', {
    streamer: streamer || '',
    days,
  });
}

// Raid Retention
export async function fetchRaidRetention(
  streamer: string | null,
  days: TimeRange
): Promise<RaidRetention> {
  return fetchApi<RaidRetention>('/raid-retention', {
    streamer: streamer || '',
    days,
  });
}

// Raid Analytics (includes incoming raids)
export async function fetchRaidAnalytics(
  streamer: string | null,
  days: TimeRange
): Promise<RaidAnalytics> {
  return fetchApi<RaidAnalytics>('/raid-analytics', {
    streamer: streamer || '',
    days,
  });
}

// Viewer Profiles
export async function fetchViewerProfiles(
  streamer: string | null,
  days: TimeRange
): Promise<ViewerProfiles> {
  return fetchApi<ViewerProfiles>('/viewer-profiles', {
    streamer: streamer || '',
    days,
  });
}

// Audience Sharing
export async function fetchAudienceSharing(
  streamer: string | null,
  days: TimeRange
): Promise<AudienceSharing> {
  return fetchApi<AudienceSharing>('/audience-sharing', {
    streamer: streamer || '',
    days,
  });
}

// Viewer Directory (paginated, filtered, sorted)
export async function fetchViewerDirectory(
  streamer: string | null,
  days: TimeRange,
  sort: ViewerSortField = 'sessions',
  order: 'asc' | 'desc' = 'desc',
  filter: ViewerFilterType = 'all',
  search: string = '',
  page: number = 1,
  perPage: number = 50
): Promise<ViewerDirectory> {
  return fetchApi<ViewerDirectory>('/viewer-directory', {
    streamer: streamer || '',
    days,
    sort,
    order,
    filter,
    ...(search && { search }),
    page,
    per_page: perPage,
  });
}

// Viewer Detail (single viewer deep-dive)
export async function fetchViewerDetail(
  streamer: string | null,
  login: string,
  days: TimeRange
): Promise<ViewerDetail> {
  return fetchApi<ViewerDetail>('/viewer-detail', {
    streamer: streamer || '',
    login,
    days,
  });
}

// Viewer Segments (segmentation + churn risk)
export async function fetchViewerSegments(
  streamer: string | null,
  days: TimeRange
): Promise<ViewerSegments> {
  return fetchApi<ViewerSegments>('/viewer-segments', {
    streamer: streamer || '',
    days,
  });
}

// Chat Hype Timeline
export async function fetchChatHypeTimeline(
  streamer: string | null,
  sessionId?: number
): Promise<ChatHypeTimeline> {
  return fetchApi<ChatHypeTimeline>('/chat-hype-timeline', {
    streamer: streamer || '',
    ...(sessionId != null && { session_id: sessionId }),
  });
}

// Chat Content Analysis (hero mentions, topics, sentiment)
export async function fetchChatContentAnalysis(
  streamer: string | null,
  days: number
): Promise<ChatContentAnalysis> {
  return fetchApi<ChatContentAnalysis>('/chat-content-analysis', {
    streamer: streamer || '',
    days,
  });
}

// Chat Social Graph (@mentions network)
export async function fetchChatSocialGraph(
  streamer: string | null,
  days: number
): Promise<ChatSocialGraph> {
  return fetchApi<ChatSocialGraph>('/chat-social-graph', {
    streamer: streamer || '',
    days,
  });
}

// Category Leaderboard (top-N from stats_category)
export async function fetchCategoryLeaderboard(
  streamer: string | null,
  days: number,
  limit: number = 25,
  sort: 'avg' | 'peak' = 'avg',
  excludeExternal = false,
  tier?: string | null
): Promise<CategoryLeaderboard> {
  return fetchApi<CategoryLeaderboard>('/category-leaderboard', {
    streamer: streamer || '',
    days,
    limit,
    sort,
    ...(excludeExternal && { exclude_external: '1' }),
    ...(tier && { tier }),
  });
}

// ========= Experimental (Labor) API Functions =========

export async function fetchExpOverview(
  streamer: string,
  days: number
): Promise<ExpOverview> {
  return fetchApi<ExpOverview>("/exp/overview", { streamer, days });
}

export async function fetchExpGameBreakdown(
  streamer: string,
  days: number
): Promise<ExpGameBreakdown[]> {
  return fetchApi<ExpGameBreakdown[]>("/exp/game-breakdown", { streamer, days });
}

export async function fetchExpGameTransitions(
  streamer: string,
  days: number
): Promise<ExpGameTransition[]> {
  return fetchApi<ExpGameTransition[]>("/exp/game-transitions", { streamer, days });
}

export async function fetchExpGrowthCurves(
  streamer: string,
  days: number
): Promise<ExpGrowthCurve[]> {
  return fetchApi<ExpGrowthCurve[]>("/exp/growth-curves", { streamer, days });
}

// ========= KI Analyse (AI Analysis) =========

export async function fetchAIAnalysis(
  streamer: string,
  days: number,
  gameFilter: 'deadlock' | 'all' = 'all'
): Promise<AIAnalysisResult> {
  return fetchApi<AIAnalysisResult>('/ai/analysis', { streamer, days, game_filter: gameFilter }, 240_000);
}

export async function fetchAIHistory(
  streamer: string,
  limit = 20
): Promise<AIHistoryEntry[]> {
  return fetchApi<AIHistoryEntry[]>('/ai/history', { streamer, limit });
}

// ========= Roadmap =========

export interface RoadmapItem {
  id: number;
  title: string;
  description: string | null;
  status: 'planned' | 'in_progress' | 'done';
  priority: number;
  created_at: string | null;
  updated_at: string | null;
}

export interface RoadmapData {
  planned: RoadmapItem[];
  in_progress: RoadmapItem[];
  done: RoadmapItem[];
}

export async function fetchRoadmap(): Promise<RoadmapData> {
  return fetchApi<RoadmapData>('/roadmap');
}

// ── Billing ────────────────────────────────────────
export type { CatalogPlan } from '../types/billing';
import type { CatalogPlan } from '../types/billing';

export async function fetchBillingCatalog(): Promise<{ plans: CatalogPlan[] }> {
  return fetchApi<{ plans: CatalogPlan[] }>('/billing/catalog');
}

// ── Admin Affiliates ───────────────────────────────
export interface Affiliate {
  login: string;
  display_name: string;
  active: boolean;
  total_claims: number;
  total_provision: number;
  created_at: string;
  last_claim_at: string | null;
}

export interface AffiliateStats {
  total_affiliates: number;
  active_affiliates: number;
  total_claims: number;
  total_provision: number;
  this_month_claims: number;
  this_month_provision: number;
}

export interface AffiliateDetail {
  affiliate: { login: string; display_name: string; active: boolean; created_at: string };
  claims: Array<{
    id: number;
    customer_login: string;
    claimed_at: string;
    commission_cents: number;
    commission_count: number;
  }>;
  stats: { total_claims: number; total_provision: number; avg_provision: number; active_customers: number };
}

export interface AffiliatePortalData {
  affiliate: { login: string; display_name: string; active: boolean; referral_code: string; referral_url: string };
  stats: { total_claims: number; total_provision: number; this_month_claims: number; this_month_provision: number; pending_payout: number };
  recent_claims: Array<{ customer_display_name: string; plan_name: string | null; amount: number; created_at: string }>;
}

async function fetchAdminApi<T>(path: string): Promise<T> {
  const url = new URL(`/twitch/api/admin${path}`, window.location.origin);
  const response = await fetch(url.toString(), {
    headers: withPartnerHeaders({ 'Accept': 'application/json' }),
  });

  if (response.status === 401) {
    const unauthorized = await response.json().catch(() => null) as
      | { error?: string; loginUrl?: string }
      | null;
    if (unauthorized?.loginUrl) {
      window.location.href = sanitizeInternalRedirectUrl(
        unauthorized.loginUrl,
        DASHBOARD_V2_LOGIN_FALLBACK
      );
      throw new Error('Redirecting to Twitch login');
    }
    throw new Error(unauthorized?.error || 'Unauthorized');
  }

  if (!response.ok) {
    const error = await response.json().catch(() => null);
    throw new Error(error?.error || `Server-Fehler (HTTP ${response.status})`);
  }

  return response.json();
}

export async function fetchAdminAffiliates(): Promise<{ affiliates: Affiliate[] }> {
  return fetchAdminApi<{ affiliates: Affiliate[] }>('/affiliates');
}

export async function fetchAdminAffiliateStats(): Promise<AffiliateStats> {
  return fetchAdminApi<AffiliateStats>('/affiliates/stats');
}

export async function fetchAdminAffiliateDetail(login: string): Promise<AffiliateDetail> {
  return fetchAdminApi<AffiliateDetail>(`/affiliates/${login}`);
}

export async function toggleAffiliate(
  login: string,
  csrfToken: string | null | undefined
): Promise<{ login: string; active: boolean }> {
  if (!csrfToken) {
    throw new Error('Missing CSRF token');
  }
  const url = new URL(`/twitch/api/admin/affiliates/${login}/toggle`, window.location.origin);
  const response = await fetch(url.toString(), {
    method: 'POST',
    headers: withPartnerHeaders({
      'Accept': 'application/json',
      'X-CSRF-Token': csrfToken,
    }),
  });

  if (response.status === 401) {
    const unauthorized = await response.json().catch(() => null) as
      | { error?: string; loginUrl?: string }
      | null;
    if (unauthorized?.loginUrl) {
      window.location.href = sanitizeInternalRedirectUrl(
        unauthorized.loginUrl,
        DASHBOARD_V2_LOGIN_FALLBACK
      );
      throw new Error('Redirecting to Twitch login');
    }
    throw new Error(unauthorized?.error || 'Unauthorized');
  }

  if (!response.ok) {
    const error = await response.json().catch(() => null);
    throw new Error(error?.error || `Server-Fehler (HTTP ${response.status})`);
  }

  return response.json();
}

export async function fetchAffiliatePortal(): Promise<AffiliatePortalData> {
  const url = buildApiUrl('/affiliate/portal');
  const response = await fetch(url, {
    headers: withPartnerHeaders({ 'Accept': 'application/json' }),
  });

  if (response.status === 401) {
    const unauthorized = await response.json().catch(() => null) as
      | { error?: string; loginUrl?: string }
      | null;
    if (unauthorized?.loginUrl) {
      window.location.href = sanitizeInternalRedirectUrl(
        unauthorized.loginUrl,
        DASHBOARD_V2_LOGIN_FALLBACK
      );
      throw new Error('Redirecting to Twitch login');
    }
    throw new Error(unauthorized?.error || 'Unauthorized');
  }

  if (response.status === 404) {
    throw new Error('affiliate_not_found');
  }

  if (!response.ok) {
    const error = await response.json().catch(() => null);
    throw new Error(error?.error || `Server-Fehler (HTTP ${response.status})`);
  }

  return response.json();
}
