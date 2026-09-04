import { useEffect, useMemo } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { InternalHomeData } from '@/api/home';
import { fetchInternalHome } from '@/api/home';
import { setAdminMode } from '@/api/auth';
import { useAuthStatus } from '@/hooks/useAnalytics';

const DASHBOARD_PROFILE_CACHE_KEY = 'ddc.dashboard.profile';

type ProfileCacheStorage = Pick<Storage, 'getItem' | 'setItem'>;

export interface CachedDashboardProfile {
  identityKey: string;
  displayName: string | null;
  avatarUrl: string | null;
  planName: string | null;
  twitchLogin: string | null;
}

export function readCachedDashboardProfile(
  identityKey: string | null,
  storage: ProfileCacheStorage | null | undefined,
): CachedDashboardProfile | null {
  if (!identityKey || !storage) return null;
  let raw: string | null = null;
  try {
    raw = storage.getItem(DASHBOARD_PROFILE_CACHE_KEY);
  } catch {
    return null;
  }
  if (!raw) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== 'object') return null;
  const record = parsed as Record<string, unknown>;
  if (record.identityKey !== identityKey) return null;
  const asText = (value: unknown): string | null =>
    typeof value === 'string' && value.length > 0 ? value : null;
  return {
    identityKey,
    displayName: asText(record.displayName),
    avatarUrl: asText(record.avatarUrl),
    planName: asText(record.planName),
    twitchLogin: asText(record.twitchLogin),
  };
}

export function writeCachedDashboardProfile(
  value: CachedDashboardProfile,
  storage: ProfileCacheStorage | null | undefined,
): void {
  if (!value.identityKey || !storage) return;
  try {
    storage.setItem(DASHBOARD_PROFILE_CACHE_KEY, JSON.stringify(value));
  } catch {
    return;
  }
}

function dashboardProfileStorage(): ProfileCacheStorage | null {
  try {
    if (typeof window === 'undefined') return null;
    return window.sessionStorage;
  } catch {
    return null;
  }
}

export function useDashboardProfile() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const queryClient = useQueryClient();

  const ownLogin = authStatus?.twitchLogin?.trim() || '';
  const isAuthenticated = authStatus?.authenticated === true;
  const isLocalhostAdmin = Boolean(authStatus?.isLocalhost);
  const isAdminWithoutOwnLogin = Boolean(authStatus?.isAdmin) && !ownLogin;
  const canRequestInternalHome =
    isAuthenticated && !loadingAuth && !isLocalhostAdmin && !isAdminWithoutOwnLogin;

  const identityKey = canRequestInternalHome && ownLogin ? ownLogin : null;

  const cachedProfile = useMemo(
    () => readCachedDashboardProfile(identityKey, dashboardProfileStorage()),
    [identityKey],
  );

  const placeholderProfile = useMemo<InternalHomeData | undefined>(
    () =>
      cachedProfile
        ? {
            twitchLogin: cachedProfile.twitchLogin,
            displayName: cachedProfile.displayName,
            avatarUrl: cachedProfile.avatarUrl,
          }
        : undefined,
    [cachedProfile],
  );

  const {
    data: profile,
    isLoading: loadingProfile,
    isPlaceholderData,
  } = useQuery({
    queryKey: ['internal-home', null],
    queryFn: () => fetchInternalHome(null),
    staleTime: 5 * 60 * 1000,
    enabled: canRequestInternalHome,
    placeholderData: placeholderProfile,
  });

  const planNameFromAuth = authStatus?.plan?.planName || null;

  useEffect(() => {
    if (!identityKey || isPlaceholderData || !profile) return;
    writeCachedDashboardProfile(
      {
        identityKey,
        displayName: profile.displayName?.trim() || null,
        avatarUrl: profile.avatarUrl?.trim() || null,
        planName: planNameFromAuth,
        twitchLogin: profile.twitchLogin?.trim() || ownLogin || null,
      },
      dashboardProfileStorage(),
    );
  }, [identityKey, isPlaceholderData, profile, planNameFromAuth, ownLogin]);

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

  const twitchLogin = profile?.twitchLogin?.trim() || cachedProfile?.twitchLogin || ownLogin;
  const displayName =
    profile?.displayName?.trim() ||
    cachedProfile?.displayName ||
    twitchLogin ||
    (canRequestInternalHome ? 'Creator' : 'Admin');
  const avatarUrl = profile?.avatarUrl?.trim() || cachedProfile?.avatarUrl || null;
  const planName = planNameFromAuth || cachedProfile?.planName || 'Free';
  const adminEligible = Boolean(authStatus?.adminEligible);
  const adminMode = Boolean(authStatus?.adminMode);
  const canAccessAnalyticsDashboard = Boolean(
    authStatus?.canAccessAnalyticsDashboard ?? authStatus?.access?.analytics ?? true
  );

  const profileReady =
    !loadingAuth && (Boolean(profile) || Boolean(cachedProfile) || !canRequestInternalHome);

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
    profileReady,
  };
}
