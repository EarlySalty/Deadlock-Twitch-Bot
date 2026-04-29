import type {
  AddStreamerPayload,
  AdminActionResult,
  AdminAuthStatus,
  AdminConfigScope,
  AffiliateClaim,
  AffiliateDetail,
  AffiliateListItem,
  AffiliateStats,
  ChatConfigSnapshot,
  ChatConfigUpdatePayload,
  ConfigOverview,
  DatabaseStatsResponse,
  DiscordFlagMode,
  ErrorLogsResponse,
  EventSubStatusResponse,
  GutschriftDocument,
  InternalHomeOverview,
  LegacyVerifyMode,
  ManualPlanPayload,
  PartnerChatActionPayload,
  PiiReadiness,
  RaidConfigSnapshot,
  RaidConfigUpdatePayload,
  ScopeStatusResponse,
  StreamerDetail,
  StreamerRow,
  StreamerDiscordProfilePayload,
  StreamerView,
  SubscriptionRecord,
  SystemHealth,
} from '@/api/types';
import { coerceArray, coerceRecord } from '@/utils/formatters';

const ADMIN_API_BASE = '/twitch/api/admin';
const AUTH_STATUS_URL = '/twitch/api/v2/auth-status';
const INTERNAL_HOME_URL = '/twitch/api/v2/internal-home';
const LEGACY_CSRF_PAGE = '/twitch/admin/announcements';
let cachedCsrfToken = '';

export class ApiError extends Error {
  status: number;
  payload?: unknown;

  constructor(message: string, status: number, payload?: unknown) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.payload = payload;
  }
}

type GenerateGutschriftenParams = {
  affiliateLogin?: string;
  year?: number;
  month?: number;
  force?: boolean;
};

type GenerateGutschriftenItem = {
  ok: boolean;
  status?: string;
  action?: string;
  affiliateLogin?: string;
  periodYear?: number;
  periodMonth?: number;
  document?: GutschriftDocument;
  readiness?: PiiReadiness;
  raw?: Record<string, unknown>;
};

type GenerateGutschriftenResult = {
  ok: boolean;
  results: GenerateGutschriftenItem[];
  raw?: Record<string, unknown>;
};

function sanitizeNextPath(rawPath: string): string {
  if (!rawPath || !rawPath.startsWith('/') || rawPath.startsWith('//') || rawPath.includes('\\')) {
    return '/twitch/admin';
  }
  return rawPath;
}

export function buildDiscordAdminLoginUrl(nextPath?: string): string {
  const next = sanitizeNextPath(nextPath || `${window.location.pathname}${window.location.search}`);
  return `/twitch/auth/discord/login?next=${encodeURIComponent(next)}`;
}

export function buildRaidAuthUrl(login: string): string {
  return `/twitch/raid/auth?login=${encodeURIComponent(login.trim())}`;
}

export function buildRaidRequirementsUrl(login: string): string {
  return `/twitch/raid/requirements?login=${encodeURIComponent(login.trim())}`;
}

async function parsePayload(response: Response): Promise<unknown> {
  const contentType = response.headers.get('content-type') || '';
  if (contentType.includes('application/json')) {
    return response.json().catch(() => null);
  }
  return response.text().catch(() => '');
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: 'include',
    headers: {
      Accept: 'application/json',
      ...(init?.headers ?? {}),
    },
    ...init,
  });
  const payload = await parsePayload(response);
  if (!response.ok) {
    const record = coerceRecord(payload);
    const message =
      typeof record.message === 'string'
        ? record.message
        : typeof record.error === 'string'
          ? record.error
          : `HTTP ${response.status}`;
    if (response.status === 401 || response.status === 403) {
      const loginUrl =
        typeof record.loginUrl === 'string'
          ? record.loginUrl
          : typeof record.discordLoginUrl === 'string'
            ? record.discordLoginUrl
            : buildDiscordAdminLoginUrl();
      throw new ApiError(loginUrl, response.status, payload);
    }
    throw new ApiError(message, response.status, payload);
  }
  return payload as T;
}

function admin<T>(suffix: string, init?: RequestInit) {
  return request<T>(`${ADMIN_API_BASE}${suffix}`, init);
}

function cacheCsrfToken(token: string | undefined | null): string {
  const normalized = typeof token === 'string' ? token.trim() : '';
  if (normalized) {
    cachedCsrfToken = normalized;
  }
  return normalized;
}

function readString(record: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    if (typeof record[key] === 'string') {
      return String(record[key]);
    }
  }
  return '';
}

