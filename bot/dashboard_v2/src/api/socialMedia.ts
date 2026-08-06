import { withCookieCredentials } from './core';
import type {
  AutoApproveSettings,
  ClipAnalyticsResponse,
  ClipApprovalRecord,
  ClipEnrichment,
  ClipListResponse,
  ClipStatus,
  LayoutPayload,
  SocialClip,
  SocialMediaReport,
  SocialMediaReportKind,
  SocialPlatform,
  StreamerLayoutResponse,
  UploadResponse,
  VocabEntry,
  VocabListResponse,
} from '@/types/socialMedia';

const ADMIN_PREFIX = '/social-media/api/admin';
const UPLOAD_PATH = '/social-media/api/clips/upload';

export class SocialMediaForbiddenError extends Error {
  constructor(message: string = 'Admin-Zugriff erforderlich.') {
    super(message);
    this.name = 'SocialMediaForbiddenError';
  }
}

async function fetchJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, withCookieCredentials(init));
  if (response.status === 403 || response.status === 401) {
    throw new SocialMediaForbiddenError();
  }
  if (!response.ok) {
    let message = `Request failed: ${response.status}`;
    try {
      const data = await response.json();
      if (data?.message) message = String(data.message);
      else if (data?.error) message = String(data.error);
    } catch {
      // ignore JSON parse errors
    }
    throw new Error(message);
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

export async function fetchStreamerLayout(streamerLogin: string): Promise<StreamerLayoutResponse> {
  const qs = buildQuery({ streamer_login: streamerLogin });
  return fetchJson<StreamerLayoutResponse>(`${ADMIN_PREFIX}/streamer-layout${qs}`);
}

export async function saveStreamerLayout(input: {
  streamer_login: string;
  layout: LayoutPayload;
}): Promise<StreamerLayoutResponse> {
  const { layout } = input;
  return fetchJson<StreamerLayoutResponse>(`${ADMIN_PREFIX}/streamer-layout`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      streamer_login: input.streamer_login,
      layout,
      cam_enabled: layout.cam_enabled,
      mode: layout.mode,
    }),
  });
}

export interface ClipListParams {
  status?: ClipStatus | 'all';
  streamer?: string;
  page?: number;
  page_size?: number;
}

export async function fetchClips(params: ClipListParams = {}): Promise<ClipListResponse> {
  const qs = buildQuery({
    status: params.status && params.status !== 'all' ? params.status : undefined,
    streamer: params.streamer,
    page: params.page,
    page_size: params.page_size,
  });
  return fetchJson<ClipListResponse>(`${ADMIN_PREFIX}/clips${qs}`);
}

export async function fetchClip(clipDbId: number): Promise<SocialClip> {
  return fetchJson<SocialClip>(`${ADMIN_PREFIX}/clips/${clipDbId}`);
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
  streamer?: string;
  kind?: SocialMediaReportKind;
  limit?: number;
}

export async function fetchReports(params: ReportListParams = {}): Promise<{ items: SocialMediaReport[] }> {
  const qs = buildQuery({
    streamer: params.streamer,
    kind: params.kind,
    limit: params.limit,
  });
  return fetchJson<{ items: SocialMediaReport[] }>(`${ADMIN_PREFIX}/reports${qs}`);
}

export async function runReport(input: {
  kind: Extract<SocialMediaReportKind, 'streamer' | 'cross'>;
  streamer?: string;
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

export async function decideClipApproval(input: {
  clipDbId: number;
  decision: 'approve' | 'skip' | 'edit';
  platforms: SocialPlatform[];
}): Promise<{ clip_db_id: number; approval: ClipApprovalRecord | null; clip: SocialClip | null }> {
  return fetchJson<{ clip_db_id: number; approval: ClipApprovalRecord | null; clip: SocialClip | null }>(
    `${ADMIN_PREFIX}/approval/${input.clipDbId}/decision`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        decision: input.decision,
        platforms: input.platforms,
      }),
    },
  );
}

export async function fetchAutoApproveSettings(): Promise<AutoApproveSettings> {
  return fetchJson<AutoApproveSettings>(`${ADMIN_PREFIX}/settings/auto-approve`);
}

export async function saveAutoApproveSettings(
  payload: AutoApproveSettings,
): Promise<AutoApproveSettings> {
  return fetchJson<AutoApproveSettings>(`${ADMIN_PREFIX}/settings/auto-approve`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
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
  streamer_login: string;
  title?: string;
  clip_id?: string;
}): Promise<UploadResponse> {
  const form = new FormData();
  form.append('file', input.file);
  form.append('streamer_login', input.streamer_login);
  if (input.title) form.append('title', input.title);
  if (input.clip_id) form.append('clip_id', input.clip_id);

  const response = await fetch(
    UPLOAD_PATH,
    withCookieCredentials({ method: 'POST', body: form }),
  );
  if (response.status === 403 || response.status === 401) {
    throw new SocialMediaForbiddenError();
  }
  if (response.status === 413) throw new Error('Datei zu groß (Max 200 MB).');
  if (response.status === 415) throw new Error('Falsches Dateiformat.');
  if (response.status === 409) throw new Error('Diese clip_id existiert bereits.');
  if (!response.ok) {
    let message = `Upload fehlgeschlagen: ${response.status}`;
    try {
      const data = await response.json();
      if (data?.message) message = String(data.message);
      else if (data?.error) message = String(data.error);
    } catch {
      // ignore
    }
    throw new Error(message);
  }
  return (await response.json()) as UploadResponse;
}
