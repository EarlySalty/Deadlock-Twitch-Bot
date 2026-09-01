import { useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertCircle,
  CheckCircle2,
  Circle,
  Download,
  Loader2,
  PlayCircle,
  RefreshCw,
  Wand2,
} from 'lucide-react';

import {
  clipPreparationMediaUrl,
  fetchClipPreparation,
  requestClipPreparation,
} from '../../api/socialMedia';
import { useT } from '../../context/LanguageContext';
import type { ClipPreparation } from '../../types/socialMedia';
import { fehlerText } from './labels';
import {
  CLIP_VORBEREITUNG_POLL_MS,
  istVorbereitungAktiv,
  rueckmeldungNachVorbereitungsfehler,
  sichereVorbereitungVorMutation,
  vorbereitungFehler,
  vorbereitungSchritte,
  type VorbereitungSchrittStatus,
} from './clipVorbereitung';

interface ClipPreparationWorkbenchProps {
  clipDbId: number;
  clipTitle: string;
  onPreparationChange?: (preparation: ClipPreparation | null) => void;
}

const SCHRITT_TON: Record<VorbereitungSchrittStatus, string> = {
  wartet: 'border-border bg-bg/35 text-text-secondary',
  aktiv: 'border-primary/45 bg-primary/12 text-primary',
  fertig: 'border-success/35 bg-success/10 text-success',
  fehler: 'border-danger/40 bg-danger/10 text-danger',
};

const SCHRITT_STATUS_TEXT: Record<VorbereitungSchrittStatus, string> = {
  wartet: 'Wartet',
  aktiv: 'Läuft',
  fertig: 'Fertig',
  fehler: 'Fehler',
};

const VORBEREITUNG_STATUS_TEXT: Record<ClipPreparation['state'], string> = {
  pending: 'Aufbereitung wartet auf den Start.',
  materializing: 'Quellvideo wird vorbereitet.',
  source_ready: 'Quellvideo ist bereit.',
  rendering: 'Hochformat-Vorschau wird gerendert.',
  preview_ready: 'Die prüfbare Vorschau ist bereit.',
  failed: 'Die Aufbereitung ist fehlgeschlagen.',
};

function SchrittIcon({ status }: { status: VorbereitungSchrittStatus }) {
  if (status === 'aktiv') return <Loader2 className="h-3.5 w-3.5 animate-spin" />;
  if (status === 'fertig') return <CheckCircle2 className="h-3.5 w-3.5" />;
  if (status === 'fehler') return <AlertCircle className="h-3.5 w-3.5" />;
  return <Circle className="h-3.5 w-3.5" />;
}

function mitMedienFallback(
  preparation: ClipPreparation,
  clipDbId: number,
): ClipPreparation {
  if (!preparation.preview_ready || (preparation.preview_url && preparation.download_url)) {
    return preparation;
  }
  return {
    ...preparation,
    preview_url: preparation.preview_url ?? clipPreparationMediaUrl(clipDbId),
    download_url: preparation.download_url ?? clipPreparationMediaUrl(clipDbId, true),
  };
}

