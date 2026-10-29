const HOUR = 3_600_000;
const DAY = 24 * HOUR;

function at(daysAgo: number, hour = 18, minute = 0): string {
  const base = new Date(Date.now() - daysAgo * DAY);
  base.setUTCHours(hour, minute, 0, 0);
  return base.toISOString();
}

function day(daysAgo: number): string {
  return at(daysAgo).slice(0, 10);
}

function nowIso(): string {
  return new Date().toISOString();
}

function firstOfNextMonth(): string {
  const now = new Date();
  return new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() + 1, 1)).toISOString();
}

const TOUR_STREAMS = [
  { daysAgo: 1, hour: 17, hours: 3.5, avg: 148, peak: 236, followers: 41, chatters: 212, messages: 3940, title: 'Deadlock Ranked Push | Community-Lobbys danach' },
  { daysAgo: 3, hour: 18, hours: 3, avg: 131, peak: 204, followers: 27, chatters: 187, messages: 3310, title: 'Neue Helden testen | Deadlock mit Chat' },
  { daysAgo: 5, hour: 17, hours: 4, avg: 139, peak: 221, followers: 33, chatters: 198, messages: 3725, title: 'Deadlock Ranked | Zuschauer-Builds' },
  { daysAgo: 8, hour: 18, hours: 2.5, avg: 117, peak: 176, followers: 19, chatters: 161, messages: 2580, title: 'Patch-Analyse und Ranked' },
  { daysAgo: 10, hour: 17, hours: 3, avg: 122, peak: 189, followers: 22, chatters: 170, messages: 2860, title: 'Deadlock mit der Community' },
];

function tourRecentStreams() {
  return TOUR_STREAMS.map(s => ({
    started_at: at(s.daysAgo, s.hour),
    ended_at: new Date(Date.parse(at(s.daysAgo, s.hour)) + s.hours * HOUR).toISOString(),
    duration_seconds: Math.round(s.hours * 3600),
    avg_viewers: s.avg,
    peak_viewers: s.peak,
    follower_delta: s.followers,
    title: s.title,
  }));
}

function tourViewersOverTime() {
  const points = [];
  for (let minute = 0; minute <= 210; minute += 10) {
    const rise = Math.min(1, minute / 45);
    const wave = Math.sin(minute / 23) * 14;
    const raidBump = minute >= 120 && minute <= 150 ? 38 : 0;
    points[points.length] = { t_seconds: minute * 60, viewers: Math.round(40 + rise * 120 + wave + raidBump) };
  }
  return points;
}

function tourImpactEvents() {
  return [
    { event_type: 'raid.sent', timestamp: at(1, 20, 30), status_label: 'Raid', target_login: 'lumen_plays', summary: 'Auto-Raid zu lumen_plays mit 152 Zuschauern', viewer_count: 152, success: true, severity: 'info', source: 'raid' },
    { event_type: 'raid.received', timestamp: at(1, 19, 5), status_label: 'Raid', actor_login: 'nordlicht_dl', summary: 'Raid von nordlicht_dl mit 38 Zuschauern', viewer_count: 38, success: true, severity: 'info', source: 'raid' },
    { event_type: 'mod.ban', timestamp: at(1, 18, 12), status_label: 'Schutz', target_login: 'cheap_viewers_bot', summary: 'Spam-Bot mit Viewer-Werbung automatisch gebannt', reason: 'Spam-Muster', success: true, severity: 'warning', source: 'protection' },
    { event_type: 'discord.announce', timestamp: at(1, 17, 1), status_label: 'Discord', summary: 'Live-Ankündigung im Community-Discord gepostet', success: true, severity: 'info', source: 'discord' },
    { event_type: 'clip.published', timestamp: at(2, 12), status_label: 'Clip', summary: 'Clip „Der letzte Push“ auf YouTube Shorts veröffentlicht', success: true, severity: 'info', source: 'social' },
    { event_type: 'raid.sent', timestamp: at(3, 21), status_label: 'Raid', target_login: 'vex_deadlock', summary: 'Auto-Raid zu vex_deadlock mit 134 Zuschauern', viewer_count: 134, success: true, severity: 'info', source: 'raid' },
  ];
}

