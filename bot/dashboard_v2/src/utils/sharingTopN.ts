export const SHARING_TOPN_OPTIONS = [3, 5, 10, 20] as const;

export type SharingTopN = (typeof SHARING_TOPN_OPTIONS)[number];

export const SHARING_TOPN_DEFAULT: SharingTopN = 3;

export const SHARING_TOPN_STORAGE_KEY = 'ddc.sharingTimelineTopN';

export function sanitizeSharingTopN(raw: unknown): SharingTopN {
  const value = typeof raw === 'string' ? Number(raw) : raw;
  return SHARING_TOPN_OPTIONS.includes(value as SharingTopN)
    ? (value as SharingTopN)
    : SHARING_TOPN_DEFAULT;
}

export function readSharingTopN(storage?: Pick<Storage, 'getItem'>): SharingTopN {
  const store =
    storage ?? (typeof localStorage !== 'undefined' ? localStorage : undefined);
  if (!store) return SHARING_TOPN_DEFAULT;
  try {
    return sanitizeSharingTopN(store.getItem(SHARING_TOPN_STORAGE_KEY));
  } catch {
    return SHARING_TOPN_DEFAULT;
  }
}

export function writeSharingTopN(
  value: SharingTopN,
  storage?: Pick<Storage, 'setItem'>,
): void {
  const store =
    storage ?? (typeof localStorage !== 'undefined' ? localStorage : undefined);
  if (!store) return;
  try {
    store.setItem(SHARING_TOPN_STORAGE_KEY, String(value));
  } catch {
    void 0;
  }
}
