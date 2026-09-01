import type {
  CreateInternalHomeChangelogPayload,
  InternalHomeChangelogEntry,
  InternalHomeData,
} from '@/api/home';
import type { AuthStatus } from '@/api/auth';
import type { AdManagerResponse } from '@/api/adManager';
import type { SocialClipMitPosting } from '@/api/socialMedia';
import type { CatalogPlan } from '@/types/billing';
import {
  DEFAULT_LAYOUT,
  type ClipEnrichment,
  type ClipPreparation,
  type PostingPlan,
  type StreamerLayoutResponse,
  type VodArchiveSettings,
} from '../types/socialMedia';

const NOW_ISO = '2026-04-22T09:30:00Z';

const AUTH_STATUS_FIXTURE: AuthStatus = {
  authenticated: true,
  level: 'localhost',
  demoMode: true,
  isAdmin: true,
  isLocalhost: true,
  adminEligible: true,
  adminMode: true,
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
  if (endpoint === '/streamers') return [{ login: 'midcore_live', isPartner: true }];
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
  reconnect_wait_s: 90,
  reconnect_wait_max_s: 300,
  dock_url_vorhanden: true,
  dock_urls: {
    chat: 'https://deutsche-deadlock-community.de/dock/chat?t=dock_vorschau',
    activity: 'https://deutsche-deadlock-community.de/dock/activity?t=dock_vorschau',
    stream_info: 'https://deutsche-deadlock-community.de/dock/stream-info?t=dock_vorschau',
    points: 'https://deutsche-deadlock-community.de/dock/points?t=dock_vorschau',
  },
  verbindungen: [
    { platform: 'twitch', status: 'verbunden' },
    { platform: 'kick', status: 'getrennt' },
    { platform: 'youtube', status: 'getrennt' },
    { platform: 'tiktok', status: 'getrennt' },
  ],
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

const UPLINK_ADMIN_WAITLIST_FIXTURE = {
  entries: [
    {
      streamer_id: 987654321,
      requested_at: '2026-08-24T19:42:00Z',
      note: null,
      enabled: false,
    },
    {
      streamer_id: 123456789,
      requested_at: '2026-08-25T08:15:00Z',
      note: 'Testlauf für den nächsten Community-Stream',
      enabled: false,
    },
  ],
};

const AD_MANAGER_FIXTURE: AdManagerResponse = {
  settings: {
    enabled: true,
    strategy: 'smart',
    adDurationSeconds: 90,
    minIntervalMinutes: 45,
    startupDelayMinutes: 20,
    quietWindowMinutes: 5,
    actionLeadSeconds: 60,
    updatedAt: '2026-04-22T09:24:00Z',
  },
  status: {
    isLive: true,
    nextAdAt: '2026-04-22T09:42:00Z',
    lastAdAt: '2026-04-22T08:55:00Z',
    durationSeconds: 90,
    prerollFreeSeconds: 1_380,
    snoozeCount: 2,
    snoozeRefreshAt: '2026-04-22T10:30:00Z',
    observedAt: NOW_ISO,
    workerHealthy: true,
    workerHeartbeatAt: NOW_ISO,
    lastAction: {
      kind: 'snooze',
      outcome: 'succeeded',
      detail: 'Nächste Werbung um fünf Minuten verschoben.',
      at: '2026-04-22T09:27:00Z',
    },
    scopes: {
      read: true,
      snooze: true,
      commercial: true,
    },
  },
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
  if (pathname === '/twitch/api/v2/streamer/ad-manager') return AD_MANAGER_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/me') return UPLINK_ME_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/destinations') return UPLINK_DESTINATIONS_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/caps') return UPLINK_CAPS_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/admin/waitlist') return UPLINK_ADMIN_WAITLIST_FIXTURE;
  return undefined;
}

