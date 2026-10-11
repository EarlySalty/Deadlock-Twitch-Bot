import { fetchClips, type SocialClipMitPosting } from '@/api/socialMedia';

const PAGE_SIZE = 100;
const MAX_PAGES = 20;

export type ChannelClipLookup =
  | { kind: 'found'; clip: SocialClipMitPosting }
  | { kind: 'not_found' }
  | { kind: 'limit_reached' };

export function isSelectableClip(clip: SocialClipMitPosting | null | undefined): clip is SocialClipMitPosting {
  return !!clip && clip.source_kind === 'twitch' && Number.isSafeInteger(clip.clip_db_id) && clip.clip_db_id > 0 && typeof clip.title === 'string' && typeof clip.created_at === 'string' && Number.isFinite(Date.parse(clip.created_at));
}

export async function findChannelClip(channel: string, id: number, signal?: AbortSignal): Promise<ChannelClipLookup> {
  if (!channel.trim() || !Number.isSafeInteger(id) || id < 1) return { kind: 'not_found' };
  for (let page = 1; page <= MAX_PAGES; page += 1) {
    const result = await fetchClips({ twitch_user_id: channel, page, page_size: PAGE_SIZE }, signal);
    if (!Array.isArray(result.items) || !Number.isSafeInteger(result.total) || result.total < 0 || result.page !== page || result.page_size !== PAGE_SIZE || result.items.length > PAGE_SIZE || result.items.length > Math.max(0, result.total - (page - 1) * PAGE_SIZE)) {
      throw new Error('Der Clipbestand konnte nicht geprüft werden.');
    }
    const match = result.items.find((clip) => clip?.clip_db_id === id);
    if (match) return isSelectableClip(match) ? { kind: 'found', clip: match } : { kind: 'not_found' };
    if (page * PAGE_SIZE >= result.total) return { kind: 'not_found' };
    if (result.items.length === 0) return { kind: 'limit_reached' };
  }
  return { kind: 'limit_reached' };
}