function readNumber(record: Record<string, unknown>, ...keys: string[]): number | undefined {
  for (const key of keys) {
    const rawValue = record[key];
    if (rawValue === undefined || rawValue === null || rawValue === '') {
      continue;
    }
    const candidate = Number(rawValue);
    if (Number.isFinite(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

function readBoolean(record: Record<string, unknown>, ...keys: string[]): boolean | undefined {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'boolean') {
      return value;
    }
    if (typeof value === 'number') {
      return value !== 0;
    }
    if (typeof value === 'string') {
      const normalized = value.trim().toLowerCase();
      if (normalized) {
        return !['0', 'false', 'off', 'no'].includes(normalized);
      }
    }
  }
  return undefined;
}

function readScope(record: Record<string, unknown>, ...keys: string[]): AdminConfigScope | undefined {
  const candidate = readString(record, ...keys).trim().toLowerCase();
  if (candidate === 'active' || candidate === 'all') {
    return candidate;
  }
  return undefined;
}

function readPartnerStatus(
  record: Record<string, unknown>,
  ...keys: string[]
): 'active' | 'archived' | 'departnered' | 'non_partner' | 'token_error' | 'blocked' | undefined {
  const candidate = readString(record, ...keys).trim().toLowerCase();
  if (
    candidate === 'active' ||
    candidate === 'archived' ||
    candidate === 'departnered' ||
    candidate === 'non_partner' ||
    candidate === 'token_error' ||
    candidate === 'blocked'
  ) {
    return candidate;
  }
  return undefined;
}

function readStringArray(value: unknown): string[] {
  return coerceArray<unknown>(value)
    .map((entry) => (typeof entry === 'string' ? entry.trim() : String(entry ?? '').trim()))
    .filter(Boolean);
}

function readStringRecord(value: unknown): Record<string, string> {
  return Object.fromEntries(
    Object.entries(coerceRecord(value)).map(([key, entry]) => [key, String(entry ?? '').trim()]),
  );
}

function normalizeAffiliateStatus(active: boolean): string {
  return active ? 'active' : 'inactive';
}

function normalizeDownloadPath(value: string): string | null {
  const normalized = value.trim();
  if (!normalized) {
    return null;
  }
  if (normalized.startsWith('http://') || normalized.startsWith('https://') || normalized.startsWith('/')) {
    return normalized;
  }
  return `/${normalized.replace(/^\/+/, '')}`;
}

function emptyAffiliateStats(overrides: Partial<AffiliateStats> = {}): AffiliateStats {
  return {
    totalClaims: 0,
    totalProvisionEuro: 0,
    thisMonthClaims: 0,
    thisMonthProvisionEuro: 0,
    ...overrides,
  };
}

function emptyReadiness(overrides: Partial<PiiReadiness> = {}): PiiReadiness {
  return {
    canGenerate: false,
    blockers: [],
    warnings: [],
    missingFields: [],
    ustStatus: 'unknown',
    ...overrides,
  };
}

function parseAffiliateStats(record: Record<string, unknown>): AffiliateStats {
  return emptyAffiliateStats({
    totalAffiliates: readNumber(record, 'totalAffiliates', 'total_affiliates'),
    activeAffiliates: readNumber(record, 'activeAffiliates', 'active_affiliates'),
    totalClaims: readNumber(record, 'totalClaims', 'total_claims') ?? 0,
    totalProvisionEuro:
      readNumber(record, 'totalProvisionEuro', 'total_provision_euro', 'totalProvision', 'total_provision') ?? 0,
    thisMonthClaims: readNumber(record, 'thisMonthClaims', 'this_month_claims'),
    thisMonthProvisionEuro: readNumber(
      record,
      'thisMonthProvisionEuro',
      'this_month_provision_euro',
      'thisMonthProvision',
      'this_month_provision',
    ),
    avgProvisionEuro: readNumber(record, 'avgProvisionEuro', 'avg_provision_euro', 'avgProvision', 'avg_provision'),
    activeCustomers: readNumber(record, 'activeCustomers', 'active_customers'),
    raw: record,
  });
}

function parseAffiliateClaim(record: Record<string, unknown>): AffiliateClaim {
  return {
    id: readNumber(record, 'id', 'claimId', 'claim_id'),
    customerLogin:
      readString(record, 'customerLogin', 'customer_login', 'claimed_streamer_login', 'streamer_login') || '—',
    claimedAt: readString(record, 'claimedAt', 'claimed_at') || null,
    commissionCents:
      readNumber(record, 'commissionCents', 'commission_cents', 'totalCommissionCents', 'total_commission_cents') ?? 0,
    commissionCount: readNumber(record, 'commissionCount', 'commission_count') ?? 0,
    raw: record,
  };
}

function parsePiiReadiness(record: Record<string, unknown>): PiiReadiness {
  if (!Object.keys(record).length) {
    return emptyReadiness({ raw: record });
  }
  return emptyReadiness({
    canGenerate: readBoolean(record, 'canGenerate', 'can_generate') ?? false,
    blockers: readStringArray(record.blockers),
    warnings: readStringArray(record.warnings),
    missingFields: readStringArray(record.missingFields ?? record.missing_fields),
    ustStatus: readString(record, 'ustStatus', 'ust_status') || 'unknown',
    raw: record,
  });
}

function parseGutschriftDocument(
  record: Record<string, unknown>,
  defaults: Partial<GutschriftDocument> = {},
): GutschriftDocument {
  const downloadPath = normalizeDownloadPath(readString(record, 'downloadPath', 'download_path'));
  const generatedAt =
    readString(record, 'generatedAt', 'generated_at', 'pdfGeneratedAt', 'pdf_generated_at') || null;
  const emailedAt = readString(record, 'emailedAt', 'emailed_at', 'emailSentAt', 'email_sent_at') || null;
  const lastError = readString(record, 'lastError', 'last_error', 'emailError', 'email_error') || '';
  const hasPdf = readBoolean(record, 'hasPdf', 'has_pdf') ?? Boolean(downloadPath || generatedAt);
  const explicitStatus = readString(record, 'status').trim().toLowerCase();
  const status =
    explicitStatus ||
    (emailedAt ? 'emailed' : lastError ? 'email_failed' : hasPdf ? 'generated' : 'blocked');

  return {
    id: readNumber(record, 'id'),
    affiliateLogin:
      readString(record, 'affiliateLogin', 'affiliate_login', 'affiliateTwitchLogin', 'affiliate_twitch_login') ||
      defaults.affiliateLogin,
    affiliateDisplayName:
      readString(record, 'affiliateDisplayName', 'affiliate_display_name', 'displayName', 'display_name') ||
      defaults.affiliateDisplayName,
    periodYear: readNumber(record, 'periodYear', 'period_year'),
    periodMonth: readNumber(record, 'periodMonth', 'period_month'),
    periodLabel: readString(record, 'periodLabel', 'period_label') || undefined,
    gutschriftNumber: readString(record, 'gutschriftNumber', 'gutschrift_number') || undefined,
    status,
    netAmountCents: readNumber(record, 'netAmountCents', 'net_amount_cents') ?? 0,
    vatAmountCents: readNumber(record, 'vatAmountCents', 'vat_amount_cents') ?? 0,
    grossAmountCents: readNumber(record, 'grossAmountCents', 'gross_amount_cents') ?? 0,
    commissionCount: readNumber(record, 'commissionCount', 'commission_count') ?? 0,
    generatedAt,
    emailedAt,
    createdAt: readString(record, 'createdAt', 'created_at') || null,
    noteText: readString(record, 'noteText', 'note_text') || undefined,
    lastError: lastError || undefined,
    downloadPath,
    hasPdf,
    affiliateUstStatus: readString(record, 'affiliateUstStatus', 'affiliate_ust_status', 'ustStatus', 'ust_status') || undefined,
    raw: record,
  };
}

function parseGutschriftCollection(
  payload: unknown,
  defaults: Partial<GutschriftDocument> = {},
): GutschriftDocument[] {
  if (Array.isArray(payload)) {
    return payload.map((entry) => parseGutschriftDocument(coerceRecord(entry), defaults));
  }

  const record = coerceRecord(payload);
  const directItems = coerceArray<Record<string, unknown>>(record.gutschriften ?? record.documents ?? record.items);
  if (directItems.length) {
    return directItems.map((entry) => parseGutschriftDocument(coerceRecord(entry), defaults));
  }

  const resultItems = coerceArray<Record<string, unknown>>(record.results).map((entry) =>
    coerceRecord(entry.document ?? entry.gutschrift ?? entry),
  );
  if (resultItems.length) {
    return resultItems.map((entry) => parseGutschriftDocument(entry, defaults));
  }

  return [];
}

function parseAffiliateListItem(record: Record<string, unknown>): AffiliateListItem {
  const active = readBoolean(record, 'active', 'isActive', 'is_active') ?? true;
  const explicitStatus = readString(record, 'status').trim().toLowerCase();
  return {
    login: readString(record, 'login', 'twitchLogin', 'twitch_login') || '—',
    displayName: readString(record, 'displayName', 'display_name') || undefined,
    active,
    totalClaims: readNumber(record, 'totalClaims', 'total_claims') ?? 0,
    totalProvisionEuro:
      readNumber(record, 'totalProvisionEuro', 'total_provision_euro', 'totalProvision', 'total_provision') ?? 0,
    createdAt: readString(record, 'createdAt', 'created_at') || null,
    lastClaimAt: readString(record, 'lastClaimAt', 'last_claim_at') || null,
    updatedAt: readString(record, 'updatedAt', 'updated_at') || null,
    stripeConnectStatus: readString(record, 'stripeConnectStatus', 'stripe_connect_status') || undefined,
    status: explicitStatus || normalizeAffiliateStatus(active),
    raw: record,
  };
}

function parseAffiliateDetail(payload: unknown, fallbackLogin: string): AffiliateDetail {
  const record = coerceRecord(payload);
  const affiliateRecord = coerceRecord(record.affiliate);
  const profileRecord = coerceRecord(record.profile);
  const merged = { ...affiliateRecord, ...profileRecord };
  const fallbackAffiliateLogin = readString(merged, 'login', 'twitchLogin', 'twitch_login') || fallbackLogin;
  const fallbackDisplayName = readString(merged, 'displayName', 'display_name') || undefined;
  const readiness = parsePiiReadiness(
    coerceRecord(
      record.readiness ??
        record.gutschriftReadiness ??
        record.gutschrift_readiness ??
        merged.gutschriftReadiness ??
        merged.gutschrift_readiness,
    ),
  );
  const stats = parseAffiliateStats(coerceRecord(record.stats ?? merged.stats));

  return {
    login: fallbackAffiliateLogin,
    displayName: fallbackDisplayName,
    active: readBoolean(merged, 'active', 'isActive', 'is_active') ?? false,
    email: readString(merged, 'email', 'payoutEmail', 'payout_email') || undefined,
    fullName: readString(merged, 'fullName', 'full_name') || undefined,
    addressLine1: readString(merged, 'addressLine1', 'address_line1') || undefined,
    addressCity: readString(merged, 'addressCity', 'address_city') || undefined,
    addressZip: readString(merged, 'addressZip', 'address_zip') || undefined,
    addressCountry: readString(merged, 'addressCountry', 'address_country') || undefined,
    taxId: readString(merged, 'taxId', 'tax_id') || undefined,
    vatId: readString(merged, 'vatId', 'vat_id') || undefined,
    ustStatus: readString(merged, 'ustStatus', 'ust_status') || readiness.ustStatus || undefined,
    stripeConnectStatus: readString(merged, 'stripeConnectStatus', 'stripe_connect_status') || undefined,
    stripeAccountId: readString(merged, 'stripeAccountId', 'stripe_account_id') || undefined,
    createdAt: readString(merged, 'createdAt', 'created_at') || null,
    updatedAt: readString(merged, 'updatedAt', 'updated_at') || null,
    profileUpdatedAt: readString(merged, 'profileUpdatedAt', 'profile_updated_at') || null,
    stats,
    claims: coerceArray<Record<string, unknown>>(record.claims).map((entry) => parseAffiliateClaim(coerceRecord(entry))),
    readiness,
    gutschriften: parseGutschriftCollection(record.gutschriften ?? record.documents ?? merged.gutschriften, {
      affiliateLogin: fallbackAffiliateLogin,
      affiliateDisplayName: fallbackDisplayName,
    }),
    raw: record,
  };
}

function parseGenerateGutschriftenResult(payload: unknown): GenerateGutschriftenResult {
  const record = coerceRecord(payload);
  const results = coerceArray<Record<string, unknown>>(record.results).map((entry) => {
    const item = coerceRecord(entry);
    const affiliateLogin =
      readString(item, 'affiliateLogin', 'affiliate_login', 'login', 'twitch_login') || undefined;
    return {
      ok: readBoolean(item, 'ok') ?? false,
      status: readString(item, 'status') || undefined,
      action: readString(item, 'action') || undefined,
      affiliateLogin,
      periodYear: readNumber(item, 'periodYear', 'period_year'),
      periodMonth: readNumber(item, 'periodMonth', 'period_month'),
      document: Object.keys(coerceRecord(item.document)).length
        ? parseGutschriftDocument(coerceRecord(item.document), { affiliateLogin })
        : undefined,
      readiness: Object.keys(coerceRecord(item.readiness)).length
        ? parsePiiReadiness(coerceRecord(item.readiness))
        : undefined,
      raw: item,
    };
  });

  return {
    ok: readBoolean(record, 'ok') ?? results.some((entry) => entry.ok),
    results,
    raw: record,
  };
}

async function adminFirst<T>(suffixes: string[]): Promise<T | null> {
  for (const suffix of suffixes) {
    try {
      return await admin<T>(suffix);
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) {
        continue;
      }
      throw error;
    }
  }
  return null;
}

function parseRaidSnapshot(record: Record<string, unknown>): RaidConfigSnapshot {
  return {
    totalManagedStreamers: readNumber(record, 'totalManagedStreamers', 'total_managed_streamers'),
    raidBotEnabledCount: readNumber(record, 'raidBotEnabledCount', 'raid_bot_enabled_count'),
    livePingEnabledCount: readNumber(record, 'livePingEnabledCount', 'live_ping_enabled_count'),
    allRaidBotEnabled: readBoolean(record, 'allRaidBotEnabled', 'all_raid_bot_enabled'),
    allLivePingEnabled: readBoolean(record, 'allLivePingEnabled', 'all_live_ping_enabled'),
    scope: readScope(record, 'scope', 'defaultScope', 'default_scope'),
    raw: record,
  };
}

function parseChatSnapshot(record: Record<string, unknown>): ChatConfigSnapshot {
  return {
    totalManagedStreamers: readNumber(record, 'totalManagedStreamers', 'total_managed_streamers'),
    silentBanCount: readNumber(record, 'silentBanCount', 'silent_ban_count'),
    silentRaidCount: readNumber(record, 'silentRaidCount', 'silent_raid_count'),
    allSilentBan: readBoolean(record, 'allSilentBan', 'all_silent_ban'),
    allSilentRaid: readBoolean(record, 'allSilentRaid', 'all_silent_raid'),
    scope: readScope(record, 'scope', 'defaultScope', 'default_scope'),
    raw: record,
  };
}

export async function fetchAuthStatus(): Promise<AdminAuthStatus> {
  try {
    const payload = await request<Record<string, unknown>>(AUTH_STATUS_URL);
    const csrfToken = cacheCsrfToken(readString(payload, 'csrfToken', 'csrf_token')) || undefined;
    const authLevel = readString(payload, 'authLevel', 'auth_level').toLowerCase();
    const isAdmin = Boolean(payload.isAdmin ?? payload.is_admin) || authLevel === 'admin';
    const isLocalhost =
      Boolean(payload.isLocalhost ?? payload.is_localhost) || authLevel === 'localhost';
    return {
      authenticated: Boolean(payload.authenticated) || isAdmin || isLocalhost || !!authLevel,
      authLevel,
      isAdmin,
      isLocalhost,
      loginUrl: readString(payload, 'loginUrl', 'login_url') || buildDiscordAdminLoginUrl(),
      discordLoginUrl:
        readString(payload, 'discordLoginUrl', 'discord_login_url', 'loginUrl', 'login_url') ||
        buildDiscordAdminLoginUrl(),
      csrfToken,
      user: {
        displayName: readString(payload, 'displayName', 'display_name', 'twitchLogin', 'login') || undefined,
        login: readString(payload, 'twitchLogin', 'login') || undefined,
        authType: readString(payload, 'authType', 'auth_type') || undefined,
      },
      permissions: coerceRecord(payload.permissions),
    };
  } catch (error) {
    if (error instanceof ApiError) {
      return {
        authenticated: false,
        loginUrl: error.message || buildDiscordAdminLoginUrl(),
        discordLoginUrl: error.message || buildDiscordAdminLoginUrl(),
      };
    }
    throw error;
  }
}

export async function fetchDashboardOverview(): Promise<InternalHomeOverview> {
  const payload = await request<Record<string, unknown>>(INTERNAL_HOME_URL);
  return {
    metrics: coerceArray(payload.metrics) as InternalHomeOverview['metrics'],
    actions: coerceArray(payload.actions),
    recentActivity: coerceArray(payload.recentActivity ?? payload.recent_activity ?? payload.activity),
    raw: payload,
  };
}

export async function fetchAdminStreamers(view: StreamerView = 'active'): Promise<StreamerRow[]> {
  const query = view ? `?view=${encodeURIComponent(view)}` : '';
  const payload = await admin<unknown>(`/streamers${query}`);
  const rows = Array.isArray(payload)
    ? payload
    : coerceArray<Record<string, unknown>>(coerceRecord(payload).items ?? coerceRecord(payload).streamers);
  return rows.map((row) => {
    const record = coerceRecord(row);
    return {
      login: readString(record, 'login', 'twitch_login', 'streamer_login'),
      displayName: readString(record, 'displayName', 'display_name', 'login') || undefined,
      twitchUserId: readString(record, 'twitchUserId', 'twitch_user_id') || undefined,
      discordUserId: readString(record, 'discordUserId', 'discord_user_id') || undefined,
      discordDisplayName: readString(record, 'discordDisplayName', 'discord_display_name') || undefined,
      verified: readBoolean(record, 'verified', 'is_verified'),
      archived: readBoolean(record, 'archived', 'is_archived'),
      archivedAt: readString(record, 'archivedAt', 'archived_at') || null,
      createdAt: readString(record, 'createdAt', 'created_at') || null,
      isLive: readBoolean(record, 'isLive', 'is_live'),
      isOnDiscord: readBoolean(record, 'isOnDiscord', 'is_on_discord'),
      manualPartnerOptOut: readBoolean(record, 'manualPartnerOptOut', 'manual_partner_opt_out'),
      partnerStatus: readPartnerStatus(record, 'partnerStatus', 'partner_status'),
      viewerCount: readNumber(record, 'viewerCount', 'viewer_count'),
      activeSessionId: readNumber(record, 'activeSessionId', 'active_session_id') ?? null,
      lastSeenAt: readString(record, 'lastSeenAt', 'last_seen_at') || null,
      lastGame: readString(record, 'lastGame', 'last_game') || null,
      lastStreamAt: readString(record, 'lastStreamAt', 'last_stream_at') || null,
      planId: readString(record, 'planId', 'plan_id') || undefined,
      billingStatus: readString(record, 'billingStatus', 'billing_status') || undefined,
      oauthConnected: readBoolean(record, 'oauthConnected', 'oauth_connected'),
      oauthNeedsReauth: readBoolean(record, 'oauthNeedsReauth', 'oauth_needs_reauth'),
      oauthStatus: readString(record, 'oauthStatus', 'oauth_status') || undefined,
      grantedScopes: readStringArray(record.grantedScopes ?? record.granted_scopes),
      missingScopes: readStringArray(record.missingScopes ?? record.missing_scopes),
      oauthAuthorizedAt: readString(record, 'oauthAuthorizedAt', 'oauth_authorized_at') || null,
      promoDisabled: readBoolean(record, 'promoDisabled', 'promo_disabled'),
      notes: readString(record, 'notes', 'manual_plan_notes') || undefined,
      status: readString(record, 'status') || undefined,
      raw: record,
    };
  });
}

export async function fetchAdminStreamerDetail(login: string): Promise<StreamerDetail> {
  const payload = await admin<Record<string, unknown>>(`/streamers/${encodeURIComponent(login)}`);
  const sessions = coerceArray<Record<string, unknown>>(payload.sessions).map((session) => ({
    sessionId: readNumber(session, 'sessionId', 'session_id'),
    startedAt: readString(session, 'startedAt', 'started_at') || undefined,
    endedAt: readString(session, 'endedAt', 'ended_at') || undefined,
    title: readString(session, 'title') || undefined,
    category: readString(session, 'category', 'game_name') || undefined,
    averageViewers: readNumber(session, 'averageViewers', 'avg_viewers'),
    peakViewers: readNumber(session, 'peakViewers', 'peak_viewers'),
    watchTimeHours: readNumber(session, 'watchTimeHours', 'watch_time_hours', 'watch_hours'),
    followerDelta: readNumber(session, 'followerDelta', 'follower_delta'),
  }));
  return {
    login: readString(payload, 'login', 'twitch_login') || login,
    displayName: readString(payload, 'displayName', 'display_name', 'login') || undefined,
    twitchUserId: readString(payload, 'twitchUserId', 'twitch_user_id') || undefined,
    verified: readBoolean(payload, 'verified', 'is_verified'),
    archived: readBoolean(payload, 'archived', 'is_archived'),
    archivedAt: readString(payload, 'archivedAt', 'archived_at') || null,
    createdAt: readString(payload, 'createdAt', 'created_at') || null,
    isLive: readBoolean(payload, 'isLive', 'is_live'),
    partnerStatus: readPartnerStatus(payload, 'partnerStatus', 'partner_status'),
    planId: readString(payload, 'planId', 'plan_id') || undefined,
    stats: coerceRecord(payload.stats),
    settings: coerceRecord(payload.settings),
    sessions,
    recentActivity: coerceArray(payload.recentActivity ?? payload.recent_activity),
    raw: payload,
  };
}

export async function fetchSystemHealth(): Promise<SystemHealth> {
  const payload = await admin<Record<string, unknown>>('/system/health');
  return {
    uptimeSeconds: readNumber(payload, 'uptimeSeconds', 'uptime_seconds', 'uptime'),
    memoryBytes: readNumber(payload, 'memoryBytes', 'memory_bytes'),
    memoryRssBytes: readNumber(payload, 'memoryRssBytes', 'memory_rss_bytes'),
    pythonVersion: readString(payload, 'pythonVersion', 'python_version') || undefined,
    processId: readNumber(payload, 'processId', 'pid'),
    lastTickAt: readString(payload, 'lastTickAt', 'last_tick_at') || undefined,
    lastTickAgeSeconds: readNumber(payload, 'lastTickAgeSeconds', 'last_tick_age_seconds'),
    rawChatLagSeconds: readNumber(payload, 'rawChatLagSeconds', 'raw_chat_lag_seconds'),
    rawChatLagStreamer: readString(payload, 'rawChatLagStreamer', 'raw_chat_lag_streamer') || undefined,
    rawChatLastMessageAt: readString(payload, 'rawChatLastMessageAt', 'raw_chat_last_message_at') || undefined,
    rawChatLastInsertOkAt: readString(payload, 'rawChatLastInsertOkAt', 'raw_chat_last_insert_ok_at') || undefined,
    rawChatLastInsertErrorAt: readString(payload, 'rawChatLastInsertErrorAt', 'raw_chat_last_insert_error_at') || undefined,
    rawChatLastError: readString(payload, 'rawChatLastError', 'raw_chat_last_error') || undefined,
    analyticsDbFingerprint: readString(payload, 'analyticsDbFingerprint', 'analytics_db_fingerprint') || undefined,
    internalAnalyticsDbFingerprint: readString(payload, 'internalAnalyticsDbFingerprint', 'internal_analytics_db_fingerprint') || undefined,
    analyticsDbFingerprintMismatch: readBoolean(payload, 'analyticsDbFingerprintMismatch', 'analytics_db_fingerprint_mismatch'),
    serviceWarnings: coerceArray(payload.serviceWarnings ?? payload.service_warnings),
    raw: payload,
  };
}

export async function fetchScopeStatus(): Promise<ScopeStatusResponse> {
  const payload = await admin<Record<string, unknown>>('/system/oauth-scopes');
  const summaryRecord = coerceRecord(payload.summary);
  return {
    requiredScopes: readStringArray(payload.requiredScopes ?? payload.required_scopes),
    criticalScopes: readStringArray(payload.criticalScopes ?? payload.critical_scopes),
    labels: readStringRecord(payload.labels),
    summary: {
      totalAuthorized: readNumber(summaryRecord, 'totalAuthorized', 'total_authorized') ?? 0,
      fullScopeCount: readNumber(summaryRecord, 'fullScopeCount', 'full_scope_count') ?? 0,
      missingScopeCount: readNumber(summaryRecord, 'missingScopeCount', 'missing_scope_count') ?? 0,
    },
    items: coerceArray<Record<string, unknown>>(payload.items).map((record) => {
      const item = coerceRecord(record);
      return {
        login: readString(item, 'login', 'twitch_login') || '—',
        displayName: readString(item, 'displayName', 'display_name', 'login') || undefined,
        partnerStatus: readPartnerStatus(item, 'partnerStatus', 'partner_status'),
        archivedAt: readString(item, 'archivedAt', 'archived_at') || null,
        oauthStatus: readString(item, 'oauthStatus', 'oauth_status') || undefined,
        oauthNeedsReauth: readBoolean(item, 'oauthNeedsReauth', 'oauth_needs_reauth'),
        grantedScopes: readStringArray(item.grantedScopes ?? item.granted_scopes),
        missingScopes: readStringArray(item.missingScopes ?? item.missing_scopes),
      };
    }),
  };
}

export async function fetchEventSubStatus(): Promise<EventSubStatusResponse> {
  return admin<EventSubStatusResponse>('/system/eventsub');
}

export async function fetchDatabaseStats(): Promise<DatabaseStatsResponse> {
  return admin<DatabaseStatsResponse>('/system/database');
}

export async function fetchErrorLogs(page = 1, pageSize = 25): Promise<ErrorLogsResponse> {
  return admin<ErrorLogsResponse>(`/system/errors?page=${page}&page_size=${pageSize}`);
}

export async function fetchConfigOverview(scope?: AdminConfigScope): Promise<ConfigOverview> {
  const query = scope ? `?scope=${encodeURIComponent(scope)}` : '';
  const payload = await admin<Record<string, unknown>>(`/config/overview${query}`);
  const csrfToken = cacheCsrfToken(readString(payload, 'csrfToken', 'csrf_token')) || undefined;
  return {
    promo: coerceRecord(payload.promo),
    raids: parseRaidSnapshot(coerceRecord(payload.raids)),
    chat: parseChatSnapshot(coerceRecord(payload.chat)),
    announcements: coerceRecord(payload.announcements),
    csrfToken,
    raw: payload,
  };
}

function readBodyCsrfToken(body: Record<string, unknown>): string {
  const rawToken = body.csrf_token ?? body.csrfToken;
  return typeof rawToken === 'string' ? rawToken.trim() : '';
}

async function resolveJsonCsrfToken(body: Record<string, unknown>): Promise<string> {
  const explicitToken = cacheCsrfToken(readBodyCsrfToken(body));
  if (explicitToken) {
    return explicitToken;
  }
  if (cachedCsrfToken) {
    return cachedCsrfToken;
  }

  try {
    const auth = await fetchAuthStatus();
    const authToken = cacheCsrfToken(auth.csrfToken);
    if (authToken) {
      return authToken;
    }
  } catch {
    // Fall back to the legacy admin page token read below.
  }

  return cacheCsrfToken(await fetchLegacyCsrfToken());
}

async function postAdminJson<T, TBody extends object = Record<string, unknown>>(path: string, body: TBody) {
  const normalizedBody = coerceRecord(body);
  const csrfToken = await resolveJsonCsrfToken(normalizedBody);
  const payload = { ...normalizedBody, csrf_token: csrfToken };
  return admin<T>(path, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': csrfToken,
    },
    body: JSON.stringify(payload),
  });
}

