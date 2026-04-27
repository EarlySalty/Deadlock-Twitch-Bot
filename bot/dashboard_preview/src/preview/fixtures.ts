import type { CreateInternalHomeChangelogPayload, InternalHomeChangelogEntry, InternalHomeData } from '@/api/home';
import type { AffiliatePortalData } from '@/api/affiliate';
import type { Affiliate, AffiliateDetail, AffiliateStats } from '@/api/admin';
import type { AuthStatus } from '@/api/auth';
import type { CatalogPlan } from '@/types/billing';

const NOW_ISO = '2026-04-22T09:30:00Z';

const AUTH_STATUS_FIXTURE: AuthStatus = {
  authenticated: true,
  level: 'localhost',
  demoMode: true,
  isAdmin: true,
  isLocalhost: true,
  canViewAllStreamers: true,
  twitchLogin: 'midcore_live',
  displayName: 'Local Preview Creator',
  csrfToken: 'preview-csrf-token',
  csrf_token: 'preview-csrf-token',
  permissions: {
    viewAllStreamers: true,
    viewComparison: true,
    viewChatAnalytics: true,
    viewOverlap: true,
  },
  plan: {
    planId: 'analysis_dashboard',
    planName: 'Extended Preview',
    tier: 'extended',
    isExtended: true,
    expiresAt: null,
    source: 'local_preview',
    entitlements: [
      'analytics.basic',
      'analytics.ai_mini',
      'analytics.ai_full',
      'analytics.extended',
      'chat.lurker_tax',
      'chat.promos.disable',
      'raid.priority',
    ],
  },
};

const BILLING_CATALOG_FIXTURE: { plans: CatalogPlan[] } = {
  plans: [
    {
      id: 'raid_free',
      name: 'Free',
      tier: 'free',
      price_monthly: 0,
      features: ['Basis-Analytics', 'Viewer-Trend', 'Kanalüberblick'],
      is_current: false,
    },
    {
      id: 'raid_boost',
      name: 'Basic',
      tier: 'basic',
      price_monthly: 9.99,
      features: ['Growth-Tracking', 'Chat-Analytics', 'Vergleichsansichten'],
      is_current: false,
    },
    {
      id: 'analysis_dashboard',
      name: 'Extended',
      tier: 'extended',
      price_monthly: 24.99,
      features: ['KI-Analysen', 'Viewer-Profile', 'Monetization', 'Coaching'],
      is_current: true,
    },
    {
      id: 'bundle_analysis_raid_boost',
      name: 'Bundle',
      tier: 'extended',
      price_monthly: 29.99,
      features: ['Alle Analytics-Features', 'Raid-Priorität', 'Extended Support'],
      is_current: false,
    },
  ],
};

