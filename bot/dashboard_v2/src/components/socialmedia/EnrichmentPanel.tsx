import { useEffect, useId, useState, type KeyboardEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertCircle,
  CheckCircle2,
  Hash,
  Loader2,
  Camera,
  RefreshCw,
  Save,
  Sparkles,
  X,
  Video,
  Music2,
  ScrollText,
  Wand2,
} from 'lucide-react';
import {
  fetchClipEnrichment,
  runClipEnrichment,
  saveClipEnrichment,
  type EnrichmentEditPayload,
} from '@/api/socialMedia';
import type { ClipEnrichment, SocialPlatform } from '@/types/socialMedia';
import { useT } from '@/context/LanguageContext';
import { fehlerText, STATUS_META, TONE_BADGE as TONE } from './labels';

const PLATFORMS: Array<{
  id: SocialPlatform;
  label: string;
  Icon: React.ComponentType<{ className?: string; 'aria-hidden'?: boolean }>;
  tone: string;
  titleLimit: number;
  descriptionLimit: number;
  hashtagTarget: string;
  hashtagLimit: number;
  hashtagLengthLimit: number;
}> = [
  {
    id: 'youtube',
    label: 'YouTube Shorts',
    Icon: Video,
    tone: 'text-[#FF5A3C]',
    titleLimit: 100,
    descriptionLimit: 5000,
    hashtagTarget: '5–10',
    hashtagLimit: 10,
    hashtagLengthLimit: 100,
  },
  {
    id: 'tiktok',
    label: 'TikTok',
    Icon: Music2,
    tone: 'text-[#00D9FF]',
    titleLimit: 150,
    descriptionLimit: 2200,
    hashtagTarget: '8–12',
    hashtagLimit: 12,
    hashtagLengthLimit: 100,
  },
  {
    id: 'instagram',
    label: 'Instagram Reels',
    Icon: Camera,
    tone: 'text-[#FF5A3C]',
    titleLimit: 125,
    descriptionLimit: 2200,
    hashtagTarget: '8–15',
    hashtagLimit: 15,
    hashtagLengthLimit: 100,
  },
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

function normalisiereHashtags(hashtags: string[] | null | undefined): string[] {
  const gesehen = new Set<string>();
  const ergebnis: string[] = [];
  for (const hashtag of hashtags ?? []) {
    const wert = hashtag.trim().replace(/^#+/, '').replace(/\s+/g, '').toLowerCase();
    if (wert && !gesehen.has(wert)) {
      gesehen.add(wert);
      ergebnis.push(wert);
    }
  }
  return ergebnis;
}

function fromEnrichment(e: ClipEnrichment): EditState {
  return {
    title_youtube: e.title_youtube ?? '',
    title_tiktok: e.title_tiktok ?? '',
    title_instagram: e.title_instagram ?? '',
    description_youtube: e.description_youtube ?? '',
    description_tiktok: e.description_tiktok ?? '',
    description_instagram: e.description_instagram ?? '',
    hashtags_youtube: normalisiereHashtags(e.hashtags_youtube),
    hashtags_tiktok: normalisiereHashtags(e.hashtags_tiktok),
    hashtags_instagram: normalisiereHashtags(e.hashtags_instagram),
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
  if (JSON.stringify(edit.hashtags_youtube) !== JSON.stringify(normalisiereHashtags(initial.hashtags_youtube))) payload.hashtags_youtube = edit.hashtags_youtube;
  if (JSON.stringify(edit.hashtags_tiktok) !== JSON.stringify(normalisiereHashtags(initial.hashtags_tiktok))) payload.hashtags_tiktok = edit.hashtags_tiktok;
  if (JSON.stringify(edit.hashtags_instagram) !== JSON.stringify(normalisiereHashtags(initial.hashtags_instagram))) payload.hashtags_instagram = edit.hashtags_instagram;
  return payload;
}

const zeichenAnzahl = (wert: string): number => Array.from(wert).length;
const hashtagZeichenAnzahl = (wert: string): number =>
  zeichenAnzahl(`#${wert.replace(/^#+/, '')}`);

function hatPlattformGrenzfehler(edit: EditState): boolean {
  return PLATFORMS.some(({
    id,
    titleLimit,
    descriptionLimit,
    hashtagLimit,
    hashtagLengthLimit,
  }) => {
    const title = edit[`title_${id}` as keyof EditState];
    const description = edit[`description_${id}` as keyof EditState];
    const hashtags = edit[`hashtags_${id}` as keyof EditState];
    return (
      (typeof title === 'string' && zeichenAnzahl(title) > titleLimit) ||
      (typeof description === 'string' && zeichenAnzahl(description) > descriptionLimit) ||
      (Array.isArray(hashtags) && (
        hashtags.length > hashtagLimit ||
        hashtags.some((hashtag) => hashtagZeichenAnzahl(hashtag) > hashtagLengthLimit)
      ))
    );
  });
}

export function EnrichmentPanel({ clipDbId, onClose }: EnrichmentPanelProps) {
  const t = useT();
  const queryClient = useQueryClient();
  const instanceId = useId();
  const [edit, setEdit] = useState<EditState | null>(null);
  const [activePlatform, setActivePlatform] = useState<SocialPlatform>('youtube');
  const headingId = `${instanceId}-heading`;
  const editorId = `${instanceId}-platform-editor`;
  const activePlatformButtonId = `${instanceId}-${activePlatform}-platform`;

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

  if (enrichmentQuery.isError) {
    return (
      <div
        role="region"
        aria-label={t('Metadaten')}
        tabIndex={-1}
        className="min-w-0 rounded-xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70"
      >
        <div role="alert" className="flex items-start gap-3 rounded-xl border border-danger/35 bg-danger/10 p-4 text-sm text-danger">
          <AlertCircle aria-hidden="true" className="mt-0.5 h-5 w-5 shrink-0" />
          <div className="min-w-0 flex-1">
            <p className="font-bold">{t('Metadaten konnten nicht geladen werden.')}</p>
            <p className="mt-1 break-words text-xs text-text-secondary">
              {fehlerText(enrichmentQuery.error, t)}
            </p>
            <button
              type="button"
              onClick={() => enrichmentQuery.refetch()}
              disabled={enrichmentQuery.isFetching}
              className="mt-3 inline-flex min-h-8 items-center gap-1.5 rounded-lg border border-danger/35 px-3 py-1.5 text-xs font-bold hover:bg-danger/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger/70 disabled:opacity-50"
            >
              {enrichmentQuery.isFetching ? (
                <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw aria-hidden="true" className="h-3.5 w-3.5" />
              )}
              {t('Erneut versuchen')}
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (enrichmentQuery.isLoading || !enrichmentQuery.data || !edit) {
    return (
      <div
        role="region"
        aria-label={t('Metadaten')}
        aria-busy="true"
        tabIndex={-1}
        className="min-w-0 rounded-xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70"
      >
        <div role="status" aria-live="polite" aria-atomic="true" className="flex items-center justify-center gap-2 py-10 text-sm text-text-secondary">
          <Loader2 aria-hidden="true" className="h-5 w-5 animate-spin text-orange" />
          <span>{t('Metadaten werden geladen…')}</span>
        </div>
      </div>
    );
  }

  const enrichment = enrichmentQuery.data!;
  const status = STATUS_META[enrichment.status] ?? STATUS_META.pending;
  const dirty = JSON.stringify(toPayload(enrichment, edit)) !== '{}';
  const editInvalid = hatPlattformGrenzfehler(edit);
  const grenzfehlerId = `${instanceId}-grenzfehler`;
  const isProcessing =
    enrichment.status === 'transcribing' ||
    enrichment.status === 'correcting' ||
    enrichment.status === 'llm' ||
    runMutation.isPending ||
    saveMutation.isPending;

  return (
    <div
      role="region"
      aria-labelledby={headingId}
      aria-busy={isProcessing}
      tabIndex={-1}
      className="min-w-0 space-y-5 rounded-xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70"
    >
      <div className="flex flex-wrap items-center gap-3 border-b border-border pb-3">
        <div className="flex items-center gap-2">
          <Wand2 aria-hidden="true" className="h-4 w-4 text-orange" />
          <h4 id={headingId} className="text-sm font-bold uppercase tracking-[0.16em] text-white">{t('Metadaten')}</h4>
        </div>
        <span
          role="status"
          aria-live="polite"
          aria-atomic="true"
          className={`rounded-md border px-2 py-1 text-[10px] font-bold uppercase tracking-[0.14em] ${TONE[status.tone]}`}
        >
          {saveMutation.isPending
            ? t('Speichert…')
            : runMutation.isPending
              ? t('Neu generieren')
              : t(status.label)}
        </span>
        {enrichment.llm_provider && (
          <span className="max-w-full break-all rounded-md border border-border bg-bg/60 px-2 py-1 font-mono text-[10px] text-text-secondary">
            {enrichment.llm_provider}
            {enrichment.llm_model ? ` · ${enrichment.llm_model}` : ''}
          </span>
        )}
        {typeof enrichment.cost_usd_estimate === 'number' && enrichment.cost_usd_estimate > 0 && (
          <span className="text-[10px] font-mono text-text-secondary">
            ≈ ${enrichment.cost_usd_estimate.toFixed(4)}
          </span>
        )}
        <div className="flex w-full flex-wrap items-center justify-end gap-2 sm:ml-auto sm:w-auto">
          <button
            type="button"
            disabled={isProcessing || dirty}
            title={dirty ? t('Speichere oder verwirf zuerst deine Änderungen.') : undefined}
            onClick={() => {
              saveMutation.reset();
              runMutation.mutate(true);
            }}
            className="inline-flex min-h-8 items-center gap-1.5 rounded-lg px-2 text-xs font-semibold text-text-secondary hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70 disabled:opacity-40"
          >
            <RefreshCw aria-hidden="true" className={`h-3.5 w-3.5 ${runMutation.isPending ? 'animate-spin' : ''}`} />
            {t('Neu generieren')}
          </button>
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              className="inline-flex min-h-8 min-w-8 items-center justify-center rounded-lg p-1.5 text-text-secondary hover:bg-bg/60 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70"
              aria-label={t('Enrichment-Panel schließen')}
            >
              <X aria-hidden="true" className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>

      {enrichment.error_message && (
        <div role="alert" className="flex items-start gap-2 rounded-lg border border-danger/30 bg-danger/10 p-2.5 text-xs text-danger">
          <AlertCircle aria-hidden="true" className="mt-0.5 h-4 w-4 flex-shrink-0" />
          <span className="min-w-0 break-words">{enrichment.error_message}</span>
        </div>
      )}

      {enrichment.status === 'skipped_no_key' && (
        <div role="status" aria-live="polite" className="rounded-lg border border-border bg-bg/40 p-3 text-xs leading-relaxed text-text-secondary">
          {t(
            'Die automatische Anreicherung ist für diesen Kanal noch nicht verfügbar. Du kannst die Metadaten manuell bearbeiten oder es später erneut versuchen.',
          )}
        </div>
      )}

      {/* Detected terms */}
      {enrichment.detected_terms.length > 0 && (
        <div className="space-y-2">
          <div className="text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary inline-flex items-center gap-1.5">
            <Sparkles aria-hidden="true" className="h-3 w-3 text-accent" /> {t('Erkannte Begriffe')}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {enrichment.detected_terms.map((term) => (
              <span
                key={term}
                className="text-[11px] font-semibold px-2 py-1 rounded-md bg-accent/10 text-accent border border-accent/30"
              >
                {term}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Platform tabs */}
      <div role="group" aria-label={t('Metadaten')} className="grid grid-cols-1 gap-1.5 sm:grid-cols-3">
        {PLATFORMS.map(({ id, label, Icon, tone }) => {
          const active = activePlatform === id;
          return (
            <button
              key={id}
              id={`${instanceId}-${id}-platform`}
              type="button"
              onClick={() => setActivePlatform(id)}
              aria-pressed={active}
              aria-controls={editorId}
              className={`inline-flex min-h-8 w-full items-center justify-center gap-1.5 rounded-xl border px-3 py-1.5 text-xs font-semibold transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70 ${
                active
                  ? 'bg-orange/15 text-orange border-orange/40 shadow-[0_4px_18px_-8px_rgba(201, 168, 106, 0.5)]'
                  : 'bg-bg/40 text-text-secondary border-border hover:text-white'
              }`}
            >
              <Icon aria-hidden={true} className={`h-3.5 w-3.5 ${active ? 'text-orange' : tone}`} />
              {t(label)}
            </button>
          );
        })}
      </div>

      <PlatformEditor
        id={editorId}
        labelledBy={activePlatformButtonId}
        fieldIdPrefix={instanceId}
        platform={activePlatform}
        edit={edit}
        onChange={setEdit}
      />

      {editInvalid && (
        <div id={grenzfehlerId} role="alert" className="rounded-lg border border-danger/30 bg-danger/10 p-2.5 text-xs text-danger">
          {t('Korrigiere die markierten Plattformgrenzen, bevor du speicherst.')}
        </div>
      )}

      {/* Transcript collapsible */}
      {enrichment.transcript_corrected && (
        <details className="rounded-xl border border-border bg-bg/40 p-3">
          <summary className="inline-flex min-h-6 cursor-pointer items-center gap-1.5 rounded text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70">
            <ScrollText aria-hidden="true" className="h-3 w-3" /> {t('Transkript anzeigen')}
          </summary>
          <div className="mt-3 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-text-secondary">
            {enrichment.transcript_corrected}
          </div>
        </details>
      )}

      {/* Actions */}
      <div className="flex flex-col items-stretch gap-3 border-t border-border pt-3 sm:flex-row sm:items-center">
        <div className="text-[11px] text-text-secondary">
          {dirty ? t('Ungesicherte Änderungen') : t('Synchron mit Server')}
        </div>
        <div className="grid w-full grid-cols-2 gap-2 sm:ml-auto sm:flex sm:w-auto">
          <button
            type="button"
            disabled={!dirty || isProcessing}
            onClick={() => setEdit(fromEnrichment(enrichment))}
            className="min-h-9 rounded-xl border border-border px-3 py-2 text-xs font-semibold text-text-secondary hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70 disabled:opacity-40"
          >
            {t('Zurücksetzen')}
          </button>
          <button
            type="button"
            disabled={!dirty || isProcessing || editInvalid}
            aria-describedby={editInvalid ? grenzfehlerId : undefined}
            onClick={() => {
              runMutation.reset();
              saveMutation.mutate(toPayload(enrichment, edit));
            }}
            className="inline-flex min-h-9 items-center justify-center gap-2 rounded-xl bg-orange px-4 py-2 text-xs font-bold text-on-gold shadow-[0_8px_22px_-8px_rgba(201,168,106,0.6)] transition hover:bg-orange-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange disabled:cursor-not-allowed disabled:opacity-40"
          >
            {saveMutation.isPending ? <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin" /> : <Save aria-hidden="true" className="h-3.5 w-3.5" />}
            {t('Speichern')}
          </button>
        </div>
      </div>
      {saveMutation.isSuccess && !dirty && (
        <div role="status" aria-live="polite" aria-atomic="true" className="inline-flex items-center gap-1.5 text-xs text-success">
          <CheckCircle2 aria-hidden="true" className="h-3.5 w-3.5" /> {t('Gespeichert.')}
        </div>
      )}
      {(saveMutation.isError || runMutation.isError) && (
        <div role="alert" className="flex items-start gap-2 rounded-lg border border-danger/30 bg-danger/10 p-2.5 text-xs text-danger">
          <AlertCircle aria-hidden="true" className="mt-0.5 h-4 w-4 flex-shrink-0" />
          <span className="min-w-0 break-words">{fehlerText(saveMutation.error ?? runMutation.error, t)}</span>
        </div>
      )}
    </div>
  );
}

interface PlatformEditorProps {
  id: string;
  labelledBy: string;
  fieldIdPrefix: string;
  platform: SocialPlatform;
  edit: EditState;
  onChange: (next: EditState) => void;
}

function PlatformEditor({ id, labelledBy, fieldIdPrefix, platform, edit, onChange }: PlatformEditorProps) {
  const t = useT();
  const config = PLATFORMS.find((p) => p.id === platform)!;
  const titleKey = `title_${platform}` as const;
  const descKey = `description_${platform}` as const;
  const tagsKey = `hashtags_${platform}` as const;

  const title = edit[titleKey];
  const desc = edit[descKey];
  const tags = edit[tagsKey];
  const titleLen = zeichenAnzahl(title);
  const descriptionLen = zeichenAnzahl(desc);
  const titleInvalid = titleLen > config.titleLimit;
  const descriptionInvalid = descriptionLen > config.descriptionLimit;
  const hashtagCountInvalid = tags.length > config.hashtagLimit;
  const hashtagLengthInvalid = tags.some(
    (hashtag) => hashtagZeichenAnzahl(hashtag) > config.hashtagLengthLimit,
  );
  const hashtagsInvalid = hashtagCountInvalid || hashtagLengthInvalid;
  const titleId = `${fieldIdPrefix}-${platform}-title`;
  const titleHintId = `${titleId}-hint`;
  const titleErrorId = `${titleId}-error`;
  const descriptionId = `${fieldIdPrefix}-${platform}-description`;
  const descriptionHintId = `${descriptionId}-hint`;
  const descriptionErrorId = `${descriptionId}-error`;
  const hashtagsId = `${fieldIdPrefix}-${platform}-hashtags`;
  const hashtagsHintId = `${hashtagsId}-hint`;
  const hashtagsCountErrorId = `${hashtagsId}-count-error`;
  const hashtagsLengthErrorId = `${hashtagsId}-length-error`;

  return (
    <div id={id} role="group" aria-labelledby={labelledBy} className="min-w-0 space-y-4">
      <div>
        <div className="mb-1.5 flex flex-wrap items-center gap-2">
          <label htmlFor={titleId} className="text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary">
            {t('Titel')}
          </label>
          <span id={titleHintId} className={`font-mono text-[11px] font-bold uppercase tracking-[0.14em] ${titleInvalid ? 'text-danger' : 'text-text-secondary'}`}>
            {titleLen}/{config.titleLimit}
          </span>
        </div>
        <input
          id={titleId}
          type="text"
          value={title}
          onChange={(e) => onChange({ ...edit, [titleKey]: e.target.value })}
          maxLength={config.titleLimit + 20}
          aria-describedby={titleInvalid ? `${titleHintId} ${titleErrorId}` : titleHintId}
          aria-invalid={titleInvalid}
          placeholder={t('{platform}-Title…', { platform: config.label })}
          className={`w-full rounded-xl border bg-bg/60 px-3 py-2 text-sm text-white placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-orange ${titleInvalid ? 'border-danger/60' : 'border-orange/70 focus:border-orange'}`}
        />
        {titleInvalid && (
          <p id={titleErrorId} role="alert" className="mt-1 text-xs text-danger">
            {t('Der Titel darf höchstens {count} Zeichen lang sein.', {
              count: config.titleLimit,
            })}
          </p>
        )}
      </div>

      <div>
        <div className="mb-1.5 flex flex-wrap items-center gap-2">
          <label htmlFor={descriptionId} className="text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary">
            {t('Beschreibung')}
          </label>
          <span id={descriptionHintId} className={`font-mono text-[11px] font-bold uppercase tracking-[0.14em] ${descriptionInvalid ? 'text-danger' : 'text-text-secondary'}`}>
            {descriptionLen}/{config.descriptionLimit}
          </span>
        </div>
        <textarea
          id={descriptionId}
          value={desc}
          onChange={(e) => onChange({ ...edit, [descKey]: e.target.value })}
          aria-invalid={descriptionInvalid}
          aria-describedby={descriptionInvalid ? `${descriptionHintId} ${descriptionErrorId}` : descriptionHintId}
          rows={3}
          placeholder={t('Kurze Beschreibung für {platform}…', { platform: config.label })}
          className={`w-full resize-y rounded-xl border bg-bg/60 px-3 py-2 text-sm leading-relaxed text-white placeholder:text-text-secondary focus:outline-none focus:ring-2 focus:ring-orange ${descriptionInvalid ? 'border-danger/60' : 'border-orange/70 focus:border-orange'}`}
        />
        {descriptionInvalid && (
          <p id={descriptionErrorId} role="alert" className="mt-1 text-xs text-danger">
            {t('Die Beschreibung darf höchstens {count} Zeichen lang sein.', {
              count: config.descriptionLimit,
            })}
          </p>
        )}
      </div>

      <div>
        <div className="mb-1.5 flex flex-wrap items-center gap-2">
          <label htmlFor={hashtagsId} className="inline-flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-[0.14em] text-text-secondary">
            <Hash aria-hidden="true" className="h-3 w-3" /> {t('Hashtags')}
          </label>
          <span id={hashtagsHintId} className={`font-mono text-[11px] font-bold uppercase tracking-[0.14em] ${hashtagsInvalid ? 'text-danger' : 'text-text-secondary'}`}>
            {t('{count} · Ziel {target}', { count: tags.length, target: config.hashtagTarget })}
            <span className="sr-only"> {t('Hashtag eingeben + Enter…')}</span>
          </span>
        </div>
        <HashtagsEditor
          key={platform}
          id={hashtagsId}
          describedBy={[
            hashtagsHintId,
            hashtagCountInvalid ? hashtagsCountErrorId : null,
            hashtagLengthInvalid ? hashtagsLengthErrorId : null,
          ].filter(Boolean).join(' ')}
          invalid={hashtagsInvalid}
          tags={tags}
          onChange={(next) => onChange({ ...edit, [tagsKey]: next })}
        />
        {hashtagCountInvalid && (
          <p id={hashtagsCountErrorId} role="alert" className="mt-1 text-xs text-danger">
            {t('Für {platform} sind höchstens {count} Hashtags erlaubt.', {
              platform: config.label,
              count: config.hashtagLimit,
            })}
          </p>
        )}
        {hashtagLengthInvalid && (
          <p id={hashtagsLengthErrorId} role="alert" className="mt-1 text-xs text-danger">
            {t('Ein Hashtag darf höchstens {count} Zeichen lang sein.', {
              count: config.hashtagLengthLimit,
            })}
          </p>
        )}
      </div>
    </div>
  );
}

function HashtagsEditor({
  id,
  describedBy,
  invalid,
  tags,
  onChange,
}: {
  id: string;
  describedBy: string;
  invalid: boolean;
  tags: string[];
  onChange: (next: string[]) => void;
}) {
  const t = useT();
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
    <div className={`flex min-w-0 flex-wrap items-center gap-1.5 rounded-xl border bg-bg/60 p-2 focus-within:ring-2 focus-within:ring-orange ${invalid ? 'border-danger/60' : 'border-orange/70 focus-within:border-orange'}`}>
      {tags.map((tag) => (
        <span
          key={tag}
          className="inline-flex items-center gap-1 text-xs font-semibold px-2 py-1 rounded-md bg-orange/10 text-orange border border-orange/30"
        >
          #{tag}
          <button
            type="button"
            onClick={() => removeTag(tag)}
            className="inline-flex min-h-6 min-w-6 items-center justify-center rounded hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange/70"
            aria-label={t('Hashtag #{tag} entfernen', { tag })}
          >
            <X aria-hidden="true" className="h-3 w-3" />
          </button>
        </span>
      ))}
      <input
        id={id}
        type="text"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKey}
        aria-describedby={describedBy}
        aria-invalid={invalid}
        onBlur={() => {
          if (input.trim()) {
            addTag(input);
            setInput('');
          }
        }}
        placeholder={tags.length === 0 ? t('Hashtag eingeben + Enter…') : ''}
        className="min-h-6 min-w-[120px] flex-[1_1_120px] bg-transparent px-2 py-1 text-sm text-white outline-none placeholder:text-text-secondary"
      />
    </div>
  );
}