export function tourInternalHome(): unknown {
  const streams = tourRecentStreams();
  const last = streams[0];
  return {
    profile: { twitch_login: 'midcore_live', twitch_user_id: '123456789', display_name: 'MidCore Live' },
    status: {
      oauth: {
        connected: true,
        status: 'connected',
        granted_scopes: ['channel:manage:raids', 'moderator:read:followers', 'channel:read:subscriptions'],
        missing_scopes: [],
        reconnect_url: '/twitch/dashboard',
        profile_url: '/twitch/dashboard',
        last_checked_at: nowIso(),
      },
      discord: { connected: true, status: 'connected', connect_url: '/twitch/verwaltung', last_checked_at: nowIso() },
      steam: { connected: true, status: 'connected', connect_url: 'https://deutsche-deadlock-community.de/twitch/verwaltung' },
      raid_status: { state: 'active', read_only: true },
    },
    kpis: { streams_count: 14, avg_viewers: 134, follower_delta: 214, bot_bans_keyword_count: 7 },
    recent_streams: streams,
    bot_impact: { events: tourImpactEvents(), note: 'Beispieldaten: Kanal, Zuschauer und Raids sind erfunden.' },
    bot_activity: { events: tourImpactEvents() },
    changelog: {
      can_write: false,
      max_entries: 10,
      entries: [
        { id: 'tour-log-1', entry_date: day(0), title: 'Willkommen im Rundgang', content: 'Alles hier sind Beispieldaten. Sieh dich in Ruhe um, gespeichert wird nichts.', created_at: nowIso() },
        { id: 'tour-log-2', entry_date: day(6), title: 'Clip-Autopilot aktiv', content: 'Die besten Clips der Woche gehen automatisch auf YouTube, TikTok und Instagram.', created_at: at(6, 10) },
      ],
    },
    health_score: { overall: 78, trend: 6, sub_scores: { growth: 74, retention: 81, engagement: 83, community: 72 } },
    last_stream_summary: {
      started_at: last.started_at,
      ended_at: last.ended_at,
      duration_seconds: last.duration_seconds,
      avg_viewers: last.avg_viewers,
      peak_viewers: last.peak_viewers,
      follower_delta: last.follower_delta,
      chat_messages: TOUR_STREAMS[0].messages,
    },
    week_comparison: {
      current_week: { avg_viewers: 139, total_followers: 101, chat_activity: 10975, stream_hours: 10.5 },
      previous_week: { avg_viewers: 121, total_followers: 76, chat_activity: 8890, stream_hours: 9 },
      changes: { avg_viewers_pct: 14.9, followers_pct: 32.9, chat_activity_pct: 23.5, stream_hours_pct: 16.7 },
      daily_series: {
        avg_viewers: [118, 0, 122, 0, 139, 131, 148],
        followers: [19, 0, 22, 0, 33, 27, 41],
        chat_activity: [2580, 0, 2860, 0, 3725, 3310, 3940],
        stream_hours: [2.5, 0, 3, 0, 4, 3, 3.5],
      },
    },
    personal_bests: {
      peak_viewers: { value: 312, achieved_at: at(24, 19), title: 'Community-Turnier Finale' },
      avg_viewers: { value: 171, achieved_at: at(24, 18), title: 'Community-Turnier Finale' },
      follower_gain: { value: 63, achieved_at: at(24, 18), title: 'Community-Turnier Finale' },
      unique_chatters: { value: 264, achieved_at: at(17, 18), title: 'Patch-Day Livestream' },
      longest_stream_seconds: { value: 23400, achieved_at: at(31, 14), title: 'Subathon-Wochenende' },
    },
    stream_comparison: {
      current_started_at: last.started_at,
      peak_viewers: { current: 236, previous: 204, pct: 15.7 },
      new_followers: { current: 41, previous: 27, pct: 51.9 },
      unique_chatters: { current: 212, previous: 187, pct: 13.4 },
    },
    viewers_over_time: tourViewersOverTime(),
    live_status: { is_live: false, viewer_count: 0, started_at: null, last_seen_at: last.ended_at, title: last.title, game: 'Deadlock' },
    generated_at: nowIso(),
  };
}