const INTERNAL_HOME_FIXTURE: InternalHomeData = {
  greeting: 'Willkommen in der lokalen Preview',
  twitchLogin: 'midcore_live',
  displayName: 'Local Preview Creator',
  loginUrl: '/dashboard',
  oauth: {
    connected: true,
    status: 'connected',
    needsReauth: false,
    grantedScopes: ['channel:manage:raids', 'moderator:read:followers', 'channel:read:subscriptions'],
    missingScopes: [],
    reconnectUrl: '/dashboard',
    profileUrl: '/dashboard',
    lastCheckedAt: NOW_ISO,
  },
  discord: {
    connected: true,
    status: 'connected',
    connectUrl: '/verwaltung',
    lastCheckedAt: NOW_ISO,
  },
  raid: {
    active: true,
    statusText: 'Preview aktiv',
    note: 'Lokale Design-Sandbox mit statischen Fixture-Daten.',
    lastEventAt: NOW_ISO,
  },
  kpis30d: {
    streams: 18,
    avgViewers: 126,
    followerDelta: 214,
    banKpi: 7,
  },
  recentStreams: [
    {
      id: 7001,
      startedAt: '2026-04-21T17:00:00Z',
      endedAt: '2026-04-21T20:10:00Z',
      durationMinutes: 190,
      avgViewers: 142,
      peakViewers: 221,
      followerDelta: 34,
      title: 'Preview Build Review | Theme Iteration',
      category: 'Deadlock',
    },
    {
      id: 7002,
      startedAt: '2026-04-19T18:15:00Z',
      endedAt: '2026-04-19T21:00:00Z',
      durationMinutes: 165,
      avgViewers: 118,
      peakViewers: 184,
      followerDelta: 21,
      title: 'Deadlock Ranked + Community Review',
      category: 'Deadlock',
    },
  ],
  actionLog: [
    {
      id: 'preview-1',
      timestamp: NOW_ISO,
      eventType: 'ops.note',
      statusLabel: 'Preview',
      targetLogin: 'midcore_live',
      summary: 'Lokale Preview nutzt Demo-Daten und isolierte Billing-/Home-Fixtures.',
      severity: 'info',
      source: 'local_preview',
    },
  ],
  impactFeed: [
    {
      id: 'impact-1',
      timestamp: NOW_ISO,
      eventType: 'growth',
      title: 'Stabile Vorschau-Daten',
      summary: 'Das Preview-Dashboard simuliert Wachstum, Monetization und Community-Signale.',
      severity: 'success',
      source: 'local_preview',
    },
  ],
  changelog: {
    canWrite: true,
    maxEntries: 10,
    entries: [
      {
        id: 'preview-log-1',
        entryDate: '2026-04-22',
        title: 'Local Preview aktiviert',
        content: 'Isolierte localhost-Sandbox für Theme-Iterationen ohne Produktivänderungen.',
        createdAt: NOW_ISO,
      },
    ],
  },
  generatedAt: NOW_ISO,
};

const ROADMAP_FIXTURE = {
  planned: [
    {
      id: 1,
      title: 'Warm-Dark Theme',
      description: 'Graphit statt Navy, ruhigere Flächenhierarchie.',
      status: 'planned',
      priority: 1,
      created_at: NOW_ISO,
      updated_at: NOW_ISO,
    },
  ],
  in_progress: [
    {
      id: 2,
      title: 'Local Preview Sandbox',
      description: 'Komplett getrennte localhost-Kopie des Dashboards.',
      status: 'in_progress',
      priority: 1,
      created_at: NOW_ISO,
      updated_at: NOW_ISO,
    },
  ],
  done: [
    {
      id: 3,
      title: 'Analyse Routing fix',
      description: 'Legacy /twitch/analyse leitet sauber auf /analyse um.',
      status: 'done',
      priority: 1,
      created_at: NOW_ISO,
      updated_at: NOW_ISO,
    },
  ],
};

const ADS_SCHEDULE_FIXTURE = {
  nextBreakAt: '2026-04-22T10:15:00Z',
  snoozeAvailable: true,
  minutesBetweenBreaks: 42,
  lastBreakAt: '2026-04-22T09:22:00Z',
  automaticMidRolls: true,
};

const CHAT_HYPE_TIMELINE_FIXTURE = {
  summary: {
    totalMessages: 1842,
    uniqueChatters: 318,
    peakMinute: 42,
    peakMessages: 67,
  },
  points: Array.from({ length: 12 }, (_, index) => ({
    minute: index * 10,
    messages: 18 + index * 4,
    chatters: 12 + index * 3,
    viewers: 95 + index * 6,
  })),
};

const CHAT_CONTENT_ANALYSIS_FIXTURE = {
  summary: {
    totalMessages: 1842,
    actionableMessages: 624,
    questions: 121,
    emotes: 412,
    commands: 93,
  },
  topTerms: [
    { term: 'build', count: 67 },
    { term: 'ranked', count: 58 },
    { term: 'lash', count: 44 },
  ],
  categories: [
    { label: 'Gameplay', value: 46 },
    { label: 'Community', value: 31 },
    { label: 'Meta', value: 23 },
  ],
};

