export interface AdminUserInfo {
  displayName?: string;
  username?: string;
  login?: string;
  userId?: string;
  authType?: string;
}

export interface AdminAuthStatus {
  authenticated: boolean;
  authLevel?: string;
  isAdmin?: boolean;
  isLocalhost?: boolean;
  loginUrl?: string;
  discordLoginUrl?: string;
  csrfToken?: string;
  user?: AdminUserInfo;
  permissions?: Record<string, unknown>;
}

export type AdminConfigScope = 'active' | 'all';
export type StreamerView = 'active' | 'archived' | 'departnered' | 'non_partner' | 'token_error' | 'blocked' | 'all';
export type StreamerPartnerStatus = 'active' | 'archived' | 'departnered' | 'non_partner' | 'token_error' | 'blocked';
export type LegacyVerifyMode = 'permanent' | 'temp' | 'failed' | 'clear';
export type DiscordFlagMode = 'mark' | 'unmark';
export type PartnerChatActionMode = 'message' | 'action' | 'announcement';
export type PartnerChatAnnouncementColor = 'blue' | 'green' | 'orange' | 'purple' | 'primary';

export interface StreamerRow {
  login: string;
  displayName?: string;
  twitchUserId?: string;
  discordUserId?: string;
  discordDisplayName?: string;
  verified?: boolean;
  archived?: boolean;
  archivedAt?: string | null;
  createdAt?: string | null;
  isLive?: boolean;
  isOnDiscord?: boolean;
  manualPartnerOptOut?: boolean;
  partnerStatus?: StreamerPartnerStatus;
  viewerCount?: number;
  activeSessionId?: number | null;
  lastSeenAt?: string | null;
  lastGame?: string | null;
  lastStreamAt?: string | null;
  planId?: string;
  billingStatus?: string;
  oauthConnected?: boolean;
  oauthNeedsReauth?: boolean;
  oauthStatus?: string;
  grantedScopes?: string[];
  missingScopes?: string[];
  oauthAuthorizedAt?: string | null;
  partnerSince?: string | null;
  promoDisabled?: boolean;
  notes?: string;
  status?: string;
  raw?: Record<string, unknown>;
}

export interface SessionSummary {
  sessionId?: number;
  startedAt?: string;
  endedAt?: string;
  title?: string;
  category?: string;
  averageViewers?: number;
  peakViewers?: number;
  watchTimeHours?: number;
  followerDelta?: number;
}

export interface StreamerDetail {
  login: string;
  displayName?: string;
  twitchUserId?: string;
  verified?: boolean;
  archived?: boolean;
  archivedAt?: string | null;
  createdAt?: string | null;
  isLive?: boolean;
  partnerStatus?: StreamerPartnerStatus;
  planId?: string;
  stats?: Record<string, unknown>;
  settings?: Record<string, unknown>;
  sessions?: SessionSummary[];
  recentActivity?: Record<string, unknown>[];
  raw?: Record<string, unknown>;
}

export interface StreamerDiscordProfilePayload {
  login: string;
  discordUserId?: string;
  discordDisplayName?: string;
  memberFlag?: boolean;
}

export interface AddStreamerPayload extends StreamerDiscordProfilePayload {}

export interface ManualPlanPayload {
  login: string;
  planId: string;
  expiresAt?: string;
  notes?: string;
}

export interface PartnerChatActionPayload {
  login: string;
  mode: PartnerChatActionMode;
  color?: PartnerChatAnnouncementColor;
  message: string;
}

export interface ScopeStatusRow {
  login: string;
  displayName?: string;
  partnerStatus?: StreamerPartnerStatus;
  archivedAt?: string | null;
  oauthStatus?: string;
  oauthNeedsReauth?: boolean;
  grantedScopes: string[];
  missingScopes: string[];
}

export interface ScopeStatusSummary {
  totalAuthorized: number;
  fullScopeCount: number;
  missingScopeCount: number;
}

export interface ScopeStatusResponse {
  requiredScopes: string[];
  criticalScopes: string[];
  labels?: Record<string, string>;
  summary: ScopeStatusSummary;
  items: ScopeStatusRow[];
}

export interface InternalHomeMetric {
  label: string;
  value: string | number;
  hint?: string;
}

export interface InternalHomeOverview {
  metrics?: InternalHomeMetric[];
  actions?: Record<string, unknown>[];
  recentActivity?: Record<string, unknown>[];
  changelog?: ChangelogEntry[];
  raw?: Record<string, unknown>;
}

export interface AdminTextDocument {
  body: string;
  lastUpdatedAt?: string | null;
  lastUpdatedBy?: string | null;
}

export interface AuditLogEntry {
  id: string;
  source: string;
  action: string;
  actor?: string | null;
  target?: string | null;
  timestamp: string;
  description: string;
  metadata?: Record<string, unknown> | null;
}