export function tourChallengesPatch(): { next_reset_at: string; generated_at: string; referral_url: string } {
  return {
    next_reset_at: firstOfNextMonth(),
    generated_at: nowIso(),
    referral_url: 'https://deutsche-deadlock-community.de/streamer/?ref=midcore_live',
  };
}

export const tourDates = { at, day, nowIso };

const TOUR_CLIP_TITLES = [
  'Die letzte Sekunde: Lash hält den Patron',
  'Bebop dreht den Teamfight am Markt',
  'Paradox rettet den Rückzug',
  'Haze im Finale: 4 Kills in 6 Sekunden',
  'Ein Seven-Ult für die Geschichtsbücher',
  'Mo & Krill graben den Sieg aus',
];

export function tourSocialClips(base: Record<string, unknown>[]): Record<string, unknown>[] {
  const approved = (platforms: string[]) => ({
    clip_db_id: 1, state: 'approved', approved_platforms: platforms, approver_user_id: null,
    decided_at: at(1, 9), dm_message_id: null, dm_channel_id: null, last_sent_at: null, not_scheduled: [],
  });
  const all = ['youtube', 'tiktok', 'instagram'];
  const states: Record<string, unknown>[] = [
    { status: 'awaiting_approval', approval: { ...approved([]), state: 'awaiting_approval', decided_at: null } },
    { status: 'approved', approval: approved(all), scheduled_at: { youtube: at(-1, 16), tiktok: at(-1, 16), instagram: at(-1, 17) }, upload_states: { youtube: 'pending', tiktok: 'pending', instagram: 'pending' } },
    { status: 'published_all', approval: approved(all), platform_status: { youtube: true, tiktok: true, instagram: true }, youtube_visibility: 'public', upload_states: { youtube: 'completed', tiktok: 'completed', instagram: 'completed' } },
    { status: 'published_all', approval: approved(all), platform_status: { youtube: true, tiktok: true, instagram: true }, youtube_visibility: 'public', upload_states: { youtube: 'completed', tiktok: 'completed', instagram: 'completed' } },
    { status: 'approved', approval: approved(['youtube', 'tiktok']), scheduled_at: { youtube: at(-3, 16), tiktok: at(-3, 16) }, upload_states: { youtube: 'pending', tiktok: 'pending' } },
    { status: 'pending', enrichment_status: 'done' },
  ];
  return base.slice(0, states.length).map((clip, index) => {
    const state = states[index];
    const approval = state.approval ? { ...(state.approval as Record<string, unknown>), clip_db_id: index + 1 } : undefined;
    const rest = { ...clip };
    delete rest.upload_errors;
    delete rest.enrichment_status;
    return {
      ...rest,
      clip_db_id: index + 1,
      title: TOUR_CLIP_TITLES[index],
      created_at: at(index + 1, 19),
      retention_until: at(-14 + index, 19),
      view_count: 1240 - index * 137,
      platform_status: { youtube: false, tiktok: false, instagram: false },
      upload_states: {},
      ...state,
      ...(approval ? { approval } : {}),
    };
  });
}

export function tourPlatforms(): Record<string, unknown>[] {
  const names: Record<string, string> = { youtube: 'MidCore Live', tiktok: 'midcore.live', instagram: 'midcore.live', youtube_vod: 'MidCore Live VODs' };
  const scopes: Record<string, string[]> = { youtube: ['https://www.googleapis.com/auth/youtube.upload'], tiktok: ['video.publish'], instagram: ['instagram_business_content_publish'], youtube_vod: ['https://www.googleapis.com/auth/youtube.upload'] };
  return ['youtube', 'tiktok', 'instagram', 'youtube_vod'].map(platform => ({
    platform, connected: true, expired: false, username: names[platform],
    automatically_renewed: true, uses_global_fallback: false,
    capabilities: { upload: true, statistics: true, upload_mode: 'direct', reason: null },
    authorization: { source: 'stored', upload_scope_status: 'sufficient', required_upload_scopes: scopes[platform], missing_upload_scopes: [], channel_account_required: false },
  }));
}

