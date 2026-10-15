import { useQuery } from '@tanstack/react-query';
import { fetchApi } from '@/api/core';
import { useAuthStatus } from '@/hooks/useAnalytics';

export function useAiFeatures(streamer?: string | null): boolean {
  const { data: auth } = useAuthStatus();
  const { data } = useQuery({
    queryKey: ['ai-capabilities', auth?.twitchUserId, streamer ?? null],
    queryFn: () => fetchApi<{ aiFeatures: boolean }>('/ai/capabilities', streamer ? { streamer } : {}),
    enabled: auth?.authenticated === true,
    staleTime: 0,
  });
  return auth?.authenticated === true && data?.aiFeatures === true;
}
