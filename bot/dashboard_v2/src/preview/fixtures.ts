import type {
  CreateInternalHomeChangelogPayload,
  InternalHomeChangelogEntry,
  InternalHomeData,
} from '@/api/home';
import type { AuthStatus } from '@/api/auth';
import type { AdManagerResponse } from '@/api/adManager';
import type { CatalogPlan } from '@/types/billing';
import { DEFAULT_LAYOUT, type ClipApprovalRecord, type PostingPlan } from '../types/socialMedia';
import type { SocialClipMitPosting, PlatformStatus } from '@/api/socialMedia';
import { aiPreviewConsent, aiPreviewJob, type AiPreviewScenario } from '../components/socialmedia/ai/preview';
import { onPublicTour } from '../publicTour';

const SOCIAL_WORKSPACE_PREVIEW = import.meta.env?.MODE === 'preview' || import.meta.env?.MODE === undefined;
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
  twitchUserId: '123456789',
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
      id: 'studio',
      name: 'Studio',
      tier: 'extended',
      price_monthly: 12,
      description: 'Pipeline, Import, VOD, drei Plattformen, Vorlagen und Freigabe-Autopilot.',
      monthly_gross_cents: 1200,
      yearly_gross_cents: 12000,
      recommended: false,
      buchbar: false,
      is_current: false,
      features: ['Pipeline', 'Import', 'VOD', 'Drei Plattformen', 'Vorlagen', 'Freigabe-Autopilot'],
    },
    {
      id: 'studio_ki',
      name: 'Studio KI',
      tier: 'extended',
      price_monthly: 15,
      description: 'Transkription, Titel, Hashtags und Vollautomatik.',
      monthly_gross_cents: 1500,
      yearly_gross_cents: 15000,
      recommended: true,
      buchbar: false,
      is_current: false,
      features: ['Transkription', 'Titel', 'Hashtags', 'Vollautomatik'],
    },
    {
      id: 'studio_cut',
      name: 'Studio Schnitt',
      tier: 'extended',
      price_monthly: 20,
      description: 'KI-Videoschnitt mit 12 Schnitten im Monat.',
      monthly_gross_cents: 2000,
      yearly_gross_cents: 20000,
      recommended: false,
      buchbar: false,
      is_current: false,
      features: ['KI-Videoschnitt', '12 Schnitte im Monat'],
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

export const INTERNAL_HOME_FIXTURE: InternalHomeData = {
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
      {
        id: 'preview-log-2',
        entryDate: '2026-04-21',
        title: 'Vorschau: ein längerer Neuigkeitentext',
        content: 'Dieser Vorschautext prüft, ob lange Neuigkeiten in der schmalen rechten Spalte kompakt bleiben. In der kurzen Ansicht erscheinen nur die ersten Zeilen. Über den Link unter der Liste soll der gesamte Text lesbar werden, ohne die übrigen Karten aus der Spalte zu verdrängen.',
        createdAt: NOW_ISO,
      },
      {
        id: 'preview-log-3',
        entryDate: '2026-04-20',
        title: 'Vorschau: die dritte Neuigkeit',
        content: 'Ein weiterer Testeintrag für die kompakte Liste auf großen und kleinen Bildschirmen.',
        createdAt: NOW_ISO,
      },
      {
        id: 'preview-log-4',
        entryDate: '2026-04-19',
        title: 'Vorschau: ältere Neuigkeit bleibt erreichbar',
        content: 'Dieser vierte Testeintrag liegt außerhalb der kurzen Vorschau. Er muss nach einem Klick auf „Alle Neuigkeiten lesen“ vollständig in der Liste erscheinen.',
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
      description: 'Alte Analyseadressen leiten sauber auf /twitch/analyse um.',
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

export const SOCIAL_POSTER = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="960" height="540"><rect width="960" height="540" fill="#1f1815"/><path d="M0 400 230 150 460 350 700 90 960 360V540H0" fill="#3a2e25"/><circle cx="740" cy="155" r="62" fill="#9c7a3c"/><path d="M0 450H960M160 0V540M480 0V540M800 0V540" stroke="#c5a059" opacity=".15"/><text x="48" y="480" font-family="sans-serif" font-size="28" fill="#e3d4b6">Synthetische Clip-Vorschau</text></svg>')}`;

function socialApproval(platforms: ClipApprovalRecord['approved_platforms'], approved = true): ClipApprovalRecord {
  return {
    clip_db_id: 1, state: approved ? 'approved' : 'awaiting_approval', approved_platforms: platforms,
    approver_user_id: null, decided_at: null, dm_message_id: null, dm_channel_id: null,
    last_sent_at: null, not_scheduled: [],
  };
}

export const SOCIAL_CLIPS_FIXTURE: SocialClipMitPosting[] = [
  { title: 'Die letzte Sekunde: Lash hält den Patron', status: 'awaiting_approval', approval: socialApproval([], false) },
  { title: 'Bebop dreht den Teamfight am Markt', status: 'approved', approval: socialApproval(['youtube']), scheduled_at: { youtube: '2026-10-12T16:00:00Z' }, upload_states: { youtube: 'pending', tiktok: 'waiting_tiktok_approval' } },
  { title: 'Ein ruhiger Moment vor dem nächsten Push', status: 'pending', enrichment_status: 'transcribing' },
  { title: 'Paradox rettet den Rückzug', status: 'published_partial', approval: socialApproval(['tiktok']), upload_states: { tiktok: 'inbox' } },
  { title: 'Haze im Finale: Upload gespeichert, privat auf YouTube', status: 'published_all', approval: socialApproval(['youtube']), platform_status: { youtube: true, tiktok: false, instagram: false }, youtube_visibility: 'private', upload_states: { youtube: 'completed' } },
  { title: 'Der nächste Versuch wartet auf die Verbindung', status: 'failed', approval: socialApproval(['youtube', 'instagram']), upload_states: { youtube: 'failed', instagram: 'waiting_connection' }, upload_errors: { youtube: 'Die synthetische Übertragung ist fehlgeschlagen.', instagram: 'connection_missing' } },
].map((entry, index) => ({
  clip_db_id: index + 1, clip_id: `synthetic-${index + 1}`, clip_url: null,
  thumbnail_url: SOCIAL_POSTER, streamer_login: 'midcore_live',
  created_at: '2026-10-10T16:00:00Z', duration_seconds: 32 + index * 4,
  view_count: 248 + index * 117, game_name: 'Deadlock',
  source_kind: 'twitch', upload_local_path: null, retention_until: '2026-10-18T16:00:00Z',
  discarded_at: null, platform_status: { youtube: false, tiktok: false, instagram: false },
  layout_override: null, effective_layout: DEFAULT_LAYOUT,
  ...entry,
})) as SocialClipMitPosting[];

export const SOCIAL_PLATFORMS_FIXTURE: PlatformStatus[] = ['youtube', 'tiktok', 'instagram'].map((platform) => ({
  platform, connected: platform !== 'instagram', expired: false, username: platform === 'instagram' ? null : 'preview_creator',
  automatically_renewed: true, uses_global_fallback: platform === 'tiktok',
  capabilities: { upload: true, statistics: platform !== 'tiktok', upload_mode: 'direct', reason: null },
  authorization: { source: 'stored', upload_scope_status: platform === 'youtube' ? 'sufficient' : platform === 'tiktok' ? 'missing' : 'unknown',
    required_upload_scopes: platform === 'youtube' ? ['https://www.googleapis.com/auth/youtube.upload'] : platform === 'tiktok' ? ['video.publish'] : [], missing_upload_scopes: platform === 'youtube' ? [] : platform === 'tiktok' ? ['video.publish'] : null,
    channel_account_required: platform === 'tiktok' },
}));

const SOCIAL_PLAN_FIXTURE: PostingPlan = {
  streamer_login: 'midcore_live', approval_mode: 'manual', approval_modes: ['manual', 'veto_window', 'full_auto'],
  timezone: 'Europe/Berlin', subtitles_enabled: true,
  platforms: ['youtube', 'tiktok', 'instagram'].map((platform) => ({ platform: platform as 'youtube' | 'tiktok' | 'instagram', auto_post: platform !== 'instagram', posts_per_week: 3, max_posts_per_day: 1, post_times: ['18:00'], next_slot: '2026-10-12T16:00:00Z' })),
  categories: [{ category_key: 'deadlock', display_name: 'Deadlock', enrichment_enabled: true, auto_post: true }],
  pool: { verfuegbare_clips: 4, aktive_plattformen: 2, reicht_fuer_posts: 8, posts_pro_woche: 6, reicht_fuer_tage: 9, warnung: false },
};

export function getPreviewSocialResponse(path: string, init: RequestInit = {}, scenario = typeof window === 'undefined' ? '' : new URLSearchParams(window.location.search).get('fixture') ?? ''): Response {
  const url = new URL(path, 'http://localhost');
  const pathname = url.pathname;
  const json = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json' } });
  if ((init.method ?? 'GET').toUpperCase() !== 'GET') {
    const tour = onPublicTour();
    const error = {
      error: tour ? 'demo_readonly' : 'preview_read_only',
      message: tour ? 'In der Demo kannst du nichts ändern.' : 'Die lokale Vorschau speichert, verbindet und veröffentlicht nichts.',
    };
    if (SOCIAL_WORKSPACE_PREVIEW && scenario === 'legacy-save-pending') {
      const body = new ReadableStream({ start(controller) {
        setTimeout(() => { controller.enqueue(new TextEncoder().encode(JSON.stringify(error))); controller.close(); }, 2000);
      } });
      return new Response(body, { status: 409, headers: { 'Content-Type': 'application/json' } });
    }
    return json(error, tour ? 403 : 409);
  }
  if (SOCIAL_WORKSPACE_PREVIEW && pathname.startsWith('/twitch/social-media/api/ai/')) {
    const owner = url.searchParams.get('twitch_user_id');
    if (!owner || !['123456789', '987654321'].includes(owner)) return json({ error: 'channel_not_found' }, 403);
    if (scenario === 'ai-error') return json({ error: 'preview_ai_unavailable' }, 503);
    if (scenario === 'ai-loading') return new Response(new ReadableStream(), { headers: { 'Content-Type': 'application/json' } });
    const aiScenario: AiPreviewScenario = scenario === 'ai-rendering' ? 'render' : scenario === 'ai-last-version' ? 'failed' : scenario === 'ai-revisions' ? 'revisions' : 'review';
    if (pathname.endsWith('/consent')) return json(aiPreviewConsent(aiScenario));
    if (pathname.endsWith('/options')) return json({ multi_output_enabled: false, experimental_ai_layout: false });
    const sample = aiPreviewJob(aiScenario);
    const job = sample && {
      ...sample,
      clip_db_id: 1,
      original_url: null,
      versions: aiScenario === 'revisions' ? sample.versions : sample.versions.map((version) => ({ ...version, file_url: null })),
      ...(scenario === 'ai-unknown-cost' ? { cost_basis: 'unknown', estimated_provider_cost_usd: null, measured_provider_cost_usd: null } : {}),
    };
    if (pathname.endsWith('/jobs')) return json({ items: scenario === 'ai-empty' || !job ? [] : [job] });
    if (job && pathname.endsWith(`/jobs/${job.id}`)) return json(job);
    return json({ error: 'preview_fixture_missing' }, 404);
  }
  const streamer = SOCIAL_WORKSPACE_PREVIEW && url.searchParams.get('twitch_user_id') === '987654321' ? 'preview_second' : 'midcore_live';
  if (pathname.endsWith('/access/me')) return json({ allowed: true, streamer: 'midcore_live', isAdmin: !onPublicTour() });
  if (pathname.endsWith('/access')) return json({ items: [{ twitch_user_id: '123456789', streamer_login: 'midcore_live', granted: true }] });
  if (pathname.endsWith('/platforms/status')) {
    if (scenario === 'accounts-error') return json({ error: 'platform_status_failed' }, 503);
    return json({ platforms: scenario === 'unknown' ? [] : SOCIAL_PLATFORMS_FIXTURE });
  }
  if (pathname.endsWith('/streamer-layout')) return json({ streamer_login: 'midcore_live', layout: DEFAULT_LAYOUT, cam_enabled: true, mode: 'stacked', updated_at: null, updated_by: null });
  if (pathname.endsWith('/posting-plan')) return json({ ...SOCIAL_PLAN_FIXTURE, streamer_login: streamer });
  if (pathname.endsWith('/settings/vod-archive')) return json({ streamer_login: 'midcore_live', enabled: true, privacy: 'private', privacy_options: ['private', 'unlisted', 'public'], privacy_forced: true });
  if (pathname.endsWith('/admin/clips')) {
    if (scenario === 'clips-error') return json({ error: 'clip_list_failed' }, 503);
    const page = Number(url.searchParams.get('page') ?? 1);
    const pageSize = Number(url.searchParams.get('page_size') ?? 100);
    const items = SOCIAL_WORKSPACE_PREVIEW && scenario === 'library-empty' ? [] : SOCIAL_CLIPS_FIXTURE;
    return json({ items: items.slice((page - 1) * pageSize, page * pageSize).map(clip => ({ ...clip, streamer_login: streamer })), total: items.length, page, page_size: pageSize });
  }
  if (pathname.endsWith('/vod-archive')) return json({ total: 1, items: [{ id: 1, twitch_id: 'synthetic-vod', channel: 'midcore_live', twitch_user_id: '123456789', title: 'Deadlock-Abend: Rangliste und Community', duration_sec: 7380, recorded_at: '2026-10-10T16:00:00Z', discovered_at: '2026-10-10T18:15:00Z', status: 'uploaded', status_label: 'YouTube-Upload abgeschlossen', display_status: 'youtube_uploaded', youtube_complete: true, youtube_verified_complete: false, can_check_youtube: false, drive_complete: false, confirmed_parts: 1, total_parts: 1, can_retry: false, can_drive: false, reason: null, drive_url: null, drive_requested: false, last_attempt_at: '2026-10-10T18:30:00Z', uploaded_at: '2026-10-10T18:45:00Z', parts: [{ index: 1, status: 'uploaded', youtube_video_id: null }], needs_connection: false }] });
  if (pathname.endsWith('/enrichment')) return json({ clip_db_id: 1, status: 'done', title_youtube: 'Der letzte Push', title_tiktok: 'Der letzte Push', title_instagram: 'Der letzte Push', description_youtube: 'Ein Moment aus der synthetischen Vorschau.', description_tiktok: '', description_instagram: '', hashtags_youtube: ['Deadlock'], hashtags_tiktok: [], hashtags_instagram: [], detected_terms: [], transcript_raw: null, transcript_corrected: null, transcript_segments: null, llm_provider: null, llm_model: null, cost_usd_estimate: null });
  if (pathname.endsWith('/preview')) {
    const id = Number(pathname.match(/clips\/(\d+)\/preview$/)?.[1] ?? 1);
    const status = scenario === 'preview-rendering' ? 'rendering' : scenario === 'preview-error' ? 'error' : null;
    return json({ clip_db_id: id, status, ready: false });
  }
  return json({ error: 'preview_fixture_missing', message: 'Für diese Ansicht gibt es noch keine lokalen Vorschau-Daten.' }, 404);
}

function internalHomeApiFixture(): unknown {
  const tour = onPublicTour();
  const displayName = tour ? 'MidCore Live' : 'Local Preview Creator';
  return {
    profile: {
      twitch_login: 'midcore_live',
      twitch_user_id: '123456789',
      display_name: displayName,
    },
    status: {
      oauth: {
        connected: true,
        status: 'connected',
        granted_scopes: [
          'channel:manage:raids',
          'moderator:read:followers',
          'channel:read:subscriptions',
        ],
        missing_scopes: [],
        reconnect_url: '/twitch/dashboard',
        profile_url: '/twitch/dashboard',
        last_checked_at: NOW_ISO,
      },
      discord: {
        connected: true,
        status: 'connected',
        connect_url: '/twitch/verwaltung',
        last_checked_at: NOW_ISO,
      },
      steam: { connected: true, status: 'connected', connect_url: '/twitch/verwaltung' },
      raid_status: { state: 'active', read_only: true },
    },
    kpis: {
      streams_count: 18,
      avg_viewers: 126,
      follower_delta: 214,
      bot_bans_keyword_count: 7,
    },
    recent_streams: [
      {
        started_at: '2026-04-21T17:00:00Z',
        ended_at: '2026-04-21T20:10:00Z',
        duration_seconds: 11400,
        avg_viewers: 142,
        peak_viewers: 221,
        follower_delta: 34,
        title: 'Deadlock Ranked | Community',
      },
      {
        started_at: '2026-04-19T18:15:00Z',
        ended_at: '2026-04-19T21:00:00Z',
        duration_seconds: 9900,
        avg_viewers: 118,
        peak_viewers: 184,
        follower_delta: 21,
        title: 'Deadlock Ranked + Community Review',
      },
    ],
    bot_activity: {
      events: [
        {
          event_type: 'ops.note',
          timestamp: NOW_ISO,
          status_label: 'Demo',
          target_login: 'midcore_live',
          summary: 'Diese Ansicht zeigt Beispieldaten. Nichts davon ist ein echter Stream.',
          severity: 'info',
          source: 'demo',
        },
      ],
    },
    changelog: {
      can_write: false,
      max_entries: 10,
      entries: [
        {
          id: 'demo-log-1',
          entry_date: '2026-04-22',
          title: 'Dashboard-Rundgang',
          content: 'Das ist eine Demo mit Beispieldaten. Du kannst dich umsehen, nichts wird gespeichert.',
          created_at: NOW_ISO,
        },
      ],
    },
    generated_at: NOW_ISO,
  };
}

const CHALLENGES_ME_FIXTURE = {
  twitch_user_id: '123456789',
  next_reset_at: '2026-05-01T00:00:00Z',
  generated_at: NOW_ISO,
  timezone: 'Europe/Berlin',
  streamer: 'midcore_live',
  quests: [
    { key: 'stream_hours', text: '8 Stunden streamen', progress: 5, goal: 8, completed: false },
    { key: 'raid_out', text: '3 Raids senden', progress: 3, goal: 3, completed: true },
  ],
  quest_assignment_status: 'assigned',
  streak: { freeze_used_this_month: false, current: 4, longest: 11 },
  level: { level: 6, total_points: 420, current_threshold: 350, next_threshold: 500 },
  next_goal: { missing_points: 80, fastest_route: 'Noch 3 Stunden streamen' },
  achievements: [
    {
      key: 'hours',
      name: 'Stunden auf Sendung',
      progress: 42,
      tiers: [
        { target: 10, unlocked: true },
        { target: 50, unlocked: false },
      ],
    },
  ],
  with_us: { people_brought_in_who_stayed: 6, community_hours: 18, received_raids: 4 },
  season: { next_reset_at: '2026-05-01T00:00:00Z', month: '2026-04', points: 420, rank: 12, active_partners: 48 },
};

const CHALLENGES_VIEWERS_FIXTURE = {
  generated_at: NOW_ISO,
  streamer: 'midcore_live',
  recruiters: [{ twitch_user_id: '555', display_name: 'ViewerAlpha', qualified_invites: 3 }],
};

const EFFORT_LEADERBOARD_FIXTURE = {
  month: '2026-04',
  entries: [
    { rank: 1, twitch_login: 'midcore_live', points: 420, raid_boost: true, is_self: true },
    { rank: 2, twitch_login: 'partner_one', points: 390, raid_boost: false, is_self: false },
  ],
  own_position: { rank: 1, twitch_login: 'midcore_live', points: 420, raid_boost: true, is_self: true },
};

const VIEWER_LEADERBOARD_FIXTURE = {
  window: { days: 30 },
  categories: [
    {
      key: 'deadlock',
      title: 'Deadlock',
      count: 2,
      entries: [
        { rank: 1, streamer: 'midcore_live', avg_viewers: 126, max_viewers: 221, samples: 18 },
        { rank: 2, streamer: 'partner_one', avg_viewers: 98, max_viewers: 160, samples: 14 },
      ],
      own_position: { rank: 1, streamer: 'midcore_live', avg_viewers: 126, max_viewers: 221, samples: 18 },
    },
  ],
};

export function getPreviewApiFixture(
  endpoint: string,
  _params: Record<string, string | number | boolean> = {}
): unknown | undefined {
  if (endpoint === '/auth-status') {
    const tour = onPublicTour();
    const partner = tour || (SOCIAL_WORKSPACE_PREVIEW && typeof window !== 'undefined' && new URLSearchParams(window.location.search).get('fixture') === 'partner');
    if (partner) {
      return {
        ...AUTH_STATUS_FIXTURE,
        isAdmin: false,
        isLocalhost: false,
        adminMode: false,
        adminEligible: false,
        level: 'partner',
        canViewAllStreamers: false,
        displayName: tour ? 'MidCore Live' : AUTH_STATUS_FIXTURE.displayName,
        partnerStatus: 'active',
        canAccessAnalyticsDashboard: true,
        plan: {
          planId: 'studio_ki',
          planName: 'Studio KI',
          tier: 'extended',
          isExtended: true,
          expiresAt: null,
          source: tour ? 'demo' : 'local_preview',
          entitlements: [
            'analytics',
            'chat.lurker_tax',
            'chat.promos.disable',
            'raid.priority',
            'social.studio',
            'social.auto_post',
            'social.captions',
          ],
        },
      };
    }
    return AUTH_STATUS_FIXTURE;
  }
  if (endpoint === '/feedback/counts') return { total: 0, unread: 0 };
  if (endpoint === '/streamers') return [{ login: 'midcore_live', twitchUserId: '123456789', isPartner: true }, ...(SOCIAL_WORKSPACE_PREVIEW ? [{ login: 'preview_second', twitchUserId: '987654321', isPartner: true }] : [])];
  if (endpoint === '/billing/catalog') {
    const tour = onPublicTour();
    if (!tour) return BILLING_CATALOG_FIXTURE;
    return {
      plans: BILLING_CATALOG_FIXTURE.plans.map((plan) => ({
        ...plan,
        is_current: plan.id === 'studio_ki',
      })),
    };
  }
  if (endpoint === '/internal-home') return internalHomeApiFixture();
  if (endpoint === '/title/insights') return getPreviewTitleInsights();
  if (endpoint === '/title/settings') {
    return {
      style_preference: 'Direkt, trocken, eher ein Satz als Keyword-Liste. Keine übertriebenen Emojis.',
      experimental_auto_set: false,
      never_words: [],
      style_summary: 'Ø ca. 72 Zeichen; Emojis sind untypisch; häufiger Trenner |; eher ruhige Satzzeichen.',
      oauth_connected: true,
      oauth_url: '/twitch/raid/auth?scope_profile=title&source=title_generator',
      model: 'glm-5.3-flash',
    };
  }
  if (endpoint === '/feedback' && onPublicTour()) return { items: [], next: null };
  if (endpoint === '/challenges/me') return CHALLENGES_ME_FIXTURE;
  if (endpoint === '/challenges/viewers') return CHALLENGES_VIEWERS_FIXTURE;
  if (endpoint === '/leaderboard/effort') return EFFORT_LEADERBOARD_FIXTURE;
  if (endpoint === '/leaderboard') return VIEWER_LEADERBOARD_FIXTURE;
  if (endpoint === '/public/network') return { streamers: [{ login: 'midcore_live' }] };
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
  service_status: 'ready',
  public_ingest_url: 'rtmps://uplink.example/live',
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
    { platform: 'twitch', status: 'rechte_ergaenzen', stream_key_vorhanden: true },
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
      rtmp_url: 'rtmps://ingest.global-contribute.live-video.net:443/app',
      enabled: true,
      requested: { width: 2560, height: 1440, fps: 60, bitrate_kbps: 16000 },
      active_profile: null, output_state: 'failed', publication_confirmed: false,
      reason: 'Die Prüfung dieses Twitch-Profils ist noch offen.',
    },
    {
      platform: 'youtube',
      rtmp_url: 'rtmps://a.rtmps.youtube.com:443/live2',
      enabled: true,
      requested: { width: 2560, height: 1440, fps: 60, bitrate_kbps: 18000 },
      active_profile: null,
      output_state: 'sending', publication_confirmed: false,
    },
  ],
};

// Aktueller Dienstvertrag: Quelle und Ziel müssen vor Empfehlungen geprüft werden.
const UPLINK_CAPS_FIXTURE = {
  platforms: ['twitch', 'kick', 'youtube', 'tiktok'].map(platform => ({
    platform, recommended_width: null, recommended_height: null,
    recommended_fps: null, recommended_bitrate_kbps: null, force_cbr: true,
    verification: 'requires_source_and_target',
  })),
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
    budgetMinutesPerHour: 3,
    minIntervalMinutes: 45,
    startupDelayMinutes: 20,
    quietWindowMinutes: 5,
    actionLeadSeconds: 60,
    chatNoticeBeforeAd: true,
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
    plan: {
      source: 'own',
      fit: 'good',
      nextBlockAt: '2026-04-22T09:38:00Z',
      blockSeconds: 30,
      blocksPerHour: 6,
      budgetUsedSecondsThisHour: 60,
    },
    currentReason: 'in_match',
    scopes: {
      read: true,
      snooze: true,
      commercial: true,
    },
    steam: {
      linked: true,
      state: 'in_match',
      hero: 'Haze',
      stage: 'laning',
      observedAt: NOW_ISO,
    },
  },
};

