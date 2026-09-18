import { getPreviewTitleInsights, getPreviewTitleSuggestion } from '../preview/fixtures';
import { isPreviewLocalhost } from '../preview/routes';
import { buildApiUrl, fetchApi, fetchJson, withCookieCredentials } from './core';

export interface TitleSuggestRequest {
  keywords?: string;
  include_live?: boolean;
  streamer?: string | null;
}

export interface TitleHistoryEntry {
  title: string;
  avg_viewers: number;
  peak_viewers: number;
  relative_perf: number;
  engagement_rate: number;
}

export interface TitleSuggestResult {
  primary: string;
  alternatives: string[];
  title_analysis: TitleHistoryEntry[];
  generation_id?: string;
  style_summary?: string;
  live_context_used?: boolean;
  co_streamers?: string[];
  auto_mode?: boolean;
  oauth_connected?: boolean;
  oauth_url?: string;
  experimental_auto_set?: boolean;
  auto_set_status?: 'disabled' | 'set' | 'scope_missing' | 'error';
  generated_by?: 'ai' | 'fallback' | string;
}

export interface TitleInsight {
  strengths: string;
  weaknesses: string;
  patterns: string;
  recommendations: string;
  generated_at: string;
}

export interface TitleSettings {
  style_preference: string;
  experimental_auto_set: boolean;
  never_words: string[];
  style_summary: string;
  oauth_connected: boolean;
  oauth_url: string;
  model: string;
}

export interface TitleFeedbackRequest {
  generation_id: string;
  feedback: 'liked' | 'disliked' | 'selected' | 'edited';
  selected_title?: string;
  edited_title?: string;
  streamer?: string | null;
}

function previewSettings(): TitleSettings {
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

export async function fetchTitleSuggestion(
  body: TitleSuggestRequest,
  csrfToken?: string | null
): Promise<TitleSuggestResult> {
  if (isPreviewLocalhost()) {
    const result = getPreviewTitleSuggestion() as TitleSuggestResult;
    return {
      ...result,
      generation_id: 'preview-title-generation',
      style_summary: previewSettings().style_summary,
      oauth_connected: true,
      auto_mode: !body.keywords?.trim(),
      experimental_auto_set: false,
      auto_set_status: 'disabled',
      generated_by: 'ai',
    };
  }

  const url = buildApiUrl('/title/suggest');
  return fetchJson<TitleSuggestResult>(url, withCookieCredentials({
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(120_000),
  }));
}

export async function fetchTitleInsights(
  streamer?: string | null
): Promise<{ insight: TitleInsight | null }> {
  if (isPreviewLocalhost()) {
    return getPreviewTitleInsights() as { insight: TitleInsight | null };
  }

  return fetchApi<{ insight: TitleInsight | null }>(
    '/title/insights',
    streamer ? { streamer } : {}
  );
}

export async function fetchTitleSettings(streamer?: string | null): Promise<TitleSettings> {
  if (isPreviewLocalhost()) return previewSettings();
  return fetchApi<TitleSettings>('/title/settings', streamer ? { streamer } : {});
}

export async function saveTitleSettings(
  settings: Pick<TitleSettings, 'style_preference' | 'experimental_auto_set' | 'never_words'> & { streamer?: string | null },
  csrfToken?: string | null,
): Promise<TitleSettings & { ok: boolean }> {
  if (isPreviewLocalhost()) return { ...previewSettings(), ...settings, ok: true };
  return fetchJson<TitleSettings & { ok: boolean }>(
    buildApiUrl('/title/settings'),
    withCookieCredentials({
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
      },
      body: JSON.stringify(settings),
    }),
  );
}

export async function sendTitleFeedback(
  feedback: TitleFeedbackRequest,
  csrfToken?: string | null,
): Promise<{ ok: boolean }> {
  if (isPreviewLocalhost()) return { ok: true };
  return fetchJson<{ ok: boolean }>(
    buildApiUrl('/title/feedback'),
    withCookieCredentials({
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
      },
      body: JSON.stringify(feedback),
    }),
  );
}

export async function setTwitchTitle(
  title: string,
  streamer?: string | null,
  generationId?: string | null,
  csrfToken?: string | null,
): Promise<{ ok: boolean; title: string }> {
  if (isPreviewLocalhost()) return { ok: true, title };
  const params = new URLSearchParams();
  if (streamer) params.set('streamer', streamer);
  return fetchJson<{ ok: boolean; title: string }>(
    `/twitch/api/v2/channel/title?${params.toString()}`,
    withCookieCredentials({
      method: 'PATCH',
      headers: {
        'Content-Type': 'application/json',
        ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
      },
      body: JSON.stringify({ title, generation_id: generationId || undefined }),
    }),
  );
}
