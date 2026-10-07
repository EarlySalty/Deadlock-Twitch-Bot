import { withCookieCredentials } from './core';
import { fetchAuthStatus } from './auth';
import type {
  ApprovalMode,
  ClipAnalyticsResponse,
  ClipApprovalRecord,
  ClipEnrichment,
  ClipListResponse,
  ClipStatus,
  LayoutPayload,
  PlatformScheduleEntry,
  PostingPlan,
  SocialClip,
  SocialMediaReport,
  SocialMediaReportKind,
  SocialPlatform,
  StreamerLayoutResponse,
  UploadResponse,
  VocabEntry,
  VocabListResponse,
  VodArchiveSettings,
} from '@/types/socialMedia';

const ADMIN_PREFIX = '/twitch/social-media/api/admin';
const UPLOAD_PATH = '/twitch/social-media/api/clips/upload';

export interface ArchivedVod {
  id: number;
  twitch_id: string;
  channel: string;
  twitch_user_id: string | null;
  title: string;
  duration_sec: number;
  recorded_at: string | null;
  discovered_at: string;
  status: string;
  status_label: string;
  reason: string | null;
  drive_url: string | null;
  drive_requested: boolean;
  last_attempt_at: string | null;
  parts: { index: number; status: string; youtube_video_id: string | null }[];
  needs_connection: boolean;
}

export async function fetchArchivedVods(twitchUserId: string | undefined, page = 1): Promise<{ items: ArchivedVod[]; total: number }> {
  const qs = buildQuery({ twitch_user_id: twitchUserId, page });
  return fetchJson(`/twitch/social-media/api/vod-archive${qs}`);
}

export async function archiveAction(id: number, action: 'retry' | 'drive' | 'hide', twitchUserId?: string): Promise<void> {
  await fetchJson('/twitch/social-media/api/vod-archive', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id, action, twitch_user_id: twitchUserId }),
  });
}

/**
 * Fehler mit stabilem Code. Der Code kommt aus dem Backend (`error`-Feld) und
 * wird erst an der Anzeigestelle in einen Satz uebersetzt: ein deutscher Text,
 * der hier fest verdrahtet ist, laeuft nie durch `t()` und steht im englischen
 * Dashboard auf Deutsch da.
 */
export class SocialMediaApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(code: string, message?: string, status = 0) {
    super(message ?? code);
    this.code = code;
    this.status = status;
    this.name = 'SocialMediaApiError';
  }
}

export class SocialMediaForbiddenError extends SocialMediaApiError {
  constructor(code: string = 'admin_required', status = 403) {
    super(code, code, status);
    this.name = 'SocialMediaForbiddenError';
  }
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, withCookieCredentials(init));
  if (response.status === 403 || response.status === 401) {
    throw new SocialMediaForbiddenError('admin_required', response.status);
  }
  if (!response.ok) {
    let code = `http_${response.status}`;
    let message: string | undefined;
    try {
      const data = await response.json();
      if (data?.error) code = String(data.error);
      if (data?.message) message = String(data.message);
    } catch {
      // ignore JSON parse errors
    }
    throw new SocialMediaApiError(code, message, response.status);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

function buildQuery(params: Record<string, string | number | undefined>): string {
  const search = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {
    if (value === undefined || value === null || value === '') return;
    search.set(key, String(value));
  });
  const qs = search.toString();
  return qs ? `?${qs}` : '';
}

/** Was die eigene Session im Social-Media-Dashboard darf. */
export interface SocialMediaAccess {
  allowed: boolean;
  streamer: string | null;
  isAdmin: boolean;
}

/** Ein Eintrag der Freigabe-Liste (Admin-Sicht). */
export interface PartnerAccessEntry {
  twitch_user_id: string;
  streamer_login: string;
  granted: boolean;
}

export async function fetchMyAccess(): Promise<SocialMediaAccess> {
  return fetchJson<SocialMediaAccess>('/twitch/social-media/api/access/me');
}

export async function fetchPartnerAccessList(): Promise<PartnerAccessEntry[]> {
  const data = await fetchJson<{ items: PartnerAccessEntry[] }>('/twitch/social-media/api/access');
  return data.items ?? [];
}

export async function setPartnerAccess(
  twitchUserId: string,
  granted: boolean,
): Promise<void> {
  await fetchJson('/twitch/social-media/api/access', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: twitchUserId, granted }),
  });
}

export async function fetchStreamerLayout(twitchUserId: string | undefined): Promise<StreamerLayoutResponse> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson<StreamerLayoutResponse>(`${ADMIN_PREFIX}/streamer-layout${qs}`);
}

