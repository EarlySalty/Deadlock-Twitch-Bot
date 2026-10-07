import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Archive, ExternalLink, Loader2 } from 'lucide-react';
import { archiveAction, fetchArchivedVods, fetchVodArchiveSettings, saveVodArchiveSettings, oauthStartUrl, type ArchivedVod } from '@/api/socialMedia';
import { useLanguage } from '@/context/LanguageContext';

export function VodArchiveTab({ twitchUserId, isAdmin }: { twitchUserId?: string; isAdmin: boolean }) {
  const { t, locale } = useLanguage();
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
  const date = (value: string | null) => value ? new Date(value).toLocaleString(locale, { dateStyle: 'medium', ...(value.includes('T') ? { timeStyle: 'short' as const } : {}) }) : t('Noch kein Versuch');
  const finished = (vod: ArchivedVod) => ['uploaded', 'archived', 'drive_uploaded'].includes(vod.status);
  const act = (vod: ArchivedVod, kind: 'retry' | 'drive' | 'hide') => {
    if (kind === 'drive' && !window.confirm(t('Dieses VOD auf Drive sichern? Für die Kopie wird ein teilbarer Link erstellt.'))) return;
    action.mutate({ vod, kind });
  };
  return <section className="space-y-5" aria-labelledby="vod-archive-title">
    <div className="flex flex-wrap items-start justify-between gap-4">
      <div>
        <h2 id="vod-archive-title" className="studio-heading flex items-center gap-2"><Archive className="h-5 w-5" />{t('VOD-Archiv')}</h2>
        <p className="mt-2 max-w-prose text-sm text-text-secondary">{t('Hier findest du deine gesicherten Streams und offene Uploads. Nach bestätigtem Upload werden die temporären Dateien gelöscht.')}</p>
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
      {list.data?.items.map((vod) => <article key={vod.id} className="p-4 sm:p-5">
        <div className="flex flex-col items-start justify-between gap-3 sm:flex-row">
          <div className="min-w-0 flex-1">
            <p className="text-sm text-text-secondary">{vod.channel} · {vod.recorded_at ? date(vod.recorded_at) : t('Aufnahmedatum unbekannt')} · {Math.floor(vod.duration_sec / 3600)}:{String(Math.floor(vod.duration_sec % 3600 / 60)).padStart(2, '0')} h</p>
            <h3 className="mt-1 break-words text-base font-semibold text-text-primary">{vod.title}</h3>
          </div>
          <span className={`shrink-0 rounded-md border border-border px-2 py-1 text-sm ${vod.reason ? 'text-danger' : finished(vod) ? 'text-success' : 'text-text-secondary'}`}>{t(vod.status_label)}</span>
        </div>
        {vod.reason && <p className="mt-3 text-sm text-danger">{t(vod.reason)}</p>}
        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
          {vod.parts.map((part) => part.youtube_video_id && <a key={part.index} className="inline-flex items-center gap-1 text-primary underline underline-offset-4" href={`https://www.youtube.com/watch?v=${encodeURIComponent(part.youtube_video_id)}`} target="_blank" rel="noopener noreferrer">{t('YouTube: Teil {part}', { part: part.index + 1 })}<ExternalLink className="h-3 w-3" /></a>)}
          {vod.drive_url?.startsWith('https://drive.google.com/') && <a className="inline-flex items-center gap-1 text-primary underline underline-offset-4" href={vod.drive_url} target="_blank" rel="noopener noreferrer">{t('Auf Drive öffnen')}<ExternalLink className="h-3 w-3" /></a>}
          <span className="text-text-secondary">{t('{done} von {total} Teilen', { done: vod.status === 'drive_uploaded' ? vod.parts.length : vod.parts.filter((part) => part.status === 'done').length, total: vod.parts.length })}</span>
          <span className="text-text-secondary">{t('Letzter Versuch: {date}', { date: date(vod.last_attempt_at) })}</span>
        </div>
        {vod.needs_connection && !finished(vod) && !vod.drive_requested && vod.twitch_user_id && <a className="studio-button mt-3" href={oauthStartUrl('youtube', vod.twitch_user_id)}>{t('YouTube neu verbinden')}</a>}
        {vod.drive_requested && !finished(vod) && <p className="mt-3 text-sm text-text-secondary">{t('Drive-Sicherung vorgemerkt.')}</p>}
        <div className="mt-4 flex flex-wrap gap-2">
          {!finished(vod) && <><button className="studio-button" disabled={action.isPending} onClick={() => act(vod, 'retry')}>{t('Erneut versuchen')}</button><button className="studio-button" disabled={action.isPending} onClick={() => act(vod, 'drive')}>{t('Auf Drive ausweichen')}</button></>}
          <button className="studio-button" disabled={action.isPending} onClick={() => act(vod, 'hide')}>{t('Aus der Liste ausblenden')}</button>
        </div>
      </article>)}
    </div>}
    {list.data && (page > 1 || list.data.total > 50) && <div className="flex flex-wrap items-center justify-between gap-3"><button className="studio-button" disabled={page <= 1} onClick={() => setPage(page - 1)}>{t('Zurück')}</button><span className="text-sm text-text-secondary">{t('Seite {page}', { page })}</span><button className="studio-button" disabled={page * 50 >= list.data.total} onClick={() => setPage(page + 1)}>{t('Weiter')}</button></div>}
  </section>;
}