export interface AuditLogResponse {
  entries: AuditLogEntry[];
  sources: string[];
  totalCount: number;
  hasMore: boolean;
}

export type LegalPageSlug = 'impressum' | 'datenschutz' | 'agb';

export interface LegalPageDocument extends AdminTextDocument {
  slug: LegalPageSlug;
  title: string;
}

export interface ChangelogEntry {
  id?: number | string | null;
  entryDate?: string | null;
  title: string;
  content: string;
  createdAt?: string | null;
}

export interface CreateChangelogEntryPayload {
  title?: string;
  content: string;
  entry_date: string;
}

export interface SystemHealth {
  uptimeSeconds?: number;
  memoryBytes?: number;
  memoryRssBytes?: number;
  pythonVersion?: string;
  processId?: number;
  lastTickAt?: string;
  lastTickAgeSeconds?: number;
  rawChatLagSeconds?: number;
  rawChatLagStreamer?: string;
  rawChatLastMessageAt?: string;
  rawChatLastInsertOkAt?: string;
  rawChatLastInsertErrorAt?: string;
  rawChatLastError?: string;
  analyticsDbFingerprint?: string;
  internalAnalyticsDbFingerprint?: string;
  analyticsDbFingerprintMismatch?: boolean;
  serviceWarnings?: Record<string, unknown>[];
  raw?: Record<string, unknown>;
}

export interface EventSubSubscription {
  id?: string;
  type?: string;
  status?: string;
  transport?: string;
  createdAt?: string;
  cost?: number;
  condition?: Record<string, unknown>;
}

export interface EventSubStatusResponse {
  websocketStatus?: string;
  transportMode?: string;
  websocketSessionId?: string;
  websocketConnectedAt?: string;
  websocketReconnectedAt?: string;
  activeSubscriptionCount?: number;
  capacity?: {
    used?: number;
    max?: number;
    remaining?: number;
    lastSnapshotAt?: string;
  };
  subscriptions?: EventSubSubscription[];
  lastKnownSubscriptions?: EventSubSubscription[];
  lastKnownSnapshotAt?: string;
  snapshotStale?: boolean;
  raw?: Record<string, unknown>;
}

export interface DatabaseTableStat {
  table: string;
  rowCount?: number;
  sizeBytes?: number;
  updatedAt?: string;
}

export interface DatabaseStatsResponse {
  databaseSizeBytes?: number;
  tables?: DatabaseTableStat[];
  raw?: Record<string, unknown>;
}

export interface ErrorLogEntry {
  id: string;
  timestamp?: string;
  level?: string;
  source?: string;
  message: string;
  context?: string;
}

export interface ErrorLogsResponse {
  page: number;
  pageSize: number;
  total?: number;
  hasMore?: boolean;
  entries: ErrorLogEntry[];
}

export interface RaidConfigSnapshot {
  totalManagedStreamers?: number;
  raidBotEnabledCount?: number;
  livePingEnabledCount?: number;
  allRaidBotEnabled?: boolean;
  allLivePingEnabled?: boolean;
  scope?: AdminConfigScope;
  raw?: Record<string, unknown>;
}

export interface ChatConfigSnapshot {
  totalManagedStreamers?: number;
  silentBanCount?: number;
  silentRaidCount?: number;
  allSilentBan?: boolean;
  allSilentRaid?: boolean;
  scope?: AdminConfigScope;
  raw?: Record<string, unknown>;
}

export interface ConfigOverview {
  promo?: Record<string, unknown>;
  raids?: RaidConfigSnapshot;
  chat?: ChatConfigSnapshot;
  announcements?: Record<string, unknown>;
  csrfToken?: string;
  raw?: Record<string, unknown>;
}

export interface RaidConfigUpdatePayload {
  raid_bot_enabled: boolean;
  live_ping_enabled: boolean;
  scope?: AdminConfigScope;
}

export interface ChatConfigUpdatePayload {
  silent_ban: boolean;
  silent_raid: boolean;
  scope?: AdminConfigScope;
}

export interface SubscriptionRecord {
  login?: string;
  customerReference?: string;
  planId?: string;
  status?: string;
  trialEndsAt?: string;
  currentPeriodEnd?: string;
  updatedAt?: string;
  priceLabel?: string;
  raw?: Record<string, unknown>;
}

export interface AffiliateListItem {
  login: string;
  displayName?: string;
  active: boolean;
  commissionRatePct: number;
  totalClaims: number;
  totalProvisionEuro: number;
  createdAt?: string | null;
  lastClaimAt?: string | null;
  updatedAt?: string | null;
  stripeConnectStatus?: string;
  status?: string;
  raw?: Record<string, unknown>;
}

export interface AffiliateStats {
  totalAffiliates?: number;
  activeAffiliates?: number;
  totalClaims: number;
  totalProvisionEuro: number;
  thisMonthClaims?: number;
  thisMonthProvisionEuro?: number;
  avgProvisionEuro?: number;
  activeCustomers?: number;
  raw?: Record<string, unknown>;
}

