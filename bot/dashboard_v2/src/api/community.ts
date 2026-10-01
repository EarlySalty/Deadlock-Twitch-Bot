import { fetchApi } from './core';
import type { CommunityData } from '../types/community';

export function fetchCommunity(streamer: string, days: number): Promise<CommunityData> {
  return fetchApi<CommunityData>('/community', { streamer, days: Math.min(90, Math.max(7, days)) });
}
