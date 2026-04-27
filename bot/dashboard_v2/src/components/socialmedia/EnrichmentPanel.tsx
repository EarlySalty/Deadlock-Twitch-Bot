import { useEffect, useState, type KeyboardEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertCircle,
  CheckCircle2,
  Hash,
  Loader2,
  RefreshCw,
  Save,
  Sparkles,
  X,
  Youtube,
  Music2,
  Instagram,
  ScrollText,
  Wand2,
} from 'lucide-react';
import {
  fetchClipEnrichment,
  runClipEnrichment,
  saveClipEnrichment,
  type EnrichmentEditPayload,
} from '@/api/socialMedia';
import type { ClipEnrichment, EnrichmentStatus, SocialPlatform } from '@/types/socialMedia';

const STATUS_META: Record<EnrichmentStatus, { label: string; tone: 'muted' | 'orange' | 'teal' | 'success' | 'danger' }> = {
  pending: { label: 'Wartet', tone: 'muted' },
  transcribing: { label: 'Transkribiert', tone: 'orange' },
  correcting: { label: 'Wörterbuch-Korrektur', tone: 'orange' },
  llm: { label: 'LLM-Hashtags', tone: 'teal' },
  done: { label: 'Fertig', tone: 'success' },
  failed: { label: 'Fehler', tone: 'danger' },
  skipped_no_key: { label: 'API-Key fehlt', tone: 'muted' },
};

const TONE: Record<string, string> = {
  muted: 'bg-bg/60 text-text-secondary border-border',
  orange: 'bg-orange/15 text-orange border-orange/35',
  teal: 'bg-teal/15 text-teal border-teal/35',
  success: 'bg-success/15 text-success border-success/35',
  danger: 'bg-danger/15 text-danger border-danger/35',
};