async function postJson<T, TBody extends object = Record<string, unknown>>(path: string, body: TBody) {
  const normalizedBody = coerceRecord(body);
  const csrfToken = await resolveJsonCsrfToken(normalizedBody);
  const payload = { ...normalizedBody, csrf_token: csrfToken };
  return request<T>(path, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-CSRF-Token': csrfToken,
    },
    body: JSON.stringify(payload),
  });
}

async function postAdminFirstJson<T, TBody extends object = Record<string, unknown>>(
  suffixes: string[],
  body: TBody,
): Promise<T | null> {
  for (const suffix of suffixes) {
    try {
      return await postAdminJson<T, TBody>(suffix, body);
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) {
        continue;
      }
      throw error;
    }
  }
  return null;
}

export async function updatePromoConfig(body: Record<string, unknown>) {
  return postAdminJson<Record<string, unknown>>('/config/promo', body);
}

export async function updateRaidConfig(body: RaidConfigUpdatePayload) {
  return postAdminJson<Record<string, unknown>, RaidConfigUpdatePayload>('/config/raids', body);
}

export async function updateChatConfig(body: ChatConfigUpdatePayload) {
  return postAdminJson<Record<string, unknown>, ChatConfigUpdatePayload>('/config/chat', body);
}

export async function fetchSubscriptions(): Promise<SubscriptionRecord[]> {
  const payload = await admin<unknown>('/billing/subscriptions');
  return Array.isArray(payload)
    ? (payload as SubscriptionRecord[])
    : (coerceArray(coerceRecord(payload).items ?? coerceRecord(payload).subscriptions) as SubscriptionRecord[]);
}