const SOCIAL_MEDIA_PREVIEW_THUMBNAIL =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 640 360'%3E%3Crect width='640' height='360' fill='%231b1720'/%3E%3Ccircle cx='520' cy='70' r='150' fill='%23c5a059' fill-opacity='.22'/%3E%3Cpath d='M70 270 210 115l110 100 90-70 160 125Z' fill='%23c5a059' fill-opacity='.7'/%3E%3C/svg%3E";

const SOCIAL_MEDIA_CLIPS_FIXTURE: SocialClipMitPosting[] = [
  {
    clip_db_id: 301,
    clip_id: 'preview-teamfight',
    clip_url: null,
    title: 'Teamfight gedreht – Vorschau bereit',
    thumbnail_url: SOCIAL_MEDIA_PREVIEW_THUMBNAIL,
    streamer_login: 'midcore_live',
    created_at: '2026-04-22T08:50:00Z',
    duration_seconds: 31,
    view_count: 428,
    game_name: 'Deadlock',
    status: 'pending',
    source_kind: 'twitch',
    upload_local_path: null,
    retention_until: '2026-05-06T08:50:00Z',
    discarded_at: null,
    platform_status: { youtube: false, tiktok: false, instagram: false },
    layout_override: null,
    effective_layout: DEFAULT_LAYOUT,
    enrichment_status: 'done',
    enrichment_summary: { top_hashtags: ['deadlock', 'teamfight'], provider: 'preview' },
    approval: null,
  },
  {
    clip_db_id: 302,
    clip_id: 'preview-render-fehler',
    clip_url: null,
    title: 'Manueller Upload – Fehlerzustand testen',
    thumbnail_url: SOCIAL_MEDIA_PREVIEW_THUMBNAIL,
    streamer_login: 'midcore_live',
    created_at: '2026-04-22T08:20:00Z',
    duration_seconds: 44,
    view_count: 0,
    game_name: 'Deadlock',
    status: 'pending',
    source_kind: 'manual_upload',
    upload_local_path: null,
    retention_until: '2026-05-06T08:20:00Z',
    discarded_at: null,
    platform_status: { youtube: false, tiktok: false, instagram: false },
    layout_override: null,
    effective_layout: DEFAULT_LAYOUT,
    enrichment_status: 'skipped_no_key',
    enrichment_summary: null,
    approval: null,
    upload_states: {
      instagram: {
        reconciliation_id: '9302',
        status: 'reconciliation_required',
        error_code: 'provider_result_unknown',
        provider_started_at: '2026-04-22T09:07:00Z',
        provider_external_id: 'preview-container-302',
        provider_accepted_at: '2026-04-22T09:07:04Z',
        reconciliation_required: true,
      },
    },
  },
];

const SOCIAL_MEDIA_PLAN_FIXTURE: PostingPlan = {
  streamer_login: 'midcore_live',
  release_enabled: false,
  approval_mode: 'manual',
  approval_modes: ['manual', 'veto_window', 'full_auto'],
  timezone: 'Europe/Berlin',
  platforms: [
    {
      platform: 'youtube',
      auto_post: false,
      provider_release_blocked: false,
      release_block_reason: null,
      posts_per_week: 4,
      max_posts_per_day: 1,
      post_times: ['18:00'],
      next_slot: null,
    },
    {
      platform: 'tiktok',
      auto_post: false,
      provider_release_blocked: true,
      release_block_reason: 'tiktok_consent_required',
      posts_per_week: 4,
      max_posts_per_day: 1,
      post_times: ['19:00'],
      next_slot: null,
    },
    {
      platform: 'instagram',
      auto_post: false,
      provider_release_blocked: false,
      release_block_reason: null,
      posts_per_week: 3,
      max_posts_per_day: 1,
      post_times: ['20:00'],
      next_slot: null,
    },
  ],
  categories: [
    {
      category_key: 'deadlock',
      display_name: 'Deadlock',
      enrichment_enabled: true,
      auto_post: false,
    },
  ],
  pool: {
    verfuegbare_clips: SOCIAL_MEDIA_CLIPS_FIXTURE.length,
    aktive_plattformen: 0,
    reicht_fuer_posts: 0,
    posts_pro_woche: 0,
    reicht_fuer_tage: null,
    warnung: false,
  },
};

