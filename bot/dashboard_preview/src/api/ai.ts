import { buildApiUrl, fetchApi, withCookieCredentials } from './core';
import type { AIAnalysisResult, AIChatResponse, AIHistoryEntry } from '@/types/analytics';

export class AIChatRateLimitError extends Error {
  retryAfter?: number;
  rateLimitReset?: number;

  constructor(message: string, options: { retryAfter?: number; rateLimitReset?: number } = {}) {
    super(message);
    this.name = 'AIChatRateLimitError';
    this.retryAfter = options.retryAfter;
    this.rateLimitReset = options.rateLimitReset;
  }
}

export async function fetchAIAnalysis(
  streamer: string,
  days: number,
  gameFilter: 'deadlock' | 'all' = 'all',
  userContext?: string
): Promise<AIAnalysisResult> {
  const params: Record<string, string | number | boolean> = { streamer, days, game_filter: gameFilter };
  if (userContext && userContext.trim()) {
    params.user_context = userContext.trim();
  }
  return fetchApi<AIAnalysisResult>('/ai/analysis', params, 240_000);
}

export async function fetchAIHistory(
  streamer: string,
  limit = 20
): Promise<AIHistoryEntry[]> {
  return fetchApi<AIHistoryEntry[]>('/ai/history', { streamer, limit });
}

export async function fetchAIChat(
  streamer: string,
  analysisId: number,
  message: string
): Promise<AIChatResponse> {
  const response = await fetch(
    buildApiUrl('/ai/chat'),
    withCookieCredentials({
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        streamer,
        analysis_id: analysisId,
        message,
      }),
    })
  );

  const payload = await response.json().catch(() => null) as {
    error?: string;
    message?: string;
    retry_after?: number;
    rateLimitReset?: number;
  } | null;

  if (response.status === 429) {
    throw new AIChatRateLimitError(
      payload?.message || payload?.error || 'Rückfragen-Limit erreicht',
      {
        retryAfter: payload?.retry_after,
        rateLimitReset: payload?.rateLimitReset,
      }
    );
  }

  if (!response.ok) {
    throw new Error(payload?.message || payload?.error || `Server-Fehler (HTTP ${response.status})`);
  }

  return payload as AIChatResponse;
}