const ENGAGEMENT_SETTINGS_FIXTURE = {
  settings: [
    {
      channelLogin: 'midcore_live',
      enabled: true,
      steamId: '76561198000000000',
      personaOverride: null,
      tabuTopics: [],
      enabledAt: NOW_ISO,
      enabledBy: 'earlysalty',
      updatedAt: NOW_ISO,
    },
  ],
  isSuperMod: false,
  actorLogin: 'midcore_live',
};

const DONO_GOAL_FIXTURE = {
  login: 'preview',
  configured: true,
  title: 'Neues Setup für den Stream',
  goal: 300,
  currency: 'EUR',
  count_from: '2026-10-01T00:00:00+00:00',
  se_channel_id: '5b2e2007760aeb3b1c7a3f4d',
  connected: true,
};

const COMMAND_NAME_FIXTURES = [
  ['raid', '!raid', 'raids', 'Raids', 'Startet einen Raid auf den nächsten Partnerkanal.'],
  ['lurk', '!lurk', 'chat', 'Chat', 'Meldet dich als stillen Zuschauer an.'],
  ['clip', '!clip', 'chat', 'Chat', 'Schneidet die letzten Sekunden als Clip.'],
  ['rank', '!rank', 'deadlock', 'Deadlock', 'Zeigt deinen aktuellen Rang.'],
].map(([command, name, group, groupLabel, summary]) => ({
  command,
  default_name: name,
  default_aliases: [],
  custom_name: null,
  effective_name: name,
  effective_aliases: [],
  group,
  group_label: groupLabel,
  summary,
}));