export async function fetchAffiliatesList(): Promise<AffiliateListItem[]> {
  try {
    const payload = await adminFirst<unknown>(['/affiliates', '/billing/affiliates']);
    if (payload === null) {
      return [];
    }
    const rows = Array.isArray(payload)
      ? payload
      : coerceArray<Record<string, unknown>>(coerceRecord(payload).items ?? coerceRecord(payload).affiliates);
    return rows.map((row) => parseAffiliateListItem(coerceRecord(row)));
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      return [];
    }
    throw error;
  }
}

export async function fetchAffiliateStats(): Promise<AffiliateStats> {
  try {
    const payload = await admin<Record<string, unknown>>('/affiliates/stats');
    return parseAffiliateStats(payload);
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      return emptyAffiliateStats({ totalAffiliates: 0, activeAffiliates: 0 });
    }
    throw error;
  }
}

export async function fetchAffiliateDetail(login: string): Promise<AffiliateDetail> {
  try {
    const payload = await admin<unknown>(`/affiliates/${encodeURIComponent(login)}`);
    return parseAffiliateDetail(payload, login);
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      return {
        login,
        active: false,
        stats: emptyAffiliateStats(),
        claims: [],
        readiness: emptyReadiness(),
        gutschriften: [],
        raw: {},
      };
    }
    throw error;
  }
}

