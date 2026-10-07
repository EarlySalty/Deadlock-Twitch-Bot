import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Archive, ExternalLink, Loader2, CircleCheck, HardDrive, Download, Clock, CircleDashed, CircleX, TriangleAlert } from 'lucide-react';
import { archiveAction, fetchArchivedVods, fetchVodArchiveSettings, saveVodArchiveSettings, oauthStartUrl, type ArchivedVod } from '@/api/socialMedia';
import { useLanguage } from '@/context/LanguageContext';

const statusAppearance = {
  youtube_uploaded: { icon: CircleCheck, color: 'text-success' },
  drive_uploaded: { icon: HardDrive, color: 'text-success' },
  uploading: { icon: Loader2, color: 'text-primary' },
  downloading: { icon: Download, color: 'text-primary' },
  waiting: { icon: Clock, color: 'text-text-secondary' },
  partial: { icon: CircleDashed, color: 'text-warning' },
  failed: { icon: CircleX, color: 'text-danger' },
  unknown: { icon: TriangleAlert, color: 'text-warning' },
};

export function VodArchiveEntry({ vod, pending, onAction }: {
  vod: ArchivedVod;
  pending: boolean;
  onAction: (vod: ArchivedVod, kind: 'retry' | 'drive' | 'hide') => void;
}) {
  const { t, locale } = useLanguage();
  const finished = vod.youtube_complete || vod.drive_complete;
  const appearance = statusAppearance[vod.display_status] ?? statusAppearance.unknown;
  const StatusIcon = appearance.icon;
  const date = (value: string | null) => {
    if (!value) return null;
    const parsed = new Date(value.length === 10 ? `${value}T12:00:00` : value);
    return Number.isNaN(parsed.getTime()) ? null : parsed.toLocaleString(locale, { dateStyle: 'medium', ...(value.includes('T') ? { timeStyle: 'short' as const } : {}) });
  };
  const completedAt = finished ? date(vod.uploaded_at) : null;
  const attemptedAt = date(vod.last_attempt_at);
  return <article className="min-w-0 p-4 sm:p-5">
    <h3 className="break-words text-base font-semibold text-text-primary">{vod.title}</h3>
    <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-2">
      <span className={`inline-flex items-start gap-2 text-sm font-semibold ${appearance.color}`}>
        <StatusIcon aria-hidden="true" className={`mt-0.5 h-4 w-4 shrink-0 ${vod.display_status === 'uploading' ? 'motion-safe:animate-spin' : ''}`} />
        {t(vod.status_label)}
      </span>
      {vod.total_parts > 1 && !vod.drive_complete && <span className="text-sm text-text-secondary">{t('YouTube: {done} von {total} Teilen bestätigt', { done: vod.confirmed_parts, total: vod.total_parts })}</span>}
    </div>
    <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-sm text-text-secondary">
      <span className="break-all">{vod.channel}</span>
      <span>{date(vod.recorded_at) ?? t('Aufnahmedatum unbekannt')}</span>
      <span className="tabular-nums">{Math.floor(vod.duration_sec / 3600)}:{String(Math.floor(vod.duration_sec % 3600 / 60)).padStart(2, '0')} h</span>
      {completedAt ? <time dateTime={vod.uploaded_at!}>{t('Abgeschlossen: {date}', { date: completedAt })}</time> : attemptedAt && <time dateTime={vod.last_attempt_at!}>{t('Letzter Versuch: {date}', { date: attemptedAt })}</time>}
    </div>
    {vod.reason && <p className={`mt-3 max-w-prose text-sm ${vod.display_status === 'unknown' ? 'text-warning' : 'text-danger'}`}>{t(vod.reason)}</p>}
    <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
      {vod.parts.map((part) => part.youtube_video_id?.trim() && <a key={part.index} className="inline-flex min-h-9 items-center gap-1 text-primary underline underline-offset-4" href={`https://www.youtube.com/watch?v=${encodeURIComponent(part.youtube_video_id)}`} target="_blank" rel="noopener noreferrer">
        {vod.parts.length === 1 ? t('Auf YouTube öffnen') : t('YouTube: Teil {part}', { part: part.index + 1 })}
        {part.status !== 'done' && <span>({t('unbestätigt')})</span>}
        <ExternalLink aria-hidden="true" className="h-3 w-3 shrink-0" />
      </a>)}
      {vod.drive_url?.startsWith('https://drive.google.com/') && <a className="inline-flex min-h-9 items-center gap-1 text-primary underline underline-offset-4" href={vod.drive_url} target="_blank" rel="noopener noreferrer">{t('Auf Drive öffnen')}{!vod.drive_complete && <span>({t('unbestätigt')})</span>}<ExternalLink aria-hidden="true" className="h-3 w-3 shrink-0" /></a>}
    </div>
    {vod.needs_connection && vod.can_retry && !vod.drive_requested && vod.twitch_user_id && <a className="studio-button mt-3" href={oauthStartUrl('youtube', vod.twitch_user_id)}>{t('YouTube neu verbinden')}</a>}
    {vod.drive_requested && !finished && <p className="mt-3 text-sm text-text-secondary">{t(vod.display_status === 'uploading' ? 'Drive-Sicherung läuft.' : 'Drive-Sicherung vorgemerkt.')}</p>}
    <div className="mt-3 flex flex-wrap items-center gap-3">
      {vod.can_retry && <><button type="button" className="studio-button" disabled={pending} onClick={() => onAction(vod, 'retry')}>{t('Erneut versuchen')}</button><button type="button" className="studio-button" disabled={pending} onClick={() => onAction(vod, 'drive')}>{t('Auf Drive ausweichen')}</button></>}
      <button type="button" className="min-h-11 rounded-md px-2 text-sm text-text-secondary underline underline-offset-4 hover:text-text-primary focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-50" disabled={pending} onClick={() => onAction(vod, 'hide')}>{t('Aus der Liste ausblenden')}</button>
    </div>
  </article>;
}

