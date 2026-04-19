import { buildApiUrl, fetchApi, fetchJson, withCookieCredentials } from './core';

export interface TitleSuggestRequest {
  keywords: string;
  include_live?: boolean;
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
}

export interface TitleInsight {
  strengths: string;
  weaknesses: string;
  patterns: string;
  recommendations: string;
  generated_at: string;
}

export async function fetchTitleSuggestion(
  body: TitleSuggestRequest
): Promise<TitleSuggestResult> {
  const url = buildApiUrl('/title/suggest');
  return fetchJson<TitleSuggestResult>(url, withCookieCredentials({
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(120_000),
  }));
}

export async function fetchTitleInsights(): Promise<{ insight: TitleInsight | null }> {
  return fetchApi<{ insight: TitleInsight | null }>('/title/insights', {});
}
