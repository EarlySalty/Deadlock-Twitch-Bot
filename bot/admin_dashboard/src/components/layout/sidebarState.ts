export interface SidebarGroupState {
  [groupLabel: string]: boolean;
}

const STORAGE_PREFIX = 'twitch-admin:sidebar-groups:v1';

export function sidebarStorageKey(userKey: string): string {
  return `${STORAGE_PREFIX}:${encodeURIComponent(userKey)}`;
}

export function defaultSidebarGroupState(groupLabels: readonly string[]): SidebarGroupState {
  return Object.fromEntries(groupLabels.map((label) => [label, true]));
}

export function readSidebarGroupState(
  storage: () => Pick<Storage, 'getItem'>,
  key: string | undefined,
  groupLabels: readonly string[],
): SidebarGroupState {
  const defaults = defaultSidebarGroupState(groupLabels);

  try {
    if (!key) {
      return defaults;
    }
    const raw = storage().getItem(key);
    if (!raw) {
      return defaults;
    }

    const parsed = JSON.parse(raw) as Record<string, unknown>;
    return Object.fromEntries(
      groupLabels.map((label) => [
        label,
        typeof parsed[label] === 'boolean' ? parsed[label] : defaults[label],
      ]),
    );
  } catch {
    return defaults;
  }
}

export function writeSidebarGroupState(
  storage: () => Pick<Storage, 'setItem'>,
  key: string | undefined,
  state: SidebarGroupState,
): void {
  try {
    if (key) {
      storage().setItem(key, JSON.stringify(state));
    }
  } catch {
    // Navigation bleibt auch verfügbar, wenn der Browser Storage blockiert.
  }
}