const STREAMER_SETTINGS_FIXTURES: Record<string, unknown> = {
  '/twitch/api/v2/streamer/moderation/settings': {
    global_ban_enabled: true,
    scam_pitch_enabled: true,
    spam_autoban_enabled: true,
    sus_invite_enabled: false,
  },
  '/twitch/api/v2/streamer/scam-guard/settings': {
    enabled: true,
    mode: 'timeout',
    threshold: 0.9,
    suggestion_floor: 0.7,
  },
  '/twitch/api/v2/streamer/sub-reminder-settings': { enabled: true, available: true },
  '/twitch/api/v2/streamer/greeting-settings': { greeting_reply_enabled: true },
  '/twitch/api/v2/streamer/silent-settings': { silent_ban: false, silent_raid: true },
  '/twitch/api/v2/streamer/stat-command-settings': {
    commands: {
      rank: true,
      wins: true,
      winrate: true,
      mmr: true,
      live: true,
      lastmatch: true,
      streak: false,
      mostplayed: true,
    },
  },
  '/twitch/api/v2/streamer/lurker-tax-settings': {
    lurker_tax_enabled: true,
    has_moderator_read_chatters: true,
    reward_present: true,
  },
  '/twitch/api/v2/streamer/lurk-command-settings': { lurk_command_enabled: true },
  '/twitch/api/v2/streamer/command-names': { commands: COMMAND_NAME_FIXTURES },
  '/twitch/api/v2/streamer/clip-command-settings': {
    clip_command_enabled: true,
    clip_command_duration_seconds: 30,
    clip_command_duration_configured: true,
  },
  '/twitch/api/v2/streamer/private-vod-clips': { enabled: false, jobs: [] },
  '/twitch/api/v2/streamer/title-command-settings': { title_command_enabled: true },
};