const SOCIAL_MEDIA_LAYOUT_FIXTURE: StreamerLayoutResponse = {
  streamer_login: 'midcore_live',
  layout: DEFAULT_LAYOUT,
  cam_enabled: DEFAULT_LAYOUT.cam_enabled,
  mode: DEFAULT_LAYOUT.mode,
  is_default: false,
  updated_at: NOW_ISO,
  updated_by: 'local-preview',
};

const SOCIAL_MEDIA_VOD_FIXTURE: VodArchiveSettings = {
  streamer_login: 'midcore_live',
  enabled: false,
  privacy: 'private',
  privacy_options: ['private', 'unlisted', 'public'],
  privacy_forced: true,
};

const SOCIAL_MEDIA_ENRICHMENT_FIXTURE: ClipEnrichment = {
  clip_db_id: 301,
  transcript_raw: 'Wir drehen den Fight noch, geh auf den Patron!',
  transcript_corrected: 'Wir drehen den Fight noch – geh auf den Patron!',
  transcript_segments: null,
  transcript_lang: 'de',
  detected_terms: ['Patron'],
  title_youtube: 'Dieser Deadlock-Teamfight war schon verloren',
  title_tiktok: 'Wie wir diesen Fight noch drehen',
  title_instagram: 'Deadlock Comeback im letzten Moment',
  description_youtube: 'Ein Teamfight, der erst im letzten Moment kippt.',
  description_tiktok: 'Nicht aufgeben – dieser Fight dreht sich komplett.',
  description_instagram: 'Das knappste Comeback des Abends.',
  hashtags_youtube: ['deadlock', 'gaming'],
  hashtags_tiktok: ['deadlock', 'gaming', 'teamfight'],
  hashtags_instagram: ['deadlock', 'gaming', 'reels'],
  llm_provider: 'preview',
  llm_model: null,
  cost_usd_estimate: null,
  status: 'done',
  error_message: null,
  started_at: '2026-04-22T09:00:00Z',
  completed_at: '2026-04-22T09:00:05Z',
  edited_by: null,
  updated_at: '2026-04-22T09:00:05Z',
};

const SOCIAL_MEDIA_PREVIEW_VIDEO =
  'data:video/mp4;base64,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';
const preparationStarted = new Map<number, number>();

function previewPreparation(clipDbId: number): ClipPreparation {
  const gestartet = preparationStarted.get(clipDbId);
  if (gestartet !== undefined) {
    const fertig = Date.now() - gestartet > 1800;
    return {
      clip_db_id: clipDbId,
      state: fertig ? 'preview_ready' : 'rendering',
      source_ready: true,
      preview_ready: fertig,
      preview_url: fertig ? SOCIAL_MEDIA_PREVIEW_VIDEO : null,
      download_url: fertig ? SOCIAL_MEDIA_PREVIEW_VIDEO : null,
      error_code: null,
      error_message: null,
      requested_at: new Date(gestartet).toISOString(),
      started_at: new Date(gestartet).toISOString(),
      completed_at: fertig ? new Date(gestartet + 1800).toISOString() : null,
      updated_at: new Date(fertig ? gestartet + 1800 : Date.now()).toISOString(),
    };
  }

  if (clipDbId === 302) {
    return {
      clip_db_id: clipDbId,
      state: 'failed',
      source_ready: true,
      preview_ready: false,
      preview_url: null,
      download_url: null,
      error_code: 'render_failed',
      error_message: 'Preview-Fehlerzustand',
      requested_at: '2026-04-22T09:05:00Z',
      started_at: '2026-04-22T09:05:01Z',
      completed_at: '2026-04-22T09:05:03Z',
      updated_at: '2026-04-22T09:05:03Z',
    };
  }

  return {
    clip_db_id: clipDbId,
    state: 'preview_ready',
    source_ready: true,
    preview_ready: true,
    preview_url: SOCIAL_MEDIA_PREVIEW_VIDEO,
    download_url: SOCIAL_MEDIA_PREVIEW_VIDEO,
    error_code: null,
    error_message: null,
    requested_at: '2026-04-22T09:00:00Z',
    started_at: '2026-04-22T09:00:01Z',
    completed_at: '2026-04-22T09:00:05Z',
    updated_at: '2026-04-22T09:00:05Z',
  };
}

