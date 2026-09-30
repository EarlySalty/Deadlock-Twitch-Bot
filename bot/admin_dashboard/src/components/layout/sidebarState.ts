export interface SidebarGroupState {
  [groupLabel: string]: boolean;
}

const STORAGE_PREFIX = 'twitch-admin:sidebar-groups:v1';

/** Only a server-provided account identity may select persistent preferences. */
export function sidebarStorageKey(userKey: string | null | undefined): string | null {
  const identity = userKey?.trim();
  return identity ? `${STORAGE_PREFIX}:${encodeURIComponent(identity)}` : null;
}

export function defaultSidebarGroupState(groupLabels: readonly string[]): SidebarGroupState {
  return Object.fromEntries(groupLabels.map((label) => [label, true]));
}

export function readSidebarGroupState(
  getStorage: () => Pick<Storage, 'getItem'>,
  key: string | null,
  groupLabels: readonly string[],
): SidebarGroupState {
  const defaults = defaultSidebarGroupState(groupLabels);
  if (!key) return defaults;

  try {
    // The window.localStorage getter itself may throw a SecurityError.
    const raw = getStorage().getItem(key);
    if (!raw) return defaults;

    const parsed: unknown = JSON.parse(raw);
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return defaults;
    const stored = parsed as Record<string, unknown>;
    return Object.fromEntries(
      groupLabels.map((label) => [
        label,
        typeof stored[label] === 'boolean' ? stored[label] : defaults[label],
      ]),
    );
  } catch {
    return defaults;
  }
}

export function writeSidebarGroupState(
  getStorage: () => Pick<Storage, 'setItem'>,
  key: string | null,
  state: SidebarGroupState,
): void {
  if (!key) return;
  try {
    getStorage().setItem(key, JSON.stringify(state));
  } catch {
    // Navigation also works with storage denied or a full storage quota.
  }
}
