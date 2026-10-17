import { useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Archive, Clapperboard, Crop, ExternalLink, Film, ListX, Pencil, Trophy } from 'lucide-react';
import { useLanguage } from '@/context/LanguageContext';
import { discardClip, getPreviewStatus, previewFileUrl, requestPreview, submitClipToContest } from '@/api/socialMedia';
import { PLATFORM_LABELS } from '@/components/socialmedia/labels';
import type { SocialPlatform } from '@/types/socialMedia';
import { CLIP_MENU_ITEM, ClipActionMenu } from './ClipCard';
import { PIPELINE_DETAIL_CUTOVER, type PipelinePage, type PipelineSource } from './pipelineApi';
import { ClipReviewDetail } from './autopilot/publish-review/PublishReviewPanel';
import { scheduleQueueApi, type QueueJob, type ReviewDecision } from './autopilot/schedule-queue/api';
import { ReviewJobActions } from './autopilot/schedule-queue/ReviewJobActions';
import './autopilot/schedule-queue/schedule-queue.css';
import { QUEUE_FILTERS, filterSources, legacyUncertainPlatforms, partialResult, publishedPlatforms, sourceStage, type EligibilityFilter, type QueueStage } from './queuePresentation';
import { clipDuration, clipThumbnail, insideWindow, localTimestamp, sourceMetadata } from './autopilot/import-pool/presentation';
import type { PoolPreferences } from './autopilot/import-pool/types';
import { HOLD_REASONS } from './reviewReasons';

const PAGE_SIZE = 24;
const PLATFORMS: SocialPlatform[] = ['youtube', 'tiktok', 'instagram'];
const STAGE_TONES: Record<Exclude<QueueStage, 'all'>, string> = {
  new: 'neutral',
  preparing: 'info',
  review: 'warning',
  planned: 'info',
  posted: 'success',
  failed: 'danger',
  archive: 'neutral',
};
const ELIGIBILITY_FILTERS: Array<{ id: EligibilityFilter; label: string }> = [
  { id: 'all', label: 'Alle Quellen' },
  { id: 'automatic', label: 'Deadlock automatisch' },
  { id: 'manual', label: 'Eigene Uploads' },
  { id: 'legacy_non_deadlock_visible', label: 'Andere Spiele' },
];


export type DetailMode = 'ai' | 'layout' | 'enrichment';

type Translate = (text: string, params?: Record<string, string | number>) => string;

export interface PipelineClipsProps {
  page: PipelinePage;
  selectionLocked: boolean;
  onSelect: (clipId: string, selected: boolean) => void;
  onBulkSelect: (clipIds: string[], selected: boolean) => void;
  bulkPending?: boolean;
  onArchived?: () => void;
  onOpenDetail?: (source: PipelineSource, mode: DetailMode) => void;
  owner?: string;
  initialStage?: QueueStage;
}

function sourceUrl(source: PipelineSource): string | null {
  const url = sourceMetadata(source).url;
  return url && url.startsWith('https://') ? url : null;
}

function selectable(source: PipelineSource, preferences: PoolPreferences): boolean {
  if (!source.pooled || source.eligibility !== 'automatic' || source.discarded_at) return false;
  if (source.processed_at || source.published_at || source.legacy_reserved) return false;
  if (source.selected_at !== null) return true;
  if (source.jobs.some((job) => job.status !== 'cancelled')) return false;
  if (source.versions.length > 0 || source.destinations.length > 0) return false;
  return insideWindow(source.source_created_at, preferences);
}

function eligibilityNote(source: PipelineSource, t: Translate): string | null {
  if (source.eligibility === 'manual') return t('Eigener Upload');
  if (source.eligibility === 'legacy_non_deadlock_visible') return t('Anderes Spiel, wird nicht automatisch gepostet');
  return null;
}

