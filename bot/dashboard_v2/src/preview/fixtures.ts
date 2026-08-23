import type {
  CreateInternalHomeChangelogPayload,
  InternalHomeChangelogEntry,
  InternalHomeData,
} from '@/api/home';
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
    planId: 'pro',
    planName: 'Creator Pro (Preview)',
    tier: 'extended',
    isExtended: true,
    expiresAt: null,
    source: 'local_preview',
    entitlements: [
      'analytics',
      'chat.lurker_tax',
      'chat.promos.disable',
      'raid.priority',
      'social.auto_post',
    ],
  },
};

const BILLING_CATALOG_FIXTURE: { plans: CatalogPlan[] } = {
  plans: [
    {
      id: 'free',
      name: 'Netzwerk Free',
      tier: 'free',
      price_monthly: 0,
      description:
        'Dauerhaft kostenlos: Auto-Raid, Chat-Schutz und die Tagesform deines letzten Streams.',
      monthly_gross_cents: 0,
      yearly_gross_cents: 0,
      features: [
        'Auto-Raid in beide Richtungen',
        'Kompletter Chat-Schutz und alle Chat-Befehle',
        'Go-Live-Post im Community-Discord',
        'Overlay-Builder und Sendeplanung',
        'Tagesform deines letzten Streams',
      ],
      buchbar: true,
      is_current: false,
    },
    {
      id: 'plus',
      name: 'Netzwerk Plus',
      tier: 'extended',
      price_monthly: 4.99,
      description: 'Dein voller Verlauf, Zeitraumvergleiche und die komplette KI-Auswertung.',
      monthly_gross_cents: 499,
      yearly_gross_cents: 4990,
      entitlements: ['analytics', 'chat.lurker_tax', 'chat.promos.disable', 'raid.priority'],
      features: [
        'Voller Verlauf statt nur letztem Stream',
        'Zeitraumvergleiche und Wachstumskurven',
        'KI-Analyse, KI-Chat, Coaching und KI-Wochenreport',
        'Werbefreier Chat, Raid-Vorrang und Lurker-Erinnerung',
      ],
      buchbar: true,
      is_current: true,
    },
    {
      id: 'pro',
      name: 'Creator Pro',
      tier: 'extended',
      price_monthly: 9.99,
      description:
        'Alles aus Netzwerk Plus, dazu Vorrang bei Support und neuen Funktionen.',
      monthly_gross_cents: 999,
      yearly_gross_cents: 9990,
      entitlements: [
        'analytics',
        'chat.lurker_tax',
        'chat.promos.disable',
        'raid.priority',
        'social.auto_post',
      ],
      features: [
        'Alles aus Netzwerk Plus',
        'Vorrang bei Support und neuen Funktionen',
      ],
      buchbar: false,
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
    grantedScopes: [
      'channel:manage:raids',
      'moderator:read:followers',
      'channel:read:subscriptions',
    ],
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
    headline:
      'Solider Mid-Core-Preview-Stream mit gutem Einstieg und stabiler Chat-Aktivität.',
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
  raids: [{ minute: 61, viewer_count: 28, source_login: 'raid_partner' }],
  subscriptions: [{ minute: 77, count: 3 }],
};

export function getPreviewApiFixture(
  endpoint: string,
  _params: Record<string, string | number | boolean> = {}
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
  return undefined;
}

/**
 * Uplink im Preview.
 *
 * Die Uplink-Aufrufe gehen ueber `fetchJson` mit absolutem Pfad, nicht ueber
 * `fetchApi`, und liefen im Preview deshalb ins Leere: die Seite zeigte nur
 * "Uplink ist gerade nicht erreichbar". Ohne diese Fixtures laesst sich die
 * Zielverwaltung lokal gar nicht ansehen, und genau daran haengt jede
 * optische Pruefung.
 *
 * Der Zustand ist absichtlich gemischt: Twitch mit einem manuellen Wert ueber
 * der Empfehlung der Plattform, YouTube mit einem manuellen Wert darunter, der
 * Rest leer. So sieht man in einem Blick alle Zustaende einer Zielkarte, auch
 * den Empfehlungshinweis am Feld.
 */
const UPLINK_ME_FIXTURE = {
  enabled: true,
  waitlisted: false,
  ingest_key: 'rsr_preview',
  rtmp_url: '',
  srt_hint:
    'srt://deutsche-deadlock-community.de:8899?mode=caller&latency=2000&streamid=rsr_preview_key',
  live_status: 'aus',
  twitch_login: 'earlysalty',
};

// `effective` ist ueberall identisch mit `requested`: das Relay rechnet nichts
// mehr herunter, das Feld steht nur noch fuer aeltere Clients im JSON.
const UPLINK_DESTINATIONS_FIXTURE = {
  destinations: [
    {
      platform: 'twitch',
      rtmp_url: 'rtmp://live.twitch.tv/app',
      enabled: true,
      requested: { width: 2560, height: 1440, fps: 60, bitrate_kbps: 16000 },
      effective: { width: 2560, height: 1440, fps: 60, bitrate_kbps: 16000 },
    },
    {
      platform: 'youtube',
      rtmp_url: 'rtmp://a.rtmp.youtube.com/live2',
      enabled: true,
      requested: { width: 2560, height: 1440, fps: 60, bitrate_kbps: 18000 },
      effective: { width: 2560, height: 1440, fps: 60, bitrate_kbps: 18000 },
    },
  ],
};

// Empfehlungen, keine Grenzen. Der `ingest`-Eintrag ist weg, es gibt keinen
// Deckel mehr, gegen den die Oberflaeche pruefen koennte.
const UPLINK_CAPS_FIXTURE = {
  platforms: [
    { platform: 'twitch', recommended_width: 2560, recommended_height: 1440, recommended_fps: 60, recommended_bitrate_kbps: 12000, force_cbr: true },
    { platform: 'kick', recommended_width: 1920, recommended_height: 1080, recommended_fps: 60, recommended_bitrate_kbps: 8000, force_cbr: true },
    { platform: 'youtube', recommended_width: 2560, recommended_height: 1440, recommended_fps: 60, recommended_bitrate_kbps: 24000, force_cbr: false },
    { platform: 'tiktok', recommended_width: 1920, recommended_height: 1080, recommended_fps: 60, recommended_bitrate_kbps: 8000, force_cbr: false },
  ],
};

/**
 * Fixture zu einem absoluten Pfad. `undefined` heisst: kein Fixture, der
 * Aufruf geht wie sonst ins Netz.
 *
 * Schreibende Aufrufe bekommen absichtlich die unveraenderte Zielliste
 * zurueck. Ein Preview ohne Server kann nichts speichern, und eine erfundene
 * Bestaetigung waere schlimmer als keine: sie zeigte einen Zustand, den es
 * nirgends gibt.
 */
export function getPreviewPathFixture(pathname: string): unknown | undefined {
  if (pathname === '/twitch/api/v2/uplink/me') return UPLINK_ME_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/destinations') return UPLINK_DESTINATIONS_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/caps') return UPLINK_CAPS_FIXTURE;
  return undefined;
}

export function getPreviewAdminFixture(pathname: string): unknown | undefined {
  void pathname;
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
      patterns:
        'Titel mit Deadlock + konkretem Ziel performen stabiler als generische Status-Titel.',
      recommendations: 'Kontext “Theme Review” nur ergänzend nutzen, nicht als Kern des Titels.',
      generated_at: NOW_ISO,
    },
  };
}

export function createPreviewChangelogEntry(
  payload: CreateInternalHomeChangelogPayload
): InternalHomeChangelogEntry {
  return {
    id: `preview-log-${Date.now()}`,
    entryDate: payload.entryDate || '2026-04-22',
    title: payload.title || 'Preview-Eintrag',
    content: payload.content,
    createdAt: NOW_ISO,
  };
}