const CHAT_SOCIAL_GRAPH_FIXTURE = {
  nodes: [
    { id: 'midcore_live', label: 'midcore_live', group: 'streamer', weight: 12 },
    { id: 'viewer_alpha', label: 'viewer_alpha', group: 'viewer', weight: 6 },
    { id: 'viewer_beta', label: 'viewer_beta', group: 'viewer', weight: 5 },
  ],
  edges: [
    { source: 'midcore_live', target: 'viewer_alpha', weight: 8 },
    { source: 'midcore_live', target: 'viewer_beta', weight: 6 },
    { source: 'viewer_alpha', target: 'viewer_beta', weight: 3 },
  ],
};

const STREAM_REPORT_FIXTURE = {
  summary: {
    headline: 'Solider Mid-Core-Preview-Stream mit gutem Einstieg und stabiler Chat-Aktivität.',
    keyTakeaways: [
      'Der Startblock erzeugt früh Aufmerksamkeit.',
      'Der Chat bleibt über die Mitte hinweg konstant.',
      'Titel- und Timing-Tests sind gut vergleichbar.',
    ],
  },
  recommendations: [
    {
      id: 'rep-1',
      title: 'Opener klarer zuspitzen',
      description: 'Die ersten 10 Minuten eignen sich gut für einen stärkeren Hook.',
      priority: 'high',
    },
  ],
};

const SESSION_DETAIL_FIXTURE = {
  id: 7001,
  started_at: '2026-04-21T17:00:00Z',
  ended_at: '2026-04-21T20:10:00Z',
  avg_viewers: 142,
  peak_viewers: 221,
  follower_delta: 34,
  title: 'Preview Build Review | Theme Iteration',
  category_name: 'Deadlock',
  timeline: Array.from({ length: 10 }, (_, index) => ({
    minute: index * 20,
    viewers: 108 + index * 9,
  })),
  chatters: [
    { login: 'viewer_alpha', messages: 42 },
    { login: 'viewer_beta', messages: 29 },
    { login: 'viewer_gamma', messages: 17 },
  ],
};

const SESSION_EVENTS_FIXTURE = {
  follows: [
    { minute: 12, count: 4 },
    { minute: 55, count: 7 },
  ],
  raids: [
    { minute: 61, viewer_count: 28, source_login: 'raid_partner' },
  ],
  subscriptions: [
    { minute: 77, count: 3 },
  ],
};

const AFFILIATE_PORTAL_FIXTURE: AffiliatePortalData = {
  affiliate: {
    login: 'local-preview-affiliate',
    display_name: 'Local Preview Affiliate',
    active: true,
    referral_code: 'PREVIEW42',
    referral_url: 'http://localhost:4174/pricing?ref=PREVIEW42',
  },
  stats: {
    total_claims: 18,
    total_provision: 274,
    this_month_claims: 6,
    this_month_provision: 82,
    pending_payout: 55,
  },
  recent_claims: [
    { customer_display_name: 'Creator One', plan_name: 'Extended', amount: 29.99, created_at: NOW_ISO },
  ],
};

const ADMIN_AFFILIATES_FIXTURE: { affiliates: Affiliate[] } = {
  affiliates: [
    {
      login: 'local-preview-affiliate',
      display_name: 'Local Preview Affiliate',
      active: true,
      total_claims: 18,
      total_provision: 274,
      created_at: '2026-01-10T12:00:00Z',
      last_claim_at: NOW_ISO,
    },
  ],
};

const ADMIN_AFFILIATE_STATS_FIXTURE: AffiliateStats = {
  total_affiliates: 1,
  active_affiliates: 1,
  total_claims: 18,
  total_provision: 274,
  this_month_claims: 6,
  this_month_provision: 82,
};

