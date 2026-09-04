import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { fetchInternalHome } from '@/api/home';
import { setAdminMode } from '@/api/auth';
import { useAuthStatus } from '@/hooks/useAnalytics';

export function useDashboardProfile() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const queryClient = useQueryClient();

  const ownLogin = authStatus?.twitchLogin?.trim() || '';
  const isAuthenticated = authStatus?.authenticated === true;
  const isLocalhostAdmin = Boolean(authStatus?.isLocalhost);
  const isAdminWithoutOwnLogin = Boolean(authStatus?.isAdmin) && !ownLogin;
  const canRequestInternalHome =
    isAuthenticated && !loadingAuth && !isLocalhostAdmin && !isAdminWithoutOwnLogin;

  const { data: profile, isLoading: loadingProfile } = useQuery({
    queryKey: ['internal-home', null],
    queryFn: () => fetchInternalHome(null),
    staleTime: 5 * 60 * 1000,
    enabled: canRequestInternalHome,
  });

  const adminModeMutation = useMutation({
    mutationFn: async (enabled: boolean) => {
      await queryClient.cancelQueries({ queryKey: ['internal-home'] });
      const result = await setAdminMode(
        enabled,
        authStatus?.csrfToken ?? authStatus?.csrf_token ?? null
      );
      await queryClient.refetchQueries(
        { queryKey: ['auth-status'], exact: true, type: 'active' },
        { throwOnError: true }
      );
      return result;
    },
  });

  const twitchLogin = profile?.twitchLogin?.trim() || ownLogin;
  const displayName =
    profile?.displayName?.trim() || twitchLogin || (canRequestInternalHome ? 'Creator' : 'Admin');
  const avatarUrl = profile?.avatarUrl?.trim() || null;
  const planName = authStatus?.plan?.planName || 'Free';
  const adminEligible = Boolean(authStatus?.adminEligible);
  const adminMode = Boolean(authStatus?.adminMode);
  const canAccessAnalyticsDashboard = Boolean(
    authStatus?.canAccessAnalyticsDashboard ?? authStatus?.access?.analytics ?? true
  );

  return {
    authStatus,
    loadingAuth,
    loadingProfile,
    displayName,
    avatarUrl,
    planName,
    adminEligible,
    adminMode,
    adminModeMutation,
    canAccessAnalyticsDashboard,
  };
}