export function tourPlanNextSlots<T extends { platforms: { next_slot?: string | null; auto_post?: boolean }[] }>(plan: T): T {
  return { ...plan, platforms: plan.platforms.map((p, index) => ({ ...p, auto_post: true, next_slot: at(-1 - index, 16) })) };
}

const NOTIFY = { dashboard: true, discord: true, email: false, ready_for_review: true, publication_deadline: true, processing_failed: true, publication_failed: true, empty_pool: true, review_expiring: true, reminder_hours: 24 };
const CONNECTIONS = [
  { platform: 'youtube', connected: true, account_id: 'tour-youtube' },
  { platform: 'instagram', connected: true, account_id: 'tour-instagram' },
  { platform: 'tiktok', connected: true, account_id: 'tour-tiktok' },
];

function published(platform: string, daysAgo: number) {
  return { platform, state: 'published', attempts: 1, next_retry_at: null, external_id: `tour-${platform}-${daysAgo}`, account_id: `tour-${platform}`, reason: null, visibility: 'public' };
}

function pendingDest(platform: string) {
  return { platform, state: 'pending', attempts: 0, next_retry_at: null, external_id: null, account_id: `tour-${platform}`, reason: null, visibility: null };
}

function approvalsFor(platforms: string[], daysAgo: number) {
  return platforms.map(platform => ({ platform, state: 'approved', approver_user_id: 'midcore_live', decided_at: at(daysAgo, 9) }));
}

export function tourPublishSnapshot(): unknown {
  const all = ['youtube', 'instagram', 'tiktok'];
  const video = (id: number, clip: number, status: string, mode: string, extra: Record<string, unknown>) => ({
    id, clip_id: clip, version_no: 1, variant_index: 0, mode, status, duration_ms: 28000 + id * 900,
    expires_at: at(-10, 16), needs_review: false, review_reasons: [], artifact_state: 'present', cleanup_error: null,
    approvals: [], event: null, ...extra,
  });
  return {
    settings: { publish_mode: 'review', review_mode: 'review', paused: false, notifications: NOTIFY },
    target_platforms: all,
    connections: CONNECTIONS,
    videos: [
      video(201, 1, 'review', 'ai', { needs_review: true, review_reasons: ['Schau kurz drüber, ob der Schnitt den Höhepunkt trifft.'] }),
      video(202, 2, 'scheduled', 'ai', { approvals: approvalsFor(all, 1), event: { id: 701, produced_video_id: 202, scheduled_at: at(-1, 16), executed_at: null, completed_at: null, destinations: all.map(pendingDest) } }),
      video(203, 5, 'scheduled', 'static', { approvals: approvalsFor(['youtube', 'tiktok'], 1), event: { id: 702, produced_video_id: 203, scheduled_at: at(-3, 16), executed_at: null, completed_at: null, destinations: ['youtube', 'tiktok'].map(pendingDest) } }),
      video(204, 3, 'published', 'ai', { approvals: approvalsFor(all, 4), event: { id: 703, produced_video_id: 204, scheduled_at: at(3, 16), executed_at: at(3, 16), completed_at: at(3, 16, 4), destinations: all.map(p => published(p, 3)) } }),
      video(205, 4, 'published', 'static', { approvals: approvalsFor(all, 6), event: { id: 704, produced_video_id: 205, scheduled_at: at(5, 16), executed_at: at(5, 16), completed_at: at(5, 16, 3), destinations: all.map(p => published(p, 5)) } }),
    ],
    notifications: [{ id: 951, kind: 'ready_for_review', content: 'Ein neues KI-Video wartet auf deine Freigabe.', created_at: at(0, 8), discord_state: 'sent', read_at: null }],
    capabilities: { discord: true, email: false, tiktok_effect_boundary: false },
  };
}

const TOUR_PREFERENCES = { clip_date_filter: { kind: 'all' }, clip_sort: 'newest_first', timezone: 'Europe/Berlin' };

function tourImport() {
  return {
    id: 41, twitch_user_id: '123456789', clip_date_filter: { kind: 'all' }, timezone: 'Europe/Berlin',
    started_at: at(30, 0), ended_at: at(0, 6), status: 'completed', raw_fetched: 184, eligible_fetched: 152,
    persisted_count: 152, completed_windows: 5, pending_windows: 0, active_window: null, incomplete_windows: [],
    attempts: 1, total_attempts: 1, next_retry_at: null, last_error: null, created_at: at(0, 6), completed_at: at(0, 6, 4),
  };
}