export async function toggleAffiliateActive(login: string): Promise<{ login: string; active: boolean }> {
  const payload = await postAdminJson<Record<string, unknown>>(`/affiliates/${encodeURIComponent(login)}/toggle`, {});
  return {
    login: readString(payload, 'login', 'twitchLogin', 'twitch_login') || login,
    active: readBoolean(payload, 'active', 'isActive', 'is_active') ?? false,
  };
}

export async function fetchAllGutschriften(): Promise<GutschriftDocument[]> {
  try {
    const payload = await adminFirst<unknown>(['/gutschriften', '/billing/gutschriften', '/affiliates/gutschriften']);
    if (payload === null) {
      return [];
    }
    return parseGutschriftCollection(payload);
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      return [];
    }
    throw error;
  }
}

export async function fetchAffiliateGutschriften(login: string): Promise<GutschriftDocument[]> {
  const encodedLogin = encodeURIComponent(login);
  try {
    const payload = await adminFirst<unknown>([
      `/affiliates/${encodedLogin}/gutschriften`,
      `/gutschriften?affiliate_login=${encodedLogin}`,
      `/billing/gutschriften?affiliate_login=${encodedLogin}`,
      `/billing/gutschriften?login=${encodedLogin}`,
    ]);
    if (payload !== null) {
      return parseGutschriftCollection(payload, { affiliateLogin: login });
    }
  } catch (error) {
    if (!(error instanceof ApiError && error.status === 404)) {
      throw error;
    }
  }

  const detail = await fetchAffiliateDetail(login);
  return detail.gutschriften;
}