export async function saveStreamerLayout(input: {
  twitch_user_id?: string;
  layout: LayoutPayload;
}): Promise<StreamerLayoutResponse> {
  const { layout } = input;
  return fetchJson<StreamerLayoutResponse>(`${ADMIN_PREFIX}/streamer-layout`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      twitch_user_id: input.twitch_user_id,
      layout,
      cam_enabled: layout.cam_enabled,
      mode: layout.mode,
    }),
  });
}

export interface ClipListParams {
  status?: ClipStatus | 'all';
  twitch_user_id?: string;
  page?: number;
  page_size?: number;
}

/**
 * Je Plattform ein Eintrag. `null` heisst "hier ist nichts schiefgegangen"
 * beziehungsweise "hier ist nichts geplant"; ein fehlender Schluessel heisst
 * dasselbe, deshalb sind beide Felder teiloptional.
 */
export type PlattformFehler = Partial<Record<SocialPlatform, string | null>>;
export type PlattformTermine = Partial<Record<SocialPlatform, string | null>>;

/**
 * Was das Backend zusaetzlich am Clip liefert: der Grund eines fehlgeschlagenen
 * Uploads und der geplante Termin je Plattform. Steht hier und nicht in
 * `types/socialMedia.ts`, damit die Antwortform beim Aufrufer bleibt.
 */
export interface ClipPostingInfo {
  youtube_visibility?: 'public' | 'private' | 'unlisted' | null;
  upload_errors?: PlattformFehler | null;
  scheduled_at?: PlattformTermine | null;
  upload_states?: Partial<Record<SocialPlatform, string | null>> | null;
  publish_states?: Partial<Record<SocialPlatform, string | null>> | null;
}

export type SocialClipMitPosting = SocialClip & ClipPostingInfo;

export interface ClipListResponseMitPosting extends Omit<ClipListResponse, 'items'> {
  items: SocialClipMitPosting[];
}

export async function fetchClips(params: ClipListParams = {}, signal?: AbortSignal): Promise<ClipListResponseMitPosting> {
  const qs = buildQuery({
    status: params.status && params.status !== 'all' ? params.status : undefined,
    twitch_user_id: params.twitch_user_id,
    page: params.page,
    page_size: params.page_size,
  });
  return fetchJson<ClipListResponseMitPosting>(`${ADMIN_PREFIX}/clips${qs}`, { signal });
}

export async function fetchClip(clipDbId: number): Promise<SocialClip> {
  return fetchJson<SocialClip>(`${ADMIN_PREFIX}/clips/${clipDbId}`);
}

export type ClipPreviewStatus = 'pending' | 'rendering' | 'ready' | 'error' | null;

export interface ClipPreviewState {
  clip_db_id: number;
  status: ClipPreviewStatus;
  error?: string | null;
  ready: boolean;
}

export async function requestPreview(
  clipDbId: number,
): Promise<{ clip_db_id: number; status: ClipPreviewStatus }> {
  return fetchJson<{ clip_db_id: number; status: ClipPreviewStatus }>(
    `${ADMIN_PREFIX}/clips/${clipDbId}/preview`,
    { method: 'POST' },
  );
}

export async function getPreviewStatus(clipDbId: number): Promise<ClipPreviewState> {
  return fetchJson<ClipPreviewState>(`${ADMIN_PREFIX}/clips/${clipDbId}/preview`);
}

export function previewFileUrl(clipDbId: number): string {
  return `${ADMIN_PREFIX}/clips/${clipDbId}/preview/file`;
}

export interface ClipLayoutOverrideResponse {
  clip_db_id: number;
  layout_override: LayoutPayload | null;
  effective_layout: LayoutPayload;
}

export async function setClipLayoutOverride(
  clipDbId: number,
  layout: LayoutPayload | null,
): Promise<ClipLayoutOverrideResponse> {
  return fetchJson<ClipLayoutOverrideResponse>(`${ADMIN_PREFIX}/clips/${clipDbId}/layout`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ layout }),
  });
}

export async function discardClip(clipDbId: number): Promise<SocialClip | { clip_db_id: number; discarded: boolean }> {
  return fetchJson<SocialClip | { clip_db_id: number; discarded: boolean }>(
    `${ADMIN_PREFIX}/clips/${clipDbId}/discard`,
    { method: 'POST' },
  );
}

export async function fetchClipEnrichment(clipDbId: number): Promise<ClipEnrichment> {
  return fetchJson<ClipEnrichment>(`${ADMIN_PREFIX}/clips/${clipDbId}/enrichment`);
}