const AUTOPILOT_SCHEDULE_FIXTURE = {
  settings: {
    enabled: true,
    paused: false,
    ai_editing_enabled: false,
    lead_time_hours: 24,
    timezone: 'Europe/Berlin',
    schedule_slots: [
      { weekday: 1, local_time: '18:00' },
      { weekday: 3, local_time: '18:00' },
      { weekday: 5, local_time: '17:00' },
    ],
    target_platforms: ['youtube', 'tiktok'],
  },
  persisted: true,
  schedule_scope: 'shared',
  distribution_policy: 'same_clip',
  next_slot: '2026-04-24T15:00:00Z',
  next_slots: { youtube: '2026-04-24T15:00:00Z', tiktok: '2026-04-24T15:00:00Z' },
  jobs: [],
  review_count: 0,
  production_hold: null,
  connected_platforms: ['youtube'],
  test_run: null,
};

const AUTOPILOT_POLICY_FIXTURE = {
  pipeline_mode: 'upload_text',
  review_policy: 'required',
  metadata_scope: 'shared',
  text_defaults: {
    shared: { title: '', description: '', hashtags: ['deadlock'] },
    per_platform: {},
  },
  schedule_scope: 'shared',
  platform_schedule_slots: {},
  distribution_policy: 'same_clip',
  policy_revision: 1,
};