export function PipelineClips({ page, selectionLocked, onSelect, onBulkSelect, bulkPending, onArchived, onOpenDetail, owner, initialStage = 'all' }: PipelineClipsProps) {
  const { t, locale } = useLanguage();
  const [detail, setDetail] = useState<{ source: PipelineSource; mode: DetailMode } | null>(null);
  const openDetail = onOpenDetail ?? (owner ? (source: PipelineSource, mode: DetailMode) => setDetail({ source, mode }) : undefined);
  const [eligibility, setEligibility] = useState<EligibilityFilter>('all');
  const [stage, setStage] = useState<QueueStage>(initialStage);
  const [appliedStage, setAppliedStage] = useState<QueueStage>(initialStage);
  const [search, setSearch] = useState('');
  const [platform, setPlatform] = useState<SocialPlatform | 'all'>('all');
  const [pageIndex, setPageIndex] = useState(0);
  if (appliedStage !== initialStage) {
    setAppliedStage(initialStage);
    setStage(initialStage);
    setPageIndex(0);
  }
  const held = page.sources.some((source) => source.jobs.some((job) => job.status === 'review'));
  const queue = useQuery({
    queryKey: ['social-media', 'autopilot', 'schedule-queue', owner],
    queryFn: ({ signal }) => scheduleQueueApi.load(owner, signal),
    enabled: Boolean(owner) && held,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const queueJobs = useMemo(() => new Map((queue.data?.jobs ?? []).map((job) => [job.id, job])), [queue.data]);
  const filtered = useMemo(
    () => filterSources(page.sources, { eligibility, stage, search, platform }, (source) => sourceMetadata(source).title),
    [page.sources, eligibility, stage, search, platform],
  );
  const pages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const current = Math.min(pageIndex, pages - 1);
  const visible = filtered.slice(current * PAGE_SIZE, (current + 1) * PAGE_SIZE);
  const allowed = visible.filter((source) => selectable(source, page.preferences));
  const toSelect = allowed.filter((source) => source.selected_at === null).map((source) => source.clip_id);
  const toClear = allowed.filter((source) => source.selected_at !== null).map((source) => source.clip_id);
  const counts: Record<EligibilityFilter, number> = {
    all: page.counts.automatic + page.counts.manual + page.counts.legacy_non_deadlock_visible,
    ...page.counts,
  };

  function reset<T>(apply: (value: T) => void) {
    return (value: T) => {
      apply(value);
      setPageIndex(0);
    };
  }

  return (
    <div className="pipeline-clips">
      <div className="pipeline-clips-filters" role="group" aria-label={t('Clips filtern')}>
        <div className="studio-filters">
          {QUEUE_FILTERS.map((item) => (
            <button key={item.id} type="button" className="studio-filter" aria-pressed={stage === item.id} onClick={() => reset(setStage)(item.id)}>
              {t(item.label)}
            </button>
          ))}
        </div>
        <div className="pipeline-clips-search">
          <input
            type="search"
            className="studio-input"
            value={search}
            placeholder={t('Titel oder Clip-ID suchen')}
            aria-label={t('Titel oder Clip-ID suchen')}
            onChange={(event) => reset(setSearch)(event.target.value)}
          />
          <select className="studio-select" value={eligibility} aria-label={t('Quelle')} onChange={(event) => reset(setEligibility)(event.target.value as EligibilityFilter)}>
            {ELIGIBILITY_FILTERS.map((item) => (
              <option key={item.id} value={item.id}>{`${t(item.label)} (${counts[item.id].toLocaleString(locale)})`}</option>
            ))}
          </select>
          <select className="studio-select" value={platform} aria-label={t('Plattform')} onChange={(event) => reset(setPlatform)(event.target.value as SocialPlatform | 'all')}>
            <option value="all">{t('Alle Plattformen')}</option>
            {PLATFORMS.map((item) => <option key={item} value={item}>{PLATFORM_LABELS[item]}</option>)}
          </select>
          <div className="import-pool-bulk">
            <button type="button" className="studio-button" title={t('Alle auf dieser Seite auswählen')} disabled={selectionLocked || toSelect.length === 0} onClick={() => onBulkSelect(toSelect, true)}>
              {t('Alle auswählen')}
            </button>
            <button type="button" className="studio-button" disabled={selectionLocked || toClear.length === 0} onClick={() => onBulkSelect(toClear, false)}>
              {t('Auswahl aufheben')}
            </button>
          </div>
        </div>
      </div>
      <p className="import-pool-help" role="status">
        {t('{count} von {total} Clips', { count: filtered.length.toLocaleString(locale), total: page.sources.length.toLocaleString(locale) })}
        {bulkPending && ` ${t('Auswahl wird gespeichert …')}`}
      </p>
      {page.sources.length === 0 ? (
        <p className="import-pool-empty">
          {t('In diesem Clip-Pool sind noch keine Clips erfasst. Mit einem historischen Import kannst du verfügbare Deadlock-Clips hinzufügen.')}
        </p>
      ) : filtered.length === 0 ? (
        <p className="import-pool-empty">{t('Keine Clips passen zu diesen Filtern.')}</p>
      ) : (
        <ul className="import-pool-clips">
          {visible.map((source) => (
            <PipelineClip
              key={source.clip_id}
              source={source}
              timezone={page.preferences.timezone}
              selectable={selectable(source, page.preferences)}
              outsideWindow={source.eligibility === 'automatic' && !insideWindow(source.source_created_at, page.preferences)}
              selectionLocked={selectionLocked}
              onSelect={onSelect}
              onArchived={onArchived}
              onOpenDetail={openDetail}
              owner={owner}
              queueJobs={queueJobs}
              onQueueChanged={() => { void queue.refetch(); onArchived?.(); }}
            />
          ))}
        </ul>
      )}
      {pages > 1 && (
        <nav className="import-pool-pagination" aria-label={t('Clip-Pool-Seiten')}>
          <button type="button" className="studio-button" disabled={current === 0} onClick={() => setPageIndex(current - 1)}>
            {t('Zurück')}
          </button>
          <span>{t('Seite')} {current + 1} / {pages}</span>
          <button type="button" className="studio-button" disabled={current + 1 >= pages} onClick={() => setPageIndex(current + 1)}>
            {t('Weiter')}
          </button>
        </nav>
      )}
      {detail && owner && (
        <ClipReviewDetail
          owner={owner}
          source={page.sources.find((item) => item.clip_id === detail.source.clip_id) ?? detail.source}
          mode={detail.mode}
          title={sourceMetadata(detail.source).title}
          onClose={() => setDetail(null)}
          onChanged={onArchived}
        />
      )}
    </div>
  );
}

function PipelineClip({
  source,
  timezone,
  selectable: canSelect,
  outsideWindow,
  selectionLocked,
  onSelect,
  onArchived,
  onOpenDetail,
  owner,
  queueJobs,
  onQueueChanged,
}: {
  source: PipelineSource;
  timezone: string;
  selectable: boolean;
  outsideWindow: boolean;
  selectionLocked: boolean;
  onSelect: (clipId: string, selected: boolean) => void;
  onArchived?: () => void;
  onOpenDetail?: (source: PipelineSource, mode: DetailMode) => void;
  owner?: string;
  queueJobs: Map<number, QueueJob>;
  onQueueChanged: () => void;
}) {
  const { t, locale } = useLanguage();
  const queryClient = useQueryClient();
  const metadata = sourceMetadata(source);
  const thumbnail = clipThumbnail(source);
  const url = sourceUrl(source);
  const stage = sourceStage(source);
  const dbId = source.clip_db_id;
  const selected = source.selected_at !== null;
  const detailReady = PIPELINE_DETAIL_CUTOVER && Boolean(onOpenDetail);
  const [previewActive, setPreviewActive] = useState(false);
  const previewKey = ['social-media', 'preview', dbId];
  const preview = useQuery({
    queryKey: previewKey,
    queryFn: () => getPreviewStatus(dbId ?? 0),
    enabled: previewActive && dbId !== null,
    refetchInterval: (query) => (['pending', 'rendering'].includes(query.state.data?.status ?? '') ? 3000 : false),
    retry: false,
  });
  const render = useMutation({
    mutationFn: () => requestPreview(dbId ?? 0),
    onSuccess: () => {
      setPreviewActive(true);
      void queryClient.invalidateQueries({ queryKey: previewKey });
    },
  });
  const contest = useMutation({ mutationFn: () => submitClipToContest(dbId ?? 0) });
  const archive = useMutation({
    mutationFn: () => discardClip(dbId ?? 0),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['social-media', 'clips'] });
      onArchived?.();
    },
  });
  const heldJob = owner ? source.jobs.find((job) => job.status === 'review') : undefined;
  const queuedJob = owner ? source.jobs.find((job) => job.status === 'queued') : undefined;
  const resolve = useMutation({
    mutationFn: ({ id, action, note }: { id: number; action: ReviewDecision; note: string }) => scheduleQueueApi.resolve(id, action, note, owner),
    onSuccess: onQueueChanged,
  });
  const cancel = useMutation({
    mutationFn: (id: number) => scheduleQueueApi.cancel(id, owner),
    onSuccess: onQueueChanged,
  });
  const holdDetails = heldJob ? queueJobs.get(heldJob.id) : undefined;
  const ready = preview.data?.status === 'ready';
  const rendering = render.isPending || ['pending', 'rendering'].includes(preview.data?.status ?? '');
  const archived = stage === 'archive';
  const noFile = dbId === null;
  const published = publishedPlatforms(source);
  const notes = [
    eligibilityNote(source, t),
    source.legacy_reserved && t('Für bestehende Verarbeitung reserviert'),
    outsideWindow && stage === 'new' && t('Außerhalb des gespeicherten Zeitraums'),
    source.processed_at && `${t('Verarbeitet am')}: ${localTimestamp(source.processed_at, timezone, locale)}`,
    source.published_at && `${t('Veröffentlicht am')}: ${localTimestamp(source.published_at, timezone, locale)}`,
    published.length > 0 && `${t('Gepostet auf')}: ${published.map((item) => PLATFORM_LABELS[item] ?? item).join(', ')}`,
    noFile && t('Verlauf erhalten. Eine Quelldatei für die Verarbeitung ist nicht nachgewiesen.'),
    ...legacyUncertainPlatforms(source).map((platform) => t('Ob der Upload auf {platform} angekommen ist, ist unklar. Er wird nicht automatisch wiederholt, damit nichts doppelt erscheint. Bitte prüfe dein Konto.', { platform: PLATFORM_LABELS[platform] ?? platform })),
  ].filter((note): note is string => Boolean(note));
  const detailItems: Array<{ mode: DetailMode; label: string; icon: typeof Film }> = [
    { mode: 'ai', label: 'Mit KI bearbeiten', icon: Clapperboard },
    { mode: 'layout', label: 'Layout anpassen', icon: Crop },
    { mode: 'enrichment', label: 'Titel, Beschreibung & Hashtags', icon: Pencil },
  ];

  return (
    <li
      data-clip-id={source.clip_id}
      data-selected={selected ? 'true' : undefined}
      data-stage={stage}
      className="import-pool-clip"
    >
      <div className="import-pool-thumb">
        {ready && dbId !== null ? (
          <video controls preload="metadata" src={previewFileUrl(dbId)} aria-label={metadata.title} />
        ) : thumbnail ? (
          <img src={thumbnail} alt="" loading="lazy" />
        ) : (
          <Film size={28} aria-hidden="true" />
        )}
        {source.pooled && source.eligibility === 'automatic' && (
          <label className="import-pool-pick">
            <input
              type="checkbox"
              checked={selected}
              disabled={selectionLocked || !canSelect}
              aria-label={`${t('Vorziehen')}: ${metadata.title}`}
              title={t('Auto-Pilot nimmt diesen Clip zuerst')}
              onChange={(event) => onSelect(source.clip_id, event.target.checked)}
            />
            <span>{t('Vorziehen')}</span>
          </label>
        )}
        {metadata.duration !== null && !ready && <span className="import-pool-duration">{clipDuration(metadata.duration)}</span>}
      </div>
      <div className="import-pool-clip-body">
        <h4 title={`${t('Clip-ID')}: ${source.clip_id}`}>{metadata.title}</h4>
        <p className="import-pool-help">
          {localTimestamp(source.source_created_at, timezone, locale)}
          {typeof source.view_count === 'number' && ` · ${source.view_count.toLocaleString(locale)} ${t('Aufrufe')}`}
        </p>
        <div className="import-pool-clip-foot">
          <span className="pipeline-clips-badges">
            <span className="import-pool-chip" data-tone={STAGE_TONES[stage]}>
              {t(QUEUE_FILTERS.find((item) => item.id === stage)?.label ?? 'Neu')}
            </span>
            {partialResult(source) && (
              <span className="import-pool-chip" data-tone="warning">{t('Teilweise veröffentlicht')}</span>
            )}
          </span>
          <span className="pipeline-clips-actions">
            <button
              type="button"
              className="studio-icon-button"
              aria-label={`${t('Archivieren')}: ${metadata.title}`}
              title={t('Archivieren')}
              disabled={noFile || stage !== 'new' || archive.isPending}
              onClick={() => {
                if (window.confirm(t('Clip „{title}“ archivieren? Er wird danach nicht mehr automatisch gepostet.', { title: metadata.title }))) archive.mutate();
              }}
            >
              <Archive size={15} aria-hidden="true" />
            </button>
            <ClipActionMenu>
              {(run) => (
                <>
                  {detailItems.map(({ mode, label, icon: Icon }) => (
                    <button
                      key={mode}
                      type="button"
                      disabled={!detailReady || noFile || archived}
                      title={detailReady ? undefined : t('Kommt mit der neuen Clip-Ansicht')}
                      onClick={() => run(() => onOpenDetail?.(source, mode))}
                      className={CLIP_MENU_ITEM}
                    >
                      <Icon className="h-4 w-4" />
                      {t(label)}
                    </button>
                  ))}
                  <button
                    type="button"
                    disabled={noFile || archived || preview.isFetching}
                    onClick={() => run(() => { setPreviewActive(true); if (previewActive) void preview.refetch(); })}
                    className={CLIP_MENU_ITEM}
                  >
                    <Film className="h-4 w-4" />
                    {t('Vorschau prüfen')}
                  </button>
                  <button
                    type="button"
                    disabled={noFile || archived || rendering}
                    onClick={() => run(() => render.mutate())}
                    className={CLIP_MENU_ITEM}
                  >
                    <Clapperboard className="h-4 w-4" />
                    {ready ? t('Vorschau neu rendern') : t('Vorschau rendern')}
                  </button>
                  {source.eligibility !== 'manual' && (
                    <button
                      type="button"
                      disabled={noFile || contest.isPending || contest.data?.status === 'accepted'}
                      onClick={() => run(() => contest.mutate())}
                      className={CLIP_MENU_ITEM}
                    >
                      <Trophy className="h-4 w-4" />
                      {t('Für Clip-Contest einreichen')}
                    </button>
                  )}
                  {queuedJob && (
                    <button
                      type="button"
                      disabled={cancel.isPending}
                      onClick={() => run(() => {
                        if (window.confirm(t('Diesen Auftrag entfernen? Der Clip wird nicht automatisch erneut ausgewählt.'))) cancel.mutate(queuedJob.id);
                      })}
                      className={CLIP_MENU_ITEM}
                    >
                      <ListX className="h-4 w-4" />
                      {t('Aus der Warteschlange entfernen')}
                    </button>
                  )}
                  {url && (
                    <a href={url} target="_blank" rel="noopener noreferrer" className={CLIP_MENU_ITEM}>
                      <ExternalLink className="h-4 w-4" />
                      {t('Original ansehen')}
                    </a>
                  )}
                </>
              )}
            </ClipActionMenu>
          </span>
        </div>
        {notes.length > 0 && (
          <ul className="import-pool-notes">
            {notes.map((note) => <li key={note}>{note}</li>)}
          </ul>
        )}
        {heldJob && (
          <div className="schedule-queue pipeline-clips-hold">
            {(holdDetails?.review_reasons.length ? holdDetails.review_reasons : ['']).map((reason) => (
              <p key={reason} className="import-pool-help">{t(HOLD_REASONS[reason] ?? 'Die Produktion benötigt eine Prüfung.')}</p>
            ))}
            {holdDetails?.review_explanation && <p className="import-pool-help">{holdDetails.review_explanation}</p>}
            <ReviewJobActions
              busy={resolve.isPending}
              clipTitle={metadata.title}
              onResolve={(action, note) => resolve.mutate({ id: heldJob.id, action, note })}
            />
          </div>
        )}
        {(resolve.isError || cancel.isError) && (
          <p role="alert" className="import-pool-error-text">{t('Die Änderung konnte nicht gespeichert werden. Bitte versuche es erneut.')}</p>
        )}
        {previewActive && preview.data?.status === null && (
          <p role="status" className="import-pool-help">{t('Noch keine Vorschau vorhanden. Über „Weitere Aktionen“ kannst du sie rendern.')}</p>
        )}
        {rendering && <p role="status" className="import-pool-help">{t('Vorschau wird gerendert…')}</p>}
        {(render.isError || preview.isError || preview.data?.status === 'error') && (
          <p role="alert" className="import-pool-error-text">{t('Vorschau konnte nicht gerendert werden.')}</p>
        )}
        {contest.data && (
          <p role={contest.data.ok ? 'status' : 'alert'} className="import-pool-help">{t(contest.data.message)}</p>
        )}
        {(contest.isError || archive.isError) && (
          <p role="alert" className="import-pool-error-text">{t('Das klappt gerade nicht. Versuch es gleich nochmal.')}</p>
        )}
      </div>
    </li>
  );
}