export function ClipPreparationWorkbench({
  clipDbId,
  clipTitle,
  onPreparationChange,
}: ClipPreparationWorkbenchProps) {
  const t = useT();
  const queryClient = useQueryClient();
  const queryKey = ['social-media', 'clip-preparation', clipDbId] as const;
  const containerRef = useRef<HTMLElement | null>(null);
  const observerVerfuegbar = typeof IntersectionObserver !== 'undefined';
  const [sichtbar, setSichtbar] = useState(!observerVerfuegbar);
  const [warSichtbar, setWarSichtbar] = useState(!observerVerfuegbar);
  const [videoFehler, setVideoFehler] = useState(false);
  const bereichsLabel = t('Clip-Aufbereitung für {title}', { title: clipTitle });
  const untertitelHinweisId = `clip-${clipDbId}-untertitel-hinweis`;

  useEffect(() => {
    if (!observerVerfuegbar || !containerRef.current) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        const jetztSichtbar = entry.isIntersecting;
        setSichtbar(jetztSichtbar);
        if (jetztSichtbar) setWarSichtbar(true);
      },
      { rootMargin: '500px 0px' },
    );
    observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, [observerVerfuegbar]);

  const preparationQuery = useQuery<ClipPreparation, Error>({
    queryKey,
    queryFn: ({ signal }) => fetchClipPreparation(clipDbId, signal),
    enabled: warSichtbar,
    staleTime: 15_000,
    refetchOnWindowFocus: false,
    refetchInterval: (query) => {
      const preparation = query.state.data;
      return sichtbar && preparation && istVorbereitungAktiv(preparation.state)
        ? CLIP_VORBEREITUNG_POLL_MS
        : false;
    },
  });

  const preparationMitMedien = useMemo(
    () =>
      preparationQuery.data
        ? mitMedienFallback(preparationQuery.data, clipDbId)
        : null,
    [clipDbId, preparationQuery.data],
  );

  const preparationMutation = useMutation({
    mutationFn: () => requestClipPreparation(clipDbId),
    onMutate: () => {
      const vorherigeVorbereitung = preparationQuery.data
        ? mitMedienFallback(preparationQuery.data, clipDbId)
        : null;
      onPreparationChange?.(null);
      return {
        vorherigeVorbereitung: sichereVorbereitungVorMutation(
          vorherigeVorbereitung,
          vorherigeVorbereitung?.preview_ready === true &&
            Boolean(vorherigeVorbereitung.preview_url) &&
            !videoFehler,
        ),
      };
    },
    onSuccess: (preparation) => {
      queryClient.setQueryData(queryKey, preparation);
    },
    onError: (_error, _variables, context) => {
      onPreparationChange?.(
        rueckmeldungNachVorbereitungsfehler(context?.vorherigeVorbereitung ?? null),
      );
    },
  });

  useEffect(() => {
    setVideoFehler(false);
  }, [preparationQuery.data?.preview_url, preparationQuery.data?.updated_at]);

  useEffect(() => {
    onPreparationChange?.(
      preparationQuery.isError || videoFehler ? null : preparationMitMedien,
    );
  }, [onPreparationChange, preparationMitMedien, preparationQuery.isError, videoFehler]);

  if (!warSichtbar) {
    return (
      <section
        ref={containerRef}
        aria-label={bereichsLabel}
        className="h-16 rounded-xl border border-border bg-bg/25"
      >
        <span className="sr-only">{t('Aufbereitungsstand wird bei Sichtbarkeit geladen.')}</span>
      </section>
    );
  }

  if (preparationQuery.isLoading) {
    return (
      <section
        ref={containerRef}
        aria-label={bereichsLabel}
        aria-busy="true"
        className="flex items-center gap-2 rounded-xl border border-border bg-bg/30 px-3 py-2.5 text-xs text-text-secondary"
      >
        <Loader2 className="h-4 w-4 animate-spin text-primary" />
        {t('Aufbereitungsstand wird geladen…')}
      </section>
    );
  }

  if (preparationQuery.isError || !preparationQuery.data) {
    return (
      <section
        ref={containerRef}
        role="alert"
        aria-label={bereichsLabel}
        className="rounded-xl border border-danger/35 bg-danger/10 p-3 text-xs text-danger"
      >
        <div className="flex items-start gap-2">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div className="min-w-0 flex-1">
            <p className="font-semibold">{t('Aufbereitungsstand konnte nicht geladen werden.')}</p>
            <p className="mt-1 text-text-secondary">
              {fehlerText(preparationQuery.error, t)}
            </p>
            <button
              type="button"
              onClick={() => preparationQuery.refetch()}
              disabled={preparationQuery.isFetching}
              className="mt-2 inline-flex items-center gap-1.5 rounded-lg border border-danger/35 px-2.5 py-1.5 font-bold hover:bg-danger/10 disabled:opacity-50"
            >
              {preparationQuery.isFetching ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw className="h-3.5 w-3.5" />
              )}
              {t('Erneut versuchen')}
            </button>
          </div>
        </div>
      </section>
    );
  }

  const preparation = preparationMitMedien ?? preparationQuery.data;
  const schritte = vorbereitungSchritte(preparation);
  const aktiv = istVorbereitungAktiv(preparation.state) || preparationMutation.isPending;
  const vorschauBereit =
    preparation.preview_ready && Boolean(preparation.preview_url) && !videoFehler;
  const aktionsLabel = preparation.preview_ready
    ? t('Neu rendern')
    : preparation.state === 'failed'
      ? t('Erneut versuchen')
      : t('Clip aufbereiten');
  const aktionsFehler = fehlerText(preparationMutation.error, t);

  return (
    <section
      ref={containerRef}
      aria-label={bereichsLabel}
      aria-busy={aktiv}
      className="rounded-xl border border-primary/20 bg-[linear-gradient(145deg,rgba(197,160,89,0.08),rgba(10,10,12,0.3))] p-3 space-y-3"
    >
      <p className="sr-only" aria-live="polite">
        {t('{title}: {status}', {
          title: clipTitle,
          status: t(VORBEREITUNG_STATUS_TEXT[preparation.state]),
        })}
      </p>
      <div className="flex items-center justify-between gap-3">
        <div className="inline-flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-[0.14em] text-primary">
          <Wand2 className="h-3.5 w-3.5" /> {t('Aufbereitung')}
        </div>
        {preparationQuery.isFetching && !aktiv && (
          <Loader2 className="h-3.5 w-3.5 animate-spin text-text-secondary" />
        )}
      </div>

      <ol className="grid grid-cols-3 gap-1.5" aria-label={t('Aufbereitungsfortschritt')}>
        {schritte.map((schritt) => (
          <li
            key={schritt.id}
            aria-current={schritt.status === 'aktiv' ? 'step' : undefined}
            aria-label={`${t(schritt.label)}: ${t(SCHRITT_STATUS_TEXT[schritt.status])}`}
            className={`flex min-w-0 items-center justify-center gap-1 rounded-lg border px-1.5 py-2 text-[11px] font-bold ${SCHRITT_TON[schritt.status]}`}
          >
            <SchrittIcon status={schritt.status} />
            <span className="truncate">{t(schritt.label)}</span>
          </li>
        ))}
      </ol>

      {preparation.state === 'failed' && (
        <div role="alert" className="flex items-start gap-2 rounded-lg border border-danger/35 bg-danger/10 p-2.5 text-xs text-danger">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <span>{t(vorbereitungFehler(preparation))}</span>
        </div>
      )}

      {videoFehler && (
        <div role="alert" className="text-xs text-danger">
          {t('Die gerenderte Vorschau konnte nicht abgespielt werden. Bitte neu rendern.')}
        </div>
      )}

      {vorschauBereit && (
        <details className="group rounded-lg border border-border bg-black/20">
          <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-xs font-bold text-white">
            <PlayCircle className="h-4 w-4 text-success" />
            {t('Gerenderte 9:16-Vorschau prüfen')}
            <span className="ml-auto text-[10px] text-text-secondary group-open:hidden">
              {t('Öffnen')}
            </span>
          </summary>
          <div className="border-t border-border p-3">
            <video
              src={preparation.preview_url ?? undefined}
              controls
              playsInline
              preload="metadata"
              onError={() => setVideoFehler(true)}
              onLoadedMetadata={() => setVideoFehler(false)}
              aria-label={t('Gerenderte Vorschau für {title}', { title: clipTitle })}
              aria-describedby={untertitelHinweisId}
              className="mx-auto max-h-[28rem] w-auto max-w-full rounded-lg bg-black shadow-lg"
            />
            <p id={untertitelHinweisId} className="mt-2 text-[11px] text-warning">
              {t('Für diese Vorschau stehen noch keine synchronisierten Untertitel bereit.')}
            </p>
          </div>
        </details>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => preparationMutation.mutate()}
          disabled={aktiv}
          className="inline-flex items-center gap-1.5 rounded-lg border border-primary/35 bg-primary/12 px-3 py-2 text-xs font-bold text-primary hover:bg-primary/20 disabled:cursor-wait disabled:opacity-55"
        >
          {aktiv ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          {aktiv ? t('Aufbereitung läuft…') : aktionsLabel}
        </button>

        {preparation.download_url && (
          <a
            href={preparation.download_url}
            download
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1.5 rounded-lg border border-border px-3 py-2 text-xs font-bold text-text-secondary hover:border-border-hover hover:text-white"
          >
            <Download className="h-3.5 w-3.5" /> {t('MP4 herunterladen')}
          </a>
        )}
      </div>

      {aktionsFehler && (
        <div role="alert" className="text-xs text-danger">
          {aktionsFehler}
        </div>
      )}
    </section>
  );
}
