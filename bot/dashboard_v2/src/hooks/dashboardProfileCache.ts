export const DASHBOARD_PROFILE_CACHE_KEY = 'ddc.dashboard.profile';

export type ProfileCacheStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

export interface CachedDashboardProfile {
  identityKey: string;
  displayName: string | null;
  avatarUrl: string | null;
  twitchLogin: string | null;
}

export function readCachedDashboardProfile(
  identityKey: string | null,
  storage: ProfileCacheStorage | null | undefined,
): CachedDashboardProfile | null {
  if (!identityKey || !storage) return null;
  let raw: string | null;
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

export function clearCachedDashboardProfile(
  storage: ProfileCacheStorage | null | undefined,
): void {
  if (!storage) return;
  try {
    storage.removeItem(DASHBOARD_PROFILE_CACHE_KEY);
  } catch {
    return;
  }
}

export interface ProfilBereitInput {
  loadingAuth: boolean;
  loadingProfile: boolean;
  hasProfile: boolean;
  hasCache: boolean;
  canRequest: boolean;
}

export function profilBereit({
  loadingAuth,
  loadingProfile,
  hasProfile,
  hasCache,
  canRequest,
}: ProfilBereitInput): boolean {
  if (loadingAuth) return false;
  return !loadingProfile || hasProfile || hasCache || !canRequest;
}