/**
 * Vollständiger lokaler Social-Media-Lesestand plus eine simulierte
 * Preparation-Mutation. Sie berührt weder Plattformkonten noch Produktivdaten.
 */
export function getSocialMediaPreviewFixture(
  pathWithQuery: string,
  method = 'GET',
): unknown | undefined {
  const url = new URL(pathWithQuery, 'http://preview.local');
  const pathname = url.pathname;
  const requestMethod = method.toUpperCase();

  if (requestMethod === 'GET' && pathname === '/social-media/api/access/me') {
    return { allowed: true, streamer: 'midcore_live', isAdmin: true };
  }
  if (requestMethod === 'GET' && pathname === '/social-media/api/access') {
    return { items: [{ streamer_login: 'midcore_live', granted: true }] };
  }
  if (requestMethod === 'POST' && pathname === '/social-media/api/mark-uploaded') {
    return {
      ok: false,
      error: 'preview_read_only',
      message: 'preview_read_only',
    };
  }
  if (requestMethod === 'GET' && pathname === '/social-media/api/admin/streamer-layout') {
    return SOCIAL_MEDIA_LAYOUT_FIXTURE;
  }
  if (requestMethod === 'GET' && pathname === '/social-media/api/admin/clips') {
    const status = url.searchParams.get('status');
    const items = status
      ? SOCIAL_MEDIA_CLIPS_FIXTURE.filter((clip) => clip.status === status)
      : SOCIAL_MEDIA_CLIPS_FIXTURE;
    return { items, total: items.length, page: 1, page_size: 24 };
  }
  if (
    requestMethod === 'GET' &&
    pathname === '/social-media/api/admin/settings/posting-plan'
  ) {
    return SOCIAL_MEDIA_PLAN_FIXTURE;
  }
  if (
    requestMethod === 'GET' &&
    pathname === '/social-media/api/admin/settings/vod-archive'
  ) {
    return SOCIAL_MEDIA_VOD_FIXTURE;
  }
  if (requestMethod === 'GET' && pathname === '/social-media/api/platforms/status') {
    return {
      platforms: ['youtube', 'tiktok', 'instagram'].map((platform) => ({
        platform,
        connected: false,
        provider_calls_enabled: false,
        provider_release_blocked: true,
        release_block_reason:
          platform === 'tiktok' ? 'tiktok_consent_required' : 'platform_release_blocked',
        username: null,
        expired: false,
        expires_at: null,
        uses_global_fallback: false,
      })),
    };
  }

  const enrichmentMatch = pathname.match(
    /^\/social-media\/api\/admin\/clips\/(\d+)\/enrichment$/,
  );
  if (requestMethod === 'GET' && enrichmentMatch) {
    return { ...SOCIAL_MEDIA_ENRICHMENT_FIXTURE, clip_db_id: Number(enrichmentMatch[1]) };
  }

  const preparationMatch = pathname.match(
    /^\/social-media\/api\/admin\/clips\/(\d+)\/preparation$/,
  );
  if (preparationMatch) {
    const clipDbId = Number(preparationMatch[1]);
    if (requestMethod === 'POST') preparationStarted.set(clipDbId, Date.now());
    if (requestMethod === 'GET' || requestMethod === 'POST') return previewPreparation(clipDbId);
  }

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
