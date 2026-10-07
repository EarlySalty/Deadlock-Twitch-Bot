import { useEffect, useState, type KeyboardEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertCircle,
  CheckCircle2,
  Hash,
  Loader2,
  Camera,
  Save,
  X,
  Video,
  Music2,
  Pencil,
} from 'lucide-react';
import {
  fetchClipEnrichment,
  saveClipEnrichment,
  type EnrichmentEditPayload,
} from '@/api/socialMedia';
import type { ClipEnrichment, SocialPlatform } from '@/types/socialMedia';
import { useT } from '@/context/LanguageContext';

const PLATFORMS: Array<{
  id: SocialPlatform;
  label: string;
  Icon: React.ComponentType<{ className?: string }>;
  tone: string;
  titleLimit: number;
  hashtagTarget: string;
}> = [
  { id: 'youtube', label: 'YouTube Shorts', Icon: Video, tone: 'text-[#FF5A3C]', titleLimit: 100, hashtagTarget: '5–10' },
  { id: 'tiktok', label: 'TikTok', Icon: Music2, tone: 'text-[#00D9FF]', titleLimit: 150, hashtagTarget: '8–12' },
  { id: 'instagram', label: 'Instagram Reels', Icon: Camera, tone: 'text-[#FF5A3C]', titleLimit: 125, hashtagTarget: '8–15' },
];

interface EnrichmentPanelProps {
  clipDbId: number;
  onClose?: () => void;
}

interface EditState {
  title_youtube: string;
  title_tiktok: string;
  title_instagram: string;
  description_youtube: string;
  description_tiktok: string;
  description_instagram: string;
  hashtags_youtube: string[];
  hashtags_tiktok: string[];
  hashtags_instagram: string[];
}