export function VodArchiveTab({ twitchUserId, isAdmin }: { twitchUserId?: string; isAdmin: boolean }) {
  const { t } = useLanguage();
  const client = useQueryClient();
  const [allChannels, setAllChannels] = useState(isAdmin && !twitchUserId);
  const [page, setPage] = useState(1);
  const [notice, setNotice] = useState('');
  const scope = allChannels && isAdmin ? undefined : twitchUserId;
  const list = useQuery({
    queryKey: ['vod-archive', scope, page],
    queryFn: () => fetchArchivedVods(scope, page),
    enabled: isAdmin || !!twitchUserId,
    refetchInterval: 30_000,
    retry: false,
  });
  useEffect(() => {
    setPage(1);
  }, [scope]);
  useEffect(() => {
    if (list.data) {
      const lastPage = Math.max(1, Math.ceil(list.data.total / 50));
      setPage((current) => Math.min(current, lastPage));
    }
  }, [list.data]);
  const settings = useQuery({
    queryKey: ['social-media', 'vod-archive-settings', twitchUserId],
    queryFn: () => fetchVodArchiveSettings(twitchUserId),
    enabled: !!twitchUserId,
    retry: false,
  });
  const toggle = useMutation({
    mutationFn: () => saveVodArchiveSettings(twitchUserId, { enabled: !settings.data!.enabled, privacy: settings.data!.privacy }),
    onSuccess: (data) => client.setQueryData(['social-media', 'vod-archive-settings', twitchUserId], data),
  });
  const action = useMutation({
    mutationFn: ({ vod, kind }: { vod: ArchivedVod; kind: 'retry' | 'drive' | 'hide' }) => archiveAction(vod.id, kind, vod.twitch_user_id ?? undefined),
    onSuccess: (_, { kind }) => {
      setNotice(kind === 'hide' ? t('VOD ausgeblendet. Die Sicherung bleibt erhalten.') : t('Vorgemerkt für den nächsten Archivlauf.'));
      client.invalidateQueries({ queryKey: ['vod-archive'] });
    },
  });
  const act = (vod: ArchivedVod, kind: 'retry' | 'drive' | 'hide') => {
    if (kind === 'drive' && !window.confirm(t('Dieses VOD auf Drive sichern? Für die Kopie wird ein teilbarer Link erstellt.'))) return;
    action.mutate({ vod, kind });
  };
  return <section className="space-y-5" aria-labelledby="vod-archive-title">
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div>
        <h2 id="vod-archive-title" className="studio-heading flex items-center gap-2"><Archive className="h-5 w-5" />{t('VOD-Archiv')}</h2>
        <p className="mt-2 max-w-prose text-sm text-text-secondary">{t('Hier findest du deine gesicherten Streams und offene Uploads. Nach bestätigtem Upload werden die temporären Dateien gelöscht.')}</p>
        <p className="mt-2 max-w-prose text-sm text-text-secondary">{t('Abschlüsse beruhen auf gespeicherten Uploadbestätigungen. Die heutige Verfügbarkeit und Sichtbarkeit prüfst du über die Ziellinks.')}</p>
      </div>
      {twitchUserId && <button type="button" role="switch" aria-checked={settings.data?.enabled ?? false} disabled={!settings.data || toggle.isPending} className="studio-button" onClick={() => toggle.mutate()}>
        {settings.isPending ? t('Einstellung wird geladen…') : settings.data?.enabled ? t('VOD-Archiv: an') : t('VOD-Archiv: aus')}
      </button>}
    </div>
    {isAdmin && <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={allChannels} onChange={(event) => { setAllChannels(event.target.checked); setPage(1); }} />{t('Alle Kanäle anzeigen')}</label>}
    {(settings.isError || toggle.isError) && <p role="alert" className="text-sm text-danger">{t('Die Archiveinstellung konnte nicht geladen oder gespeichert werden. Bitte versuche es erneut.')}</p>}
    {notice && <p role="status" className="text-sm text-success">{notice}</p>}
    {action.isError && <p role="alert" className="text-sm text-danger">{t('Die Aktion konnte nicht ausgeführt werden. Das VOD wird gerade bearbeitet oder ist bereits fertig. Lade die Liste erneut.')}</p>}
    {list.isPending ? <p role="status" className="flex items-center gap-2 text-sm text-text-secondary"><Loader2 className="h-4 w-4 animate-spin" />{t('VODs werden geladen…')}</p> : list.isError ? <div role="alert"><p>{t('Die VOD-Liste konnte nicht geladen werden.')}</p><button className="studio-button mt-3" onClick={() => list.refetch()}>{t('Erneut laden')}</button></div> : list.data?.items.length === 0 ? <p className="rounded-xl border border-border p-6 text-text-secondary">{t('Hier stehen noch keine VODs. Schalte das Archiv für deinen Kanal ein, damit neue Streams gesichert werden.')}</p> : <div className="divide-y divide-border rounded-xl border border-border">
      {list.data?.items.map((vod) => <VodArchiveEntry key={vod.id} vod={vod} pending={action.isPending} onAction={act} />)}
    </div>}
    {list.data && (page > 1 || list.data.total > 50) && <div className="flex flex-wrap items-center justify-between gap-3"><button className="studio-button" disabled={page <= 1} onClick={() => setPage(page - 1)}>{t('Zurück')}</button><span className="text-sm text-text-secondary">{t('Seite {page}', { page })}</span><button className="studio-button" disabled={page * 50 >= list.data.total} onClick={() => setPage(page + 1)}>{t('Weiter')}</button></div>}
  </section>;
}
