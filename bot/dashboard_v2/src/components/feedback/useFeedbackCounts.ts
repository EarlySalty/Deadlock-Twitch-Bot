import { useQuery } from '@tanstack/react-query';
import { useAuthStatus } from '../../hooks/useAnalytics';
import { fetchFeedbackCounts } from '../../api/feedback';

export function useFeedbackIdentity() {
  const auth = useAuthStatus();
  const data = auth.data;
  // Auth-A ergänzt twitchUserId zentral. Keine Identität aus dem Anzeigenamen bilden.
  const userId = data && 'twitchUserId' in data && typeof data.twitchUserId === 'string' ? data.twitchUserId : '';
  return { userId, isAdmin: !!data?.isAdmin, authenticated: !!data?.authenticated, loading: auth.isLoading,
    key: `${userId || 'ohne-twitch-id'}:${data?.isAdmin ? 'admin' : 'partner'}`,
    csrfToken: data?.csrfToken ?? data?.csrf_token };
}
export function useFeedbackCounts(isAdmin = false) {
  const identity = useFeedbackIdentity();
  return useQuery({
    queryKey: ['feedback', identity.key, 'counts', isAdmin],
    queryFn: () => fetchFeedbackCounts(isAdmin),
    enabled: identity.authenticated && (isAdmin ? identity.isAdmin : !!identity.userId),
    staleTime: 30_000, refetchInterval: 60_000, refetchOnWindowFocus: true, retry: 1,
  });
}