export async function fetchClipAnalytics(clipDbId: number): Promise<ClipAnalyticsResponse> {
  return fetchJson<ClipAnalyticsResponse>(`${ADMIN_PREFIX}/analytics/clips/${clipDbId}`);
}

export interface ReportListParams {
  twitch_user_id?: string;
  kind?: SocialMediaReportKind;
  limit?: number;
}

export async function fetchReports(params: ReportListParams = {}): Promise<{ items: SocialMediaReport[] }> {
  const qs = buildQuery({
    twitch_user_id: params.twitch_user_id,
    kind: params.kind,
    limit: params.limit,
  });
  return fetchJson<{ items: SocialMediaReport[] }>(`${ADMIN_PREFIX}/reports${qs}`);
}

export async function runReport(input: {
  kind: Extract<SocialMediaReportKind, 'streamer' | 'cross'>;
  twitch_user_id?: string;
}): Promise<SocialMediaReport> {
  return fetchJson<SocialMediaReport>(`${ADMIN_PREFIX}/reports/run`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
}

export interface EnrichmentEditPayload {
  title_youtube?: string | null;
  title_tiktok?: string | null;
  title_instagram?: string | null;
  description_youtube?: string | null;
  description_tiktok?: string | null;
  description_instagram?: string | null;
  hashtags_youtube?: string[];
  hashtags_tiktok?: string[];
  hashtags_instagram?: string[];
}

export async function saveClipEnrichment(
  clipDbId: number,
  payload: EnrichmentEditPayload,
): Promise<ClipEnrichment> {
  return fetchJson<ClipEnrichment>(`${ADMIN_PREFIX}/clips/${clipDbId}/enrichment`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
}

export async function runClipEnrichment(clipDbId: number, force = false): Promise<ClipEnrichment> {
  return fetchJson<ClipEnrichment>(`${ADMIN_PREFIX}/clips/${clipDbId}/enrichment/run`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ force }),
  });
}

export async function fetchClipApproval(
  clipDbId: number,
): Promise<{ clip_db_id: number; approval: ClipApprovalRecord | null }> {
  return fetchJson<{ clip_db_id: number; approval: ClipApprovalRecord | null }>(
    `${ADMIN_PREFIX}/approval/${clipDbId}`,
  );
}

export interface TikTokPostOptions {
  caption: string;
  privacy_level: string;
  allow_comment: boolean;
  allow_duet: boolean;
  allow_stitch: boolean;
  commercial_content: boolean;
  brand_organic_toggle: boolean;
  brand_content_toggle: boolean;
  consent: boolean;
  creator_username: string;
  credential_id: number;
  platform_user_id: string;
  approved_video_sha256: string;
}

export interface TikTokPostingContext {
  creator: {
    creator_username: string;
    creator_nickname: string;
    privacy_level_options: string[];
    comment_disabled: boolean;
    duet_disabled: boolean;
    stitch_disabled: boolean;
    max_video_post_duration_sec: number;
  };
  caption: string;
  duration_seconds: number;
  approved_video_sha256: string;
  credential_id: number;
  platform_user_id: string;
}

export function fetchTikTokCreatorInfo(clipDbId: number): Promise<TikTokPostingContext> {
  return fetchJson(`/twitch/social-media/api/clips/${clipDbId}/tiktok/creator-info`, { cache: 'no-store' });
}

export async function decideClipApproval(input: {
  clipDbId: number;
  decision: 'approve' | 'skip' | 'edit';
  platforms: SocialPlatform[];
  tiktokOptions?: TikTokPostOptions;
}): Promise<{ clip_db_id: number; approval: ClipApprovalRecord | null; clip: SocialClip | null }> {
  return fetchJson<{ clip_db_id: number; approval: ClipApprovalRecord | null; clip: SocialClip | null }>(
    `${ADMIN_PREFIX}/approval/${input.clipDbId}/decision`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        decision: input.decision,
        platforms: input.platforms,
        tiktok_options: input.tiktokOptions,
      }),
    },
  );
}

/**
 * Einen bereits eingeplanten Post stoppen (Veto-Fenster).
 *
 * `cancelled` zaehlt die gestoppten Plattformen, `already_running` die, bei
 * denen der Upload schon lief. Die zweite Zahl ist der ehrliche Teil: dort
 * kommt das Veto zu spaet.
 */
export async function cancelScheduledPost(
  clipDbId: number,
): Promise<{ cancelled: number; already_running: number }> {
  return fetchJson<{ cancelled: number; already_running: number }>(
    `/twitch/social-media/api/approval/${clipDbId}/cancel`,
    { method: 'POST' },
  );
}