const ADMIN_AFFILIATE_DETAIL_FIXTURE: AffiliateDetail = {
  affiliate: {
    login: 'local-preview-affiliate',
    display_name: 'Local Preview Affiliate',
    active: true,
    created_at: '2026-01-10T12:00:00Z',
  },
  claims: [
    {
      id: 1,
      customer_login: 'creator_one',
      claimed_at: NOW_ISO,
      commission_cents: 599,
      commission_count: 1,
    },
  ],
  stats: {
    total_claims: 18,
    total_provision: 274,
    avg_provision: 15.2,
    active_customers: 9,
  },
};

export function getPreviewApiFixture(
  endpoint: string,
  _params: Record<string, string | number | boolean> = {},
): unknown | undefined {
  if (endpoint === '/auth-status') return AUTH_STATUS_FIXTURE;
  if (endpoint === '/billing/catalog') return BILLING_CATALOG_FIXTURE;
  if (endpoint === '/internal-home') return INTERNAL_HOME_FIXTURE;
  if (endpoint === '/roadmap') return ROADMAP_FIXTURE;
  if (endpoint === '/ads-schedule') return ADS_SCHEDULE_FIXTURE;
  if (endpoint === '/chat-hype-timeline') return CHAT_HYPE_TIMELINE_FIXTURE;
  if (endpoint === '/chat-content-analysis') return CHAT_CONTENT_ANALYSIS_FIXTURE;
  if (endpoint === '/chat-social-graph') return CHAT_SOCIAL_GRAPH_FIXTURE;
  if (endpoint === '/stream-report') return STREAM_REPORT_FIXTURE;
  if (endpoint.startsWith('/session/') && endpoint.endsWith('/events')) return SESSION_EVENTS_FIXTURE;
  if (endpoint.startsWith('/session/')) return SESSION_DETAIL_FIXTURE;
  if (endpoint === '/affiliate/portal') return AFFILIATE_PORTAL_FIXTURE;
  return undefined;
}

export function getPreviewAdminFixture(pathname: string): unknown | undefined {
  if (pathname === '/twitch/api/admin/affiliates') return ADMIN_AFFILIATES_FIXTURE;
  if (pathname === '/twitch/api/admin/affiliates/stats') return ADMIN_AFFILIATE_STATS_FIXTURE;
  if (pathname.startsWith('/twitch/api/admin/affiliates/') && pathname.endsWith('/toggle')) {
    return { login: 'local-preview-affiliate', active: false };
  }
  if (pathname.startsWith('/twitch/api/admin/affiliates/')) return ADMIN_AFFILIATE_DETAIL_FIXTURE;
  return undefined;
}

export function getPreviewTitleSuggestion(): unknown {
  return {
    primary: 'Deadlock Ranked Push | Local Preview Theme Review',
    alternatives: [
      'Theme Iteration + Ranked Grind | Preview Build',
      'Local Preview: Deadlock Analytics Deep Dive',
      'Design Review + Deadlock Ranked Session',
    ],
    title_analysis: [
      {
        title: 'Deadlock Ranked Grind | Preview Build',
        avg_viewers: 142,
        peak_viewers: 221,
        relative_perf: 1.12,
        engagement_rate: 0.68,
      },
    ],
  };
}

export function getPreviewTitleInsights(): unknown {
  return {
    insight: {
      strengths: 'Klare Spiel- und Kontextsignale machen die Vorschau-Titel gut lesbar.',
      weaknesses: 'Zu generische “Preview”-Wortwahl reduziert den eigentlichen Hook.',
      patterns: 'Titel mit Deadlock + konkretem Ziel performen stabiler als generische Status-Titel.',
      recommendations: 'Kontext “Theme Review” nur ergänzend nutzen, nicht als Kern des Titels.',
      generated_at: NOW_ISO,
    },
  };
}

export function createPreviewChangelogEntry(
  payload: CreateInternalHomeChangelogPayload,
): InternalHomeChangelogEntry {
  return {
    id: `preview-log-${Date.now()}`,
    entryDate: payload.entryDate || '2026-04-22',
    title: payload.title || 'Preview-Eintrag',
    content: payload.content,
    createdAt: NOW_ISO,
  };
}