export interface AffiliateClaim {
  id?: number;
  customerLogin: string;
  claimedAt?: string | null;
  commissionCents: number;
  commissionCount: number;
  raw?: Record<string, unknown>;
}

export interface PiiReadiness {
  canGenerate: boolean;
  blockers: string[];
  warnings: string[];
  missingFields: string[];
  status?: string;
  ustStatus: string;
  raw?: Record<string, unknown>;
}

export interface GutschriftDocument {
  id?: number;
  affiliateLogin?: string;
  affiliateDisplayName?: string;
  periodYear?: number;
  periodMonth?: number;
  periodLabel?: string;
  gutschriftNumber?: string;
  status?: string;
  netAmountCents: number;
  vatAmountCents: number;
  grossAmountCents: number;
  commissionCount: number;
  generatedAt?: string | null;
  emailedAt?: string | null;
  createdAt?: string | null;
  noteText?: string;
  lastError?: string;
  downloadPath?: string | null;
  hasPdf?: boolean;
  affiliateUstStatus?: string;
  raw?: Record<string, unknown>;
}

export interface AffiliateDetail {
  login: string;
  displayName?: string;
  active: boolean;
  commissionRatePct: number;
  email?: string;
  fullName?: string;
  addressLine1?: string;
  addressCity?: string;
  addressZip?: string;
  addressCountry?: string;
  taxId?: string;
  vatId?: string;
  ustStatus?: string;
  stripeConnectStatus?: string;
  stripeAccountId?: string;
  createdAt?: string | null;
  updatedAt?: string | null;
  profileUpdatedAt?: string | null;
  stats: AffiliateStats;
  claims: AffiliateClaim[];
  readiness: PiiReadiness;
  gutschriften: GutschriftDocument[];
  raw?: Record<string, unknown>;
}

export interface AdminActionResult {
  ok: boolean;
  message: string;
  redirectUrl?: string;
}

export interface EngagementSettings {
  channelLogin: string;
  enabled: boolean;
  enabledAt?: string | null;
  enabledBy?: string | null;
  updatedAt?: string | null;
}

export interface MarketSharePoint {
  ts: string;
  partnerViewers: number;
  totalViewers: number;
  partnerStreams: number;
  totalStreams: number;
  sharePct: number;
}

export interface MarketSharePeak {
  ts: string;
  sharePct: number;
  partnerViewers: number;
  totalViewers: number;
}

export interface MarketShareTopStream {
  streamer: string;
  viewers: number;
  isPartner: boolean;
  isGerman: boolean;
  language: string | null;
}

export interface MarketShareCurrent {
  ts: string;
  totalViewers: number;
  partnerViewers: number;
  totalStreams: number;
  partnerStreams: number;
  sharePct: number;
  germanViewers: number;
  germanStreams: number;
  germanPartnerViewers: number;
  germanPartnerStreams: number;
  germanSharePct: number;
  topStreams: MarketShareTopStream[];
}

export type MarketShareScope = 'all' | 'german';

export interface MarketShareRoster {
  partnersTotal: number;
  partnersSeenInRange: number;
}

export interface MarketShareResponse {
  days: number;
  scope: MarketShareScope;
  bucketSeconds: number;
  series: MarketSharePoint[];
  peak: MarketSharePeak | null;
  current: MarketShareCurrent | null;
  roster: MarketShareRoster;
}

export interface ResearchDistribution {
  median: number;
  p25: number;
  p75: number;
}

export interface ResearchSubject {
  sessions_count: number;
  total_hours: number;
  active_days: number;
  avg_viewers: number;
  median_viewers: number;
  peak_viewers: number;
  sample_count: number;
  last_seen: string | null;
  dominant_language: string | null;
  de_share: number;
  recent_titles: string[];
}

export interface ResearchBaseline {
  partner_count: number;
  avg_viewers: ResearchDistribution;
  total_hours: ResearchDistribution;
  active_days: ResearchDistribution;
}

export interface ResearchScoreComponent {
  value: number;
  percentile: number;
  weight: number;
}

export interface ResearchResponse {
  login: string;
  days: number;
  found: boolean;
  is_already_partner: boolean;
  partner_status: string | null;
  subject: ResearchSubject;
  baseline: ResearchBaseline;
  score: {
    total: number;
    components: {
      viewers: ResearchScoreComponent;
      hours: ResearchScoreComponent;
      consistency: ResearchScoreComponent;
    };
    tier: {
      key: string;
      label: string;
    };
  };
}

export interface ResearchSuggestion {
  login: string;
  subject: ResearchSubject;
  score: ResearchResponse['score'];
}

export interface ResearchSuggestionsResponse {
  days: number;
  baseline: ResearchBaseline;
  items: ResearchSuggestion[];
}