/** Antwort von `POST /twitch/social-media/api/clips/{id}/clip-contest`. */
export interface ClipContestResult {
  /** `accepted`, `already_in`, `rejected`, `rate_limited`, `broker_unavailable` … */
  status: string;
  /** Fertiger deutscher Satz aus dem Backend, gleich wie die Chat-Antwort. */
  message: string;
  clip_url: string | null;
  ok: boolean;
}

/**
 * Clip für den wöchentlichen Clip-Contest im Discord einreichen. Derselbe
 * Dienst wie der Chat-Befehl `!clipcontest`; auch Ablehnungen kommen als
 * lesbarer Satz zurück, deshalb hier ohne `fetchJson`.
 */
export async function submitClipToContest(clipDbId: number): Promise<ClipContestResult> {
  const auth = await fetchAuthStatus();
  const csrf = auth.csrfToken ?? auth.csrf_token;
  const response = await fetch(
    `/twitch/social-media/api/clips/${clipDbId}/clip-contest`,
    withCookieCredentials({
      method: 'POST',
      headers: csrf ? { 'X-CSRF-Token': csrf } : {},
    }),
  );
  let data: Partial<ClipContestResult> = {};
  try {
    data = (await response.json()) as Partial<ClipContestResult>;
  } catch {
    // leerer oder kaputter Body: unten generische Meldung
  }
  return {
    status: String(data.status ?? `http_${response.status}`),
    message: String(data.message ?? 'Das klappt gerade nicht. Versuch es gleich nochmal.'),
    clip_url: typeof data.clip_url === 'string' ? data.clip_url : null,
    ok: response.ok,
  };
}

/**
 * Neue Twitch-Clips einsammeln. Der Weg, den die Vorratswarnung meint: ohne
 * Nachschub hoert das Posting irgendwann auf, ohne dass jemand es merkt.
 */
export async function fetchTwitchClips(
  twitchUserId: string | undefined,
  limit = 20,
): Promise<{ success: boolean; clips_found: number; message?: string }> {
  return fetchJson<{ success: boolean; clips_found: number; message?: string }>(
    '/twitch/social-media/api/fetch-clips',
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ twitch_user_id: twitchUserId, limit }),
    },
  );
}

/**
 * Zeitplan eines Kanals. Alle vier Aufrufe liefern den kompletten Plan zurueck,
 * damit die Oberflaeche nach jeder Aenderung den neu berechneten Termin und die
 * neue Vorratsrechnung sieht, ohne zweite Abfrage.
 */
export async function fetchPostingPlan(twitchUserId: string | undefined): Promise<PostingPlan> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson<PostingPlan>(`${ADMIN_PREFIX}/settings/posting-plan${qs}`);
}

