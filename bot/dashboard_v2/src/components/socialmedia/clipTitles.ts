import type { SocialClipMitPosting } from '@/api/socialMedia';
import { formatTerminInZone } from './queuePresentation';

export function clipDisplayTitles(clips: SocialClipMitPosting[], locale: string, timezone: string): Map<number, string> {
  const counts = new Map<string, number>();
  for (const clip of clips) counts.set(clip.title, (counts.get(clip.title) ?? 0) + 1);
  return new Map(clips.map(clip => [clip.clip_db_id, clip.generated_title?.trim() || ((counts.get(clip.title) ?? 0) > 1 ? `${clip.title} · ${formatTerminInZone(clip.created_at, locale, timezone)}` : clip.title)]));
}
