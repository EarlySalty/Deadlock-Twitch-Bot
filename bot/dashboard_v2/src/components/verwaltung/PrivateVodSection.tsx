import { useEffect, useRef, useState } from 'react';
import { Download, Loader2, Power, PowerOff, RefreshCw } from 'lucide-react';
import {
  fetchPrivateVodClips,
  privateVodDownloadUrl,
  setPrivateVodCommand,
  type PrivateVodClipsResponse,
  type PrivateVodJob,
} from '@/api/clipCommand';
import { EditableCommandName } from './CommandNameSection';

function timestamp(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor(seconds / 60) % 60;
  const rest = String(seconds % 60).padStart(2, '0');
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${rest}` : `${minutes}:${rest}`;
}

function jobStatus(job: PrivateVodJob, now: number): string {
  if (Date.parse(job.expires_at) <= now || job.status === 'expired') return 'Abgelaufen';
  if (job.status === 'queued') return 'Wartet auf Verarbeitung';
  if (job.status === 'running') return 'Wird vorbereitet';
  if (job.status === 'ready') return 'Bereit zum Download';
  if (job.error_code === 'unavailable') return 'Aufzeichnung nicht verfügbar';
  if (job.error_code === 'bounds') return 'Bereich noch nicht verfügbar';
  if (job.error_code === 'timeout') return 'Zeitlimit erreicht. Bitte später erneut anfordern.';
  if (job.error_code === 'interrupted') return 'Vorbereitung unterbrochen. Bitte erneut anfordern.';
  return 'Vorbereitung fehlgeschlagen. Bitte später erneut anfordern.';
}

export function PrivateVodSection() {
  const [data, setData] = useState<PrivateVodClipsResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [success, setSuccess] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const mutationVersion = useRef(0);

  async function refresh() {
    const version = mutationVersion.current;
    setRefreshing(true);
    try {
      const next = await fetchPrivateVodClips();
      if (version === mutationVersion.current) { setData(next); setError(null); }
    } catch {
      if (version === mutationVersion.current) setError('VOD-Ausschnitte konnten nicht geladen werden. Bitte erneut laden.');
    } finally {
      setNow(Date.now());
      setRefreshing(false);
    }
  }

  useEffect(() => {
    let active = true;
    const load = async () => {
      const version = mutationVersion.current;
      try {
        const next = await fetchPrivateVodClips();
        if (active && version === mutationVersion.current) { setData(next); setError(null); }
      } catch {
        if (active && version === mutationVersion.current) setError('VOD-Ausschnitte konnten nicht geladen werden. Bitte erneut laden.');
      } finally {
        if (active) setNow(Date.now());
      }
    };
    void load();
    const interval = window.setInterval(() => { if (!document.hidden) void load(); }, 30_000);
    return () => { active = false; window.clearInterval(interval); };
  }, []);

  async function toggle() {
    if (!data) return;
    mutationVersion.current += 1;
    setPending(true);
    setError(null);
    setSuccess(null);
    try {
      const result = await setPrivateVodCommand(!data.enabled);
      setData(previous => previous ? { ...previous, enabled: result.enabled } : previous);
      setSuccess(result.enabled ? 'VOD-Befehl eingeschaltet.' : 'VOD-Befehl ausgeschaltet. Vorhandene Downloads bleiben bis zum Ablauf verfügbar.');
    } catch {
      setError('Die VOD-Einstellung konnte nicht gespeichert werden. Bitte erneut versuchen.');
    } finally {
      mutationVersion.current += 1;
      setPending(false);
    }
  }

  return (
    <div className="mt-6 border-t border-border pt-6">
      <h3 className="text-lg font-semibold text-white">Private VOD-Ausschnitte</h3>
      <p className="mt-2 text-sm text-text-secondary">
        Schneide einen längeren Bereich aus der Twitch-Aufzeichnung. Im laufenden Stream wird dessen Aufzeichnung verwendet, offline die neueste. Twitch muss den gewählten Abschnitt bereits bereitstellen.
      </p>
      <p className="mt-2 text-sm text-text-secondary">
        Der Kanalinhaber und die Mods können 1 bis 10 Minuten anfordern. Ein Auftrag gleichzeitig pro Kanal, höchstens drei pro Stunde. Der Download ist für den angemeldeten Kanalinhaber verfügbar, wird nicht veröffentlicht und nach 24 Stunden ab der Anfrage gelöscht.
      </p>
      <p className="mt-2 text-sm text-text-secondary">
        Bis zu 64 MiB als MP4, Vorbereitung höchstens 15 Minuten. Insgesamt werden höchstens 20 Aufträge aufbewahrt und einer nach dem anderen verarbeitet.
      </p>
      <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <EditableCommandName command="vodclip" className="mb-2" />
          <p className="text-sm text-text-secondary">{data ? (data.enabled ? 'Befehl ist aktiv' : 'Befehl ist aus') : 'Status wird geladen …'}</p>
        </div>
        <button
          type="button"
          onClick={() => void toggle()}
          disabled={!data || pending}
          aria-label={`VOD-Befehl ${data?.enabled ? 'deaktivieren' : 'aktivieren'}`}
          className="inline-flex min-h-11 items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-4 py-2.5 text-sm font-semibold text-primary hover:bg-primary/20 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary disabled:cursor-not-allowed disabled:opacity-50"
        >
          {pending ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> : data?.enabled ? <PowerOff className="h-4 w-4" aria-hidden="true" /> : <Power className="h-4 w-4" aria-hidden="true" />}
          {data?.enabled ? 'Deaktivieren' : 'Aktivieren'}
        </button>
      </div>
      <p className="mt-4 text-sm text-text-secondary">
        Beispiel: <code>!vodclip 55 58</code> für Minute 55 bis 58. Auch <code>55:00 58:00</code>, <code>1:55:00 1:58:00</code> und Punkte statt Doppelpunkten funktionieren. Bei einem eigenen Befehlsnamen verwendest du diesen statt <code>!vodclip</code>.
      </p>
      {error && <p role="alert" className="mt-3 text-sm text-danger">{error}</p>}
      {success && <p role="status" className="mt-3 text-sm text-success">{success}</p>}
      <div className="mt-5 flex items-center justify-between gap-3">
        <h4 className="text-sm font-semibold text-white">Deine Downloads</h4>
        <button type="button" onClick={() => void refresh()} disabled={refreshing} className="inline-flex min-h-11 items-center gap-2 rounded-lg px-3 py-2 text-sm text-primary hover:bg-primary/10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary disabled:opacity-50">
          <RefreshCw className={`h-4 w-4 ${refreshing ? 'animate-spin' : ''}`} aria-hidden="true" />
          Neu laden
        </button>
      </div>
      {data && data.jobs.length === 0 && <p className="mt-2 text-sm text-text-secondary">Noch keine VOD-Ausschnitte. Fordere im Chat einen Bereich an, nachdem du den Befehl eingeschaltet hast.</p>}
      {data && data.jobs.length > 0 && (
        <ul className="mt-2 divide-y divide-border">
          {data.jobs.map(job => (
            <li key={job.id} className="flex flex-wrap items-center justify-between gap-3 py-4">
              <div className="min-w-0">
                <p className="text-sm font-semibold text-white">{timestamp(job.start_seconds)} bis {timestamp(job.end_seconds)}</p>
                <p className="mt-1 text-sm text-text-secondary">{jobStatus(job, now)}</p>
                {job.status === 'ready' && <p className="mt-1 text-xs text-text-secondary">Verfügbar bis {new Date(job.expires_at).toLocaleString('de-DE')}{job.file_bytes ? ` · ${(job.file_bytes / 1024 / 1024).toFixed(1)} MiB` : ''}</p>}
              </div>
              {job.status === 'ready' && Date.parse(job.expires_at) > now && (
                <a href={privateVodDownloadUrl(job.id)} download className="inline-flex min-h-11 items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-4 py-2.5 text-sm font-semibold text-primary hover:bg-primary/20 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary" aria-label={`MP4 herunterladen: ${timestamp(job.start_seconds)} bis ${timestamp(job.end_seconds)}`}>
                  <Download className="h-4 w-4" aria-hidden="true" />MP4 herunterladen
                </a>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