export async function savePostingPlanSettings(
  twitchUserId: string | undefined,
  payload: { approval_mode?: ApprovalMode; timezone?: string; subtitles_enabled?: boolean },
): Promise<PostingPlan> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson<PostingPlan>(`${ADMIN_PREFIX}/settings/posting-plan${qs}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
}

export async function savePlatformSchedule(
  twitchUserId: string | undefined,
  platform: SocialPlatform,
  payload: Partial<Omit<PlatformScheduleEntry, 'platform' | 'next_slot'>>,
): Promise<PostingPlan> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson<PostingPlan>(
    `${ADMIN_PREFIX}/settings/posting-plan/platform/${encodeURIComponent(platform)}${qs}`,
    {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    },
  );
}

export async function saveCategoryAutoPost(
  twitchUserId: string | undefined,
  categoryKey: string,
  autoPost: boolean,
): Promise<PostingPlan> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson<PostingPlan>(
    `${ADMIN_PREFIX}/settings/posting-plan/category/${encodeURIComponent(categoryKey)}${qs}`,
    {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ auto_post: autoPost }),
    },
  );
}

/** Das VOD-Archiv haengt am Streamer, nicht am Dashboard: immer mit Kanal. */
export async function fetchVodArchiveSettings(
  twitchUserId: string | undefined,
): Promise<VodArchiveSettings> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson<VodArchiveSettings>(`${ADMIN_PREFIX}/settings/vod-archive${qs}`);
}

export async function saveVodArchiveSettings(
  twitchUserId: string | undefined,
  payload: Pick<VodArchiveSettings, 'enabled' | 'privacy'>,
): Promise<VodArchiveSettings> {
  return fetchJson<VodArchiveSettings>(`${ADMIN_PREFIX}/settings/vod-archive`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ twitch_user_id: twitchUserId, ...payload }),
  });
}

export interface VocabListParams {
  category?: VocabEntry['category'];
  q?: string;
  page?: number;
  page_size?: number;
}

export async function fetchVocab(params: VocabListParams = {}): Promise<VocabListResponse> {
  const qs = buildQuery({
    category: params.category,
    q: params.q,
    page: params.page,
    page_size: params.page_size,
  });
  return fetchJson<VocabListResponse>(`${ADMIN_PREFIX}/vocab${qs}`);
}

export async function upsertVocab(entry: Partial<VocabEntry> & { term: string; canonical: string; category: VocabEntry['category'] }): Promise<VocabEntry> {
  return fetchJson<VocabEntry>(`${ADMIN_PREFIX}/vocab`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(entry),
  });
}

export async function deleteVocab(term: string): Promise<void> {
  await fetchJson<void>(`${ADMIN_PREFIX}/vocab/${encodeURIComponent(term)}`, {
    method: 'DELETE',
  });
}

export async function seedVocab(): Promise<{ inserted: number; updated: number }> {
  return fetchJson<{ inserted: number; updated: number }>(`${ADMIN_PREFIX}/vocab/seed`, {
    method: 'POST',
  });
}

export async function uploadClip(input: {
  file: File;
  twitch_user_id?: string;
  title?: string;
  clip_id?: string;
}): Promise<UploadResponse> {
  const form = new FormData();
  form.append('file', input.file);
  if (input.twitch_user_id) form.append('twitch_user_id', input.twitch_user_id);
  if (input.title) form.append('title', input.title);
  if (input.clip_id) form.append('clip_id', input.clip_id);

  const response = await fetch(
    UPLOAD_PATH,
    withCookieCredentials({ method: 'POST', body: form }),
  );
  if (response.status === 403 || response.status === 401) {
    throw new SocialMediaForbiddenError();
  }
  if (response.status === 413) throw new SocialMediaApiError('upload_too_large', undefined, 413);
  if (response.status === 415) throw new SocialMediaApiError('upload_wrong_format', undefined, 415);
  if (response.status === 409) throw new SocialMediaApiError('upload_duplicate', undefined, 409);
  if (!response.ok) {
    let code = 'upload_failed';
    let message: string | undefined;
    try {
      const data = await response.json();
      if (data?.error) code = String(data.error);
      if (data?.message) message = String(data.message);
    } catch {
      // ignore
    }
    throw new SocialMediaApiError(code, message, response.status);
  }
  return (await response.json()) as UploadResponse;
}

export interface PlatformCapabilities {
  upload: boolean;
  statistics: boolean;
  upload_mode: string;
  reason: string | null;
}

export interface PlatformStatus {
  capabilities: PlatformCapabilities;
  platform: string;
  connected: boolean;
  username: string | null;
  user_id?: string | null;
  expired: boolean;
  expires_at?: string | null;
  refresh_expires_at?: string | null;
  needs_reauth?: boolean;
  reauth_soon?: boolean;
  automatically_renewed?: boolean;
  /**
   * Der Kanal haengt an der Sammelverbindung statt an einem eigenen Zugang.
   * Wichtig, weil ein Trennen dann alle Kanaele treffen wuerde.
   */
  uses_global_fallback?: boolean;
}

/**
 * Verbindungsstatus je Plattform. Der OAuth-Flow selbst ist ein Redirect und
 * laeuft deshalb nicht ueber fetch, sondern ueber `oauthStartUrl`.
 *
 * `streamer` ist Pflicht: ohne den Kanal antwortet das Backend mit der
 * globalen Sammelverbindung, und die Karte behauptet nach einem erfolgreichen
 * OAuth fuer Kanal X weiter "nicht verbunden".
 */
export async function fetchPlatformStatus(
  twitchUserId: string | undefined,
): Promise<{ platforms: PlatformStatus[] }> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson<{ platforms: PlatformStatus[] }>(`/twitch/social-media/api/platforms/status${qs}`);
}

export function oauthStartUrl(platform: string, twitchUserId: string | undefined): string {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return `/twitch/social-media/oauth/start/${platform}${qs}`;
}

/**
 * Zugang eines Kanals kappen. `streamer` ist Pflicht, sonst trifft es die
 * Sammelverbindung und damit jeden Kanal.
 */
export async function disconnectPlatform(platform: string, twitchUserId: string | undefined): Promise<{ success: boolean; revocation_pending: boolean }> {
  const qs = buildQuery({ twitch_user_id: twitchUserId });
  return fetchJson(`/twitch/social-media/oauth/disconnect/${platform}${qs}`, { method: 'POST' });
}