function tourPoolClips() {
  return TOUR_CLIP_TITLES.map((title, index) => ({
    clip_id: `tour-clip-${index + 1}`, clip_db_id: index + 1, source_created_at: at(index + 1, 19), game_id: '1311226066',
    source_metadata: { title, creator_name: index % 2 ? 'nordlicht_dl' : 'midcore_live', duration: 30 + index * 4, game_name: 'Deadlock' },
    view_count: 1240 - index * 137, selected_at: index < 3 ? at(0, 7) : null, first_selected_at: index < 3 ? at(0, 7) : null,
    processed_at: index < 2 ? at(0, 8) : null, published_at: index === 2 || index === 3 ? at(index, 16) : null,
    legacy_reserved: false, eligible: true, selectable: true,
  }));
}

export function tourAutopilotPath(pathname: string): unknown | undefined {
  if (pathname === '/twitch/social-media/api/autopilot/publish') return tourPublishSnapshot();
  if (pathname === '/twitch/social-media/api/autopilot/preferences') return TOUR_PREFERENCES;
  if (pathname === '/twitch/social-media/api/autopilot/imports') return { import: tourImport() };
  if (pathname === '/twitch/social-media/api/autopilot/pool') {
    const clips = tourPoolClips();
    return { clips, total: clips.length, limit: 50, offset: 0, preferences: TOUR_PREFERENCES };
  }
  if (pathname === '/twitch/social-media/api/pipeline/sources') {
    const sources = tourPoolClips().map(clip => ({
      clip_id: clip.clip_id, clip_db_id: clip.clip_db_id, source_kind: 'twitch', eligibility: 'automatic', pooled: true, auto_post: true,
      source_created_at: clip.source_created_at, game_id: clip.game_id, game_name: 'Deadlock', source_metadata: clip.source_metadata,
      view_count: clip.view_count, selected_at: clip.selected_at, processed_at: clip.processed_at, published_at: clip.published_at,
      legacy_reserved: false, discarded_at: null, jobs: [], versions: [], destinations: [], legacy_publications: [],
    }));
    return { sources, total: sources.length, limit: 50, offset: 0, eligibility: null, counts: { automatic: sources.length, manual: 0, legacy_non_deadlock_visible: 0 }, preferences: TOUR_PREFERENCES, latest_import: tourImport() };
  }
  return undefined;
}

export function tourUplinkMe<T extends Record<string, unknown>>(base: T): T {
  return {
    ...base,
    verbindungen: ['twitch', 'kick', 'youtube', 'tiktok'].map(platform => ({ platform, status: 'verbunden', stream_key_vorhanden: true })),
  };
}

export function tourUplinkDestinations(): unknown {
  const target = (platform: string, rtmp_url: string, width: number, height: number, bitrate_kbps: number) => ({
    platform, rtmp_url, enabled: true, requested: { width, height, fps: 60, bitrate_kbps },
    active_profile: null, publication_confirmed: false,
  });
  return {
    destinations: [
      target('twitch', 'rtmps://ingest.global-contribute.live-video.net:443/app', 1920, 1080, 8000),
      target('youtube', 'rtmps://a.rtmps.youtube.com:443/live2', 2560, 1440, 18000),
      target('kick', 'rtmps://fa723fc1b171.global-contribute.live-video.net:443/app', 1920, 1080, 8000),
      target('tiktok', 'rtmp://live.tiktok.example/stream', 1080, 1920, 6000),
    ],
  };
}

export function tourSchedulePatch<T extends Record<string, unknown>>(base: T): T {
  const settings = base.settings as Record<string, unknown>;
  return {
    ...base,
    settings: { ...settings, target_platforms: ['youtube', 'tiktok', 'instagram'] },
    next_slot: at(-1, 16),
    next_slots: { youtube: at(-1, 16), tiktok: at(-1, 16), instagram: at(-1, 17) },
    connected_platforms: ['youtube', 'tiktok', 'instagram'],
  };
}
