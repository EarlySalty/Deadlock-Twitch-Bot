import { fetchApi } from './core';
import type { AIAnalysisResult, AIHistoryEntry } from '@/types/analytics';

export async function fetchAIAnalysis(
  streamer: string,
  days: number,
  gameFilter: 'deadlock' | 'all' = 'all'
): Promise<AIAnalysisResult> {
  return fetchApi<AIAnalysisResult>('/ai/analysis', { streamer, days, game_filter: gameFilter }, 240_000);
}

export async function fetchAIHistory(
  streamer: string,
  limit = 20
): Promise<AIHistoryEntry[]> {
  return fetchApi<AIHistoryEntry[]>('/ai/history', { streamer, limit });
}