const PLATFORMS: Array<{
  id: SocialPlatform;
  label: string;
  Icon: React.ComponentType<{ className?: string }>;
  tone: string;
  titleLimit: number;
  hashtagTarget: string;
}> = [
  { id: 'youtube', label: 'YouTube Shorts', Icon: Youtube, tone: 'text-[#ff5b5b]', titleLimit: 100, hashtagTarget: '5–10' },
  { id: 'tiktok', label: 'TikTok', Icon: Music2, tone: 'text-[#69e1ff]', titleLimit: 150, hashtagTarget: '8–12' },
  { id: 'instagram', label: 'Instagram Reels', Icon: Instagram, tone: 'text-[#ff8acc]', titleLimit: 125, hashtagTarget: '8–15' },
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
  const queryClient = useQueryClient();
  const [edit, setEdit] = useState<EditState | null>(null);
  const [activePlatform, setActivePlatform] = useState<SocialPlatform>('youtube');

  const enrichmentQuery = useQuery({
    queryKey: ['social-media', 'enrichment', clipDbId],
    queryFn: () => fetchClipEnrichment(clipDbId),
    refetchInterval: (q) => {
      const data = q.state.data as ClipEnrichment | undefined;
      if (!data) return 5000;
      if (data.status === 'transcribing' || data.status === 'correcting' || data.status === 'llm') {
        return 4000;
      }
      return false;
    },
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

  const runMutation = useMutation({
    mutationFn: (force: boolean) => runClipEnrichment(clipDbId, force),
    onSuccess: (data) => {
      queryClient.setQueryData(['social-media', 'enrichment', clipDbId], data);
      queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      setEdit(fromEnrichment(data));
    },
  });

  if (enrichmentQuery.isLoading || !edit) {
    return (
      <div className="flex items-center justify-center py-10">
        <Loader2 className="w-5 h-5 text-orange animate-spin" />
      </div>
    );
  }

  const enrichment = enrichmentQuery.data!;
  const status = STATUS_META[enrichment.status] ?? STATUS_META.pending;
  const dirty = JSON.stringify(toPayload(enrichment, edit)) !== '{}';
  const isProcessing =
    enrichment.status === 'transcribing' ||
    enrichment.status === 'correcting' ||
    enrichment.status === 'llm' ||
    runMutation.isPending;

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center gap-3 border-b border-border pb-3">
        <div className="flex items-center gap-2">
          <Wand2 className="w-4 h-4 text-orange" />
          <h4 className="text-sm font-bold uppercase tracking-[0.16em] text-white">Metadaten</h4>
        </div>
        <span className={`text-[10px] font-bold uppercase tracking-[0.14em] px-2 py-1 rounded-md border ${TONE[status.tone]}`}>
          {status.label}
        </span>
        {enrichment.llm_provider && (
          <span className="text-[10px] font-mono text-text-secondary bg-bg/60 px-2 py-1 rounded-md border border-border">
            {enrichment.llm_provider}
            {enrichment.llm_model ? ` · ${enrichment.llm_model}` : ''}
          </span>
        )}
        {typeof enrichment.cost_usd_estimate === 'number' && enrichment.cost_usd_estimate > 0 && (
          <span className="text-[10px] font-mono text-text-secondary">
            ≈ ${enrichment.cost_usd_estimate.toFixed(4)}
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            disabled={isProcessing}
            onClick={() => runMutation.mutate(true)}
            className="text-xs font-semibold text-text-secondary hover:text-white inline-flex items-center gap-1.5 disabled:opacity-40"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${runMutation.isPending ? 'animate-spin' : ''}`} />
            Neu generieren
          </button>
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="p-1.5 rounded-lg hover:bg-bg/60 text-text-secondary hover:text-white"
              aria-label="Enrichment-Panel schließen"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {enrichment.error_message && (
        <div className="flex items-start gap-2 text-xs text-danger bg-danger/10 border border-danger/30 rounded-lg p-2.5">
          <AlertCircle className="w-4 h-4 mt-0.5 flex-shrink-0" />
          <span>{enrichment.error_message}</span>
        </div>
      )}

      {enrichment.status === 'skipped_no_key' && (
        <div className="text-xs text-text-secondary bg-bg/40 border border-border rounded-lg p-3 leading-relaxed">
          Enrichment wurde übersprungen, weil kein LLM-Key gesetzt ist (
          <code className="font-mono text-orange">MINIMAX_API_KEY</code> /
          {' '}
          <code className="font-mono text-orange">ANTHROPIC_API_KEY</code>
          ). Setze einen Key und drücke „Neu generieren".
        </div>
      )}

      {/* Detected terms */}
      {enrichment.detected_terms.length > 0 && (
        <div className="space-y-2">
          <div className="text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary inline-flex items-center gap-1.5">
            <Sparkles className="w-3 h-3 text-teal" /> Erkannte Begriffe
          </div>
          <div className="flex flex-wrap gap-1.5">
            {enrichment.detected_terms.map((term) => (
              <span
                key={term}
                className="text-[11px] font-semibold px-2 py-1 rounded-md bg-teal/10 text-teal border border-teal/30"
              >
                {term}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Platform tabs */}
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
                  ? 'bg-orange/15 text-orange border-orange/40 shadow-[0_4px_18px_-8px_rgba(255,122,24,0.5)]'
                  : 'bg-bg/40 text-text-secondary border-border hover:text-white'
              }`}
            >
              <Icon className={`w-3.5 h-3.5 ${active ? 'text-orange' : tone}`} />
              {label}
            </button>
          );
        })}
      </div>

      <PlatformEditor
        platform={activePlatform}
        edit={edit}
        onChange={setEdit}
      />

      {/* Transcript collapsible */}
      {enrichment.transcript_corrected && (
        <details className="rounded-xl border border-border bg-bg/40 p-3">
          <summary className="cursor-pointer text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary inline-flex items-center gap-1.5">
            <ScrollText className="w-3 h-3" /> Transkript anzeigen
          </summary>
          <div className="text-xs text-text-secondary leading-relaxed mt-3 whitespace-pre-wrap font-mono">
            {enrichment.transcript_corrected}
          </div>
        </details>
      )}

      {/* Actions */}
      <div className="flex items-center gap-3 border-t border-border pt-3">
        <div className="text-[11px] text-text-secondary">
          {dirty ? 'Ungesicherte Änderungen' : 'Synchron mit Server'}
        </div>
        <div className="ml-auto flex gap-2">
          <button
            type="button"
            disabled={!dirty || saveMutation.isPending}
            onClick={() => setEdit(fromEnrichment(enrichment))}
            className="px-3 py-2 rounded-xl text-xs font-semibold text-text-secondary border border-border hover:text-white disabled:opacity-40"
          >
            Zurücksetzen
          </button>
          <button
            type="button"
            disabled={!dirty || saveMutation.isPending}
            onClick={() => saveMutation.mutate(toPayload(enrichment, edit))}
            className="px-4 py-2 rounded-xl text-xs font-bold inline-flex items-center gap-2 bg-orange text-white shadow-[0_8px_22px_-8px_rgba(255,122,24,0.6)] hover:bg-orange-hover transition disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {saveMutation.isPending ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Save className="w-3.5 h-3.5" />}
            Speichern
          </button>
        </div>
      </div>
      {saveMutation.isSuccess && !dirty && (
        <div className="text-xs text-success inline-flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5" /> Gespeichert.
        </div>
      )}
    </div>
  );
}