export async function generateGutschriften(
  params: GenerateGutschriftenParams = {},
): Promise<GenerateGutschriftenResult> {
  const requestBody = Object.fromEntries(
    Object.entries({
      affiliate_login: params.affiliateLogin?.trim() || undefined,
      year: params.year,
      month: params.month,
      force: params.force ? true : undefined,
    }).filter(([, value]) => value !== undefined && value !== ''),
  );

  const adminPayload = await postAdminFirstJson<Record<string, unknown>>(
    ['/gutschriften/generate', '/billing/gutschriften/generate'],
    requestBody,
  );
  if (adminPayload !== null) {
    return parseGenerateGutschriftenResult(adminPayload);
  }

  const legacyPayload = await postJson<Record<string, unknown>>(
    '/twitch/api/affiliate/admin/generate-gutschriften',
    requestBody,
  );
  return parseGenerateGutschriftenResult(legacyPayload);
}

export async function fetchLegacyCsrfToken(): Promise<string> {
  if (cachedCsrfToken) {
    return cachedCsrfToken;
  }
  const response = await fetch(LEGACY_CSRF_PAGE, {
    credentials: 'include',
    headers: { Accept: 'text/html' },
  });
  const html = await response.text();
  const match =
    html.match(/name=["']csrf_token["'][^>]*value=["']([^"']+)["']/i) ??
    html.match(/value=["']([^"']+)["'][^>]*name=["']csrf_token["']/i);
  if (!match?.[1]) {
    throw new ApiError('CSRF-Token konnte nicht gelesen werden.', 500);
  }
  return cacheCsrfToken(match[1]);
}

async function submitLegacyAction(path: string, fields: Record<string, string>): Promise<AdminActionResult> {
  const csrfToken = fields.csrf_token || cachedCsrfToken || (await fetchLegacyCsrfToken());
  const body = new URLSearchParams({ ...fields, csrf_token: csrfToken });
  const response = await fetch(path, {
    method: 'POST',
    credentials: 'include',
    headers: {
      Accept: 'text/html',
      'Content-Type': 'application/x-www-form-urlencoded;charset=UTF-8',
    },
    body,
    redirect: 'follow',
  });
  if (!response.ok) {
    throw new ApiError(`Aktion fehlgeschlagen (${response.status})`, response.status);
  }
  const finalUrl = new URL(response.url, window.location.origin);
  return {
    ok: !finalUrl.searchParams.get('err'),
    message: finalUrl.searchParams.get('ok') || finalUrl.searchParams.get('err') || 'Aktion ausgeführt.',
    redirectUrl: `${finalUrl.pathname}${finalUrl.search}`,
  };
}

export function addStreamer(payload: string | AddStreamerPayload) {
  const normalized: Record<string, string> =
    typeof payload === 'string'
      ? { login: payload }
      : {
          login: payload.login,
          discord_user_id: payload.discordUserId?.trim() || '',
          discord_display_name: payload.discordDisplayName?.trim() || '',
          member_flag: payload.memberFlag ? '1' : '',
        };
  return submitLegacyAction('/twitch/add_streamer', normalized);
}

export function removeStreamer(login: string) {
  return submitLegacyAction('/twitch/remove', { login });
}

export function verifyStreamer(login: string, mode: LegacyVerifyMode = 'permanent') {
  return submitLegacyAction('/twitch/verify', { login, mode });
}

export function archiveStreamer(login: string, mode: 'archive' | 'unarchive' | 'toggle' = 'toggle') {
  return submitLegacyAction('/twitch/archive', { login, mode });
}

export function blockStreamer(login: string, mode: 'block' | 'unblock' | 'toggle_block' = 'toggle_block') {
  return submitLegacyAction('/twitch/archive', { login, mode });
}

export function updateStreamerDiscordProfile(payload: StreamerDiscordProfilePayload) {
  return submitLegacyAction('/twitch/discord_link', {
    login: payload.login,
    discord_user_id: payload.discordUserId?.trim() || '',
    discord_display_name: payload.discordDisplayName?.trim() || '',
    member_flag: payload.memberFlag ? '1' : '',
  });
}

export function toggleStreamerDiscordFlag(login: string, mode: DiscordFlagMode) {
  return submitLegacyAction('/twitch/discord_flag', { login, mode });
}

export function saveManualPlanOverride(payload: ManualPlanPayload) {
  return submitLegacyAction('/twitch/admin/manual-plan', {
    login: payload.login,
    plan_id: payload.planId,
    expires_at: payload.expiresAt?.trim() || '',
    notes: payload.notes?.trim() || '',
  });
}

export function clearManualPlanOverride(login: string) {
  return submitLegacyAction('/twitch/admin/manual-plan/clear', { login });
}

export function sendPartnerChatAction(payload: PartnerChatActionPayload) {
  return submitLegacyAction('/twitch/admin/chat_action', {
    login: payload.login,
    mode: payload.mode,
    color: payload.color || 'purple',
    message: payload.message,
  });
}