export function getPreviewPathFixture(pathname: string): unknown | undefined {
  if (pathname === '/twitch/social-media/api/autopilot/schedule') return AUTOPILOT_SCHEDULE_FIXTURE;
  if (pathname === '/twitch/social-media/api/autopilot/policy') return AUTOPILOT_POLICY_FIXTURE;
  if (pathname === '/twitch/api/v2/streamer/community-hints') {
    return { enabled: true, can_disable: true };
  }
  if (pathname === '/twitch/api/v2/streamer/dono-goal') return DONO_GOAL_FIXTURE;
  if (Object.hasOwn(STREAMER_SETTINGS_FIXTURES, pathname)) return STREAMER_SETTINGS_FIXTURES[pathname];
  if (pathname === '/twitch/api/v2/streamer/ad-manager') return AD_MANAGER_FIXTURE;
  if (pathname === '/twitch/api/v2/streamer/ad-manager/history') {
    return {
      sessionStartedAt: '2026-04-22T08:00:00Z',
      summary: {
        blocksRun: 2,
        blocksInWindow: 3,
        budgetSecondsUsed: 180,
        budgetSecondsPlanned: 270,
        postponed: 1,
      },
      entries: [
        {
          at: '2026-04-22T08:55:00Z',
          decision: 'commercial',
          reason: 'budget',
          blockSeconds: 90,
          detail: null,
        },
      ],
    };
  }
  if (pathname === '/twitch/api/v2/engagement/settings') return ENGAGEMENT_SETTINGS_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/me') return UPLINK_ME_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/destinations') return UPLINK_DESTINATIONS_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/caps') return UPLINK_CAPS_FIXTURE;
  if (pathname === '/twitch/api/v2/uplink/admin/waitlist') return UPLINK_ADMIN_WAITLIST_FIXTURE;
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