interface PlatformEditorProps {
  platform: SocialPlatform;
  edit: EditState;
  onChange: (next: EditState) => void;
}

function PlatformEditor({ platform, edit, onChange }: PlatformEditorProps) {
  const config = PLATFORMS.find((p) => p.id === platform)!;
  const titleKey = `title_${platform}` as const;
  const descKey = `description_${platform}` as const;
  const tagsKey = `hashtags_${platform}` as const;

  const title = edit[titleKey];
  const desc = edit[descKey];
  const tags = edit[tagsKey];
  const titleLen = title.length;

  return (
    <div className="space-y-4">
      <div>
        <label className="block text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary mb-1.5">
          Title
          <span className={`ml-2 font-mono ${titleLen > config.titleLimit ? 'text-danger' : 'text-text-secondary'}`}>
            {titleLen}/{config.titleLimit}
          </span>
        </label>
        <input
          type="text"
          value={title}
          onChange={(e) => onChange({ ...edit, [titleKey]: e.target.value })}
          maxLength={config.titleLimit + 20}
          placeholder={`${config.label}-Title…`}
          className="w-full px-3 py-2 rounded-xl bg-bg/60 border border-border focus:border-orange/60 focus:outline-none text-sm text-white placeholder:text-text-secondary/60"
        />
      </div>

      <div>
        <label className="block text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary mb-1.5">
          Beschreibung
        </label>
        <textarea
          value={desc}
          onChange={(e) => onChange({ ...edit, [descKey]: e.target.value })}
          rows={3}
          placeholder={`Kurze Beschreibung für ${config.label}…`}
          className="w-full px-3 py-2 rounded-xl bg-bg/60 border border-border focus:border-orange/60 focus:outline-none text-sm text-white placeholder:text-text-secondary/60 resize-y leading-relaxed"
        />
      </div>

      <div>
        <label className="block text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary mb-1.5 inline-flex items-center gap-1.5">
          <Hash className="w-3 h-3" /> Hashtags
          <span className="ml-1 font-mono text-text-secondary">
            {tags.length} · Ziel {config.hashtagTarget}
          </span>
        </label>
        <HashtagsEditor
          tags={tags}
          onChange={(next) => onChange({ ...edit, [tagsKey]: next })}
        />
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
  const [input, setInput] = useState('');

  const addTag = (raw: string) => {
    const cleaned = raw
      .trim()
      .replace(/^#+/, '')
      .replace(/\s+/g, '')
      .toLowerCase();
    if (!cleaned) return;
    if (tags.includes(cleaned)) return;
    onChange([...tags, cleaned]);
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
          #{tag}
          <button
            type="button"
            onClick={() => removeTag(tag)}
            className="hover:text-white"
            aria-label={`Hashtag #${tag} entfernen`}
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
        placeholder={tags.length === 0 ? 'Hashtag eingeben + Enter…' : ''}
        className="flex-1 min-w-[120px] bg-transparent text-sm text-white placeholder:text-text-secondary/60 outline-none px-2 py-1"
      />
    </div>
  );
}