function normalizeHashtag(tag: string): string {
  const cleaned = tag.trim().replace(/^#+/, '').replace(/\s+/g, '').toLowerCase();
  return cleaned ? `#${cleaned}` : '';
}

function fromEnrichment(e: ClipEnrichment): EditState {
  return {
    title_youtube: e.title_youtube ?? '',
    title_tiktok: e.title_tiktok ?? '',
    title_instagram: e.title_instagram ?? '',
    description_youtube: e.description_youtube ?? '',
    description_tiktok: e.description_tiktok ?? '',
    description_instagram: e.description_instagram ?? '',
    hashtags_youtube: e.hashtags_youtube ?? [],
    hashtags_tiktok: e.hashtags_tiktok ?? [],
    hashtags_instagram: e.hashtags_instagram ?? [],
  };
}

function toPayload(initial: ClipEnrichment, edit: EditState): EnrichmentEditPayload {
  const payload: EnrichmentEditPayload = {};
  if (edit.title_youtube !== (initial.title_youtube ?? '')) payload.title_youtube = edit.title_youtube || null;
  if (edit.title_tiktok !== (initial.title_tiktok ?? '')) payload.title_tiktok = edit.title_tiktok || null;
  if (edit.title_instagram !== (initial.title_instagram ?? '')) payload.title_instagram = edit.title_instagram || null;
  if (edit.description_youtube !== (initial.description_youtube ?? '')) payload.description_youtube = edit.description_youtube || null;
  if (edit.description_tiktok !== (initial.description_tiktok ?? '')) payload.description_tiktok = edit.description_tiktok || null;
  if (edit.description_instagram !== (initial.description_instagram ?? '')) payload.description_instagram = edit.description_instagram || null;
  if (JSON.stringify(edit.hashtags_youtube) !== JSON.stringify(initial.hashtags_youtube ?? [])) payload.hashtags_youtube = edit.hashtags_youtube;
  if (JSON.stringify(edit.hashtags_tiktok) !== JSON.stringify(initial.hashtags_tiktok ?? [])) payload.hashtags_tiktok = edit.hashtags_tiktok;
  if (JSON.stringify(edit.hashtags_instagram) !== JSON.stringify(initial.hashtags_instagram ?? [])) payload.hashtags_instagram = edit.hashtags_instagram;
  return payload;
}

export function EnrichmentPanel({ clipDbId, onClose }: EnrichmentPanelProps) {
  const t = useT();
  const queryClient = useQueryClient();
  const [edit, setEdit] = useState<EditState | null>(null);
  const [activePlatform, setActivePlatform] = useState<SocialPlatform>('youtube');

  const enrichmentQuery = useQuery({
    queryKey: ['social-media', 'enrichment', clipDbId],
    queryFn: () => fetchClipEnrichment(clipDbId),
  });

  useEffect(() => {
    if (enrichmentQuery.data && !edit) {
      setEdit(fromEnrichment(enrichmentQuery.data));
    }
  }, [enrichmentQuery.data, edit]);

  const saveMutation = useMutation({
    mutationFn: (payload: EnrichmentEditPayload) => saveClipEnrichment(clipDbId, payload),
    onSuccess: (data) => {
      queryClient.setQueryData(['social-media', 'enrichment', clipDbId], data);
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      setEdit(fromEnrichment(data));
    },
  });

  if (enrichmentQuery.isError) {
    return (
      <div role="alert" className="space-y-3 text-sm text-danger">
        <p>{t('Die Cliptexte konnten nicht geladen werden.')}</p>
        <button type="button" onClick={() => enrichmentQuery.refetch()} className="studio-button">
          {t('Erneut laden')}
        </button>
      </div>
    );
  }

  if (enrichmentQuery.isLoading || !edit) {
    return (
      <div className="flex items-center justify-center py-10">
        <Loader2 className="w-5 h-5 text-orange animate-spin" />
      </div>
    );
  }

  const enrichment = enrichmentQuery.data!;
  const dirty = JSON.stringify(toPayload(enrichment, edit)) !== '{}';

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center gap-3 border-b border-border pb-3">
        <div className="flex items-center gap-2">
          <Pencil className="w-4 h-4 text-orange" />
          <h4 className="text-sm font-bold uppercase tracking-[0.16em] text-white">{t('Titel, Beschreibung & Hashtags')}</h4>
        </div>
        <div className="ml-auto flex items-center gap-2">
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="p-1.5 rounded-lg hover:bg-bg/60 text-text-secondary hover:text-white"
              aria-label={t('Editor schließen')}
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      <div className="flex flex-wrap gap-1.5">
        {PLATFORMS.map(({ id, label, Icon, tone }) => {
          const active = activePlatform === id;
          return (
            <button
              key={id}
              type="button"
              onClick={() => setActivePlatform(id)}
              className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold border transition ${
                active
                  ? 'bg-orange/15 text-orange border-orange/40 shadow-[0_4px_18px_-8px_rgba(201, 168, 106, 0.5)]'
                  : 'bg-bg/40 text-text-secondary border-border hover:text-white'
              }`}
            >
              <Icon className={`w-3.5 h-3.5 ${active ? 'text-orange' : tone}`} />
              {t(label)}
            </button>
          );
        })}
      </div>

      <PlatformEditor
        platform={activePlatform}
        edit={edit}
        onChange={setEdit}
        lastHashtags={enrichment.last_hashtags ?? []}
      />

      <div className="flex items-center gap-3 border-t border-border pt-3">
        <div className="text-[11px] text-text-secondary">
          {dirty ? t('Ungesicherte Änderungen') : t('Synchron mit Server')}
        </div>
        <div className="ml-auto flex gap-2">
          <button
            type="button"
            disabled={!dirty || saveMutation.isPending}
            onClick={() => setEdit(fromEnrichment(enrichment))}
            className="px-3 py-2 rounded-xl text-xs font-semibold text-text-secondary border border-border hover:text-white disabled:opacity-40"
          >
            {t('Zurücksetzen')}
          </button>
          <button
            type="button"
            disabled={!dirty || saveMutation.isPending}
            onClick={() => saveMutation.mutate(toPayload(enrichment, edit))}
            className="px-4 py-2 rounded-xl text-xs font-bold inline-flex items-center gap-2 bg-orange text-white shadow-[0_8px_22px_-8px_rgba(201, 168, 106, 0.6)] hover:bg-orange-hover transition disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {saveMutation.isPending ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Save className="w-3.5 h-3.5" />}
            {t('Speichern')}
          </button>
        </div>
      </div>
      {saveMutation.isError && (
        <div role="alert" className="text-sm text-danger inline-flex items-center gap-2">
          <AlertCircle className="w-4 h-4" />
          {t('Speichern hat nicht geklappt. Versuch es nochmal.')}
        </div>
      )}
      {saveMutation.isSuccess && !dirty && (
        <div className="text-xs text-success inline-flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5" /> {t('Gespeichert.')}
        </div>
      )}
    </div>
  );
}

interface PlatformEditorProps {
  platform: SocialPlatform;
  edit: EditState;
  onChange: (next: EditState) => void;
  lastHashtags: string[];
}

function PlatformEditor({ platform, edit, onChange, lastHashtags }: PlatformEditorProps) {
  const t = useT();
  const config = PLATFORMS.find((p) => p.id === platform)!;
  const titleKey = `title_${platform}` as const;
  const descKey = `description_${platform}` as const;
  const tagsKey = `hashtags_${platform}` as const;

  const title = edit[titleKey];
  const desc = edit[descKey];
  const tags = edit[tagsKey];
  const savedTags = [...new Set(lastHashtags.map(normalizeHashtag).filter(Boolean))];
  const containsTag = (tag: string) => tags.some((current) => normalizeHashtag(current) === tag);
  const titleLen = title.length;

  return (
    <div className="space-y-4">
      <div>
        <label className="block text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary mb-1.5">
          {t('Titel')}
          <span className={`ml-2 font-mono ${titleLen > config.titleLimit ? 'text-danger' : 'text-text-secondary'}`}>
            {titleLen}/{config.titleLimit}
          </span>
        </label>
        <input
          type="text"
          value={title}
          onChange={(e) => onChange({ ...edit, [titleKey]: e.target.value })}
          maxLength={config.titleLimit + 20}
          placeholder={t('{platform}-Title…', { platform: config.label })}
          className="w-full px-3 py-2 rounded-xl bg-bg/60 border border-border focus:border-orange/60 focus:outline-none text-sm text-white placeholder:text-text-secondary/60"
        />
      </div>

      <div>
        <label className="block text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary mb-1.5">
          {t('Beschreibung')}
        </label>
        <textarea
          value={desc}
          onChange={(e) => onChange({ ...edit, [descKey]: e.target.value })}
          rows={3}
          placeholder={t('Kurze Beschreibung für {platform}…', { platform: config.label })}
          className="w-full px-3 py-2 rounded-xl bg-bg/60 border border-border focus:border-orange/60 focus:outline-none text-sm text-white placeholder:text-text-secondary/60 resize-y leading-relaxed"
        />
      </div>

      <div>
        <label className="block text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary mb-1.5 inline-flex items-center gap-1.5">
          <Hash className="w-3 h-3" /> {t('Hashtags')}
          <span className="ml-1 font-mono text-text-secondary">
            {t('{count} · Ziel {target}', { count: tags.length, target: config.hashtagTarget })}
          </span>
        </label>
        <HashtagsEditor
          tags={tags}
          onChange={(next) => onChange({ ...edit, [tagsKey]: next })}
        />
        {savedTags.length > 0 && (
          <div className="mt-3 space-y-2">
            <p className="text-xs text-text-secondary">{t('Zuletzt gespeicherte Hashtags')}</p>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="studio-button text-xs"
                onClick={() => onChange({ ...edit, [tagsKey]: [...tags, ...savedTags.filter((tag) => !containsTag(tag))] })}
              >
                {t('Gespeicherte Hashtags einfügen')}
              </button>
              {savedTags.map((tag) => (
                <button
                  key={tag}
                  type="button"
                  className="studio-button text-xs"
                  disabled={containsTag(tag)}
                  onClick={() => onChange({ ...edit, [tagsKey]: [...tags, tag] })}
                >
                  {tag.startsWith('#') ? tag : `#${tag}`}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function HashtagsEditor({
  tags,
  onChange,
}: {
  tags: string[];
  onChange: (next: string[]) => void;
}) {
  const t = useT();
  const [input, setInput] = useState('');

  const addTag = (raw: string) => {
    const tag = normalizeHashtag(raw);
    if (!tag || tags.some((current) => normalizeHashtag(current) === tag)) return;
    onChange([...tags, tag]);
  };

  const removeTag = (tag: string) => {
    onChange(tags.filter((t) => t !== tag));
  };

  const handleKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' || e.key === ',' || e.key === ' ') {
      e.preventDefault();
      addTag(input);
      setInput('');
    } else if (e.key === 'Backspace' && !input && tags.length > 0) {
      removeTag(tags[tags.length - 1]);
    }
  };

  return (
    <div className="rounded-xl border border-border bg-bg/60 p-2 flex flex-wrap items-center gap-1.5">
      {tags.map((tag) => (
        <span
          key={tag}
          className="inline-flex items-center gap-1 text-xs font-semibold px-2 py-1 rounded-md bg-orange/10 text-orange border border-orange/30"
        >
          {tag.startsWith('#') ? tag : `#${tag}`}
          <button
            type="button"
            onClick={() => removeTag(tag)}
            className="hover:text-white"
            aria-label={t('Hashtag #{tag} entfernen', { tag: tag.replace(/^#+/, '') })}
          >
            <X className="w-3 h-3" />
          </button>
        </span>
      ))}
      <input
        type="text"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKey}
        onBlur={() => {
          if (input.trim()) {
            addTag(input);
            setInput('');
          }
        }}
        placeholder={tags.length === 0 ? t('Hashtag eingeben + Enter…') : ''}
        className="flex-1 min-w-[120px] bg-transparent text-sm text-white placeholder:text-text-secondary/60 outline-none px-2 py-1"
      />
    </div>
  );
}
