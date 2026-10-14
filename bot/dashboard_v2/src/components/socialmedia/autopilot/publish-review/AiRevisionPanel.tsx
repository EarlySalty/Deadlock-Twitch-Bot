import { useCallback, useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { AiJobDetails } from '../../ai/AiEditorPanel';
import type { AiJobView } from '../../ai/api';
import { fetchAiConsent, fetchAiJob, openAiVideoEditor, retryAiJob } from '../../ai/api';
import { canRetryJob } from '../../ai/failure';

export function AiRevisionPanel({ owner, videoId, onRefresh, onClose, onBusyChange }: {
  owner: string;
  videoId: number;
  onRefresh: () => Promise<boolean>;
  onClose: () => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const client = useQueryClient();
  const [selectedJob, setSelectedJob] = useState<number | null>(null);
  const [locked, setLocked] = useState(false);
  const reportBusy = useCallback((busy: boolean) => { setLocked(busy); onBusyChange(busy); }, [onBusyChange]);
  const refreshed = useRef('');
  const opening = useQuery({ queryKey: ['social-ai', 'video-editor', owner, videoId], queryFn: () => openAiVideoEditor(owner, videoId), retry: false, refetchOnWindowFocus: false, staleTime: 0 });
  const jobId = selectedJob ?? opening.data?.id;
  const consent = useQuery({ queryKey: ['social-ai', 'consent', owner], queryFn: ({ signal }) => fetchAiConsent(owner, signal), retry: false });
  const job = useQuery({
    queryKey: ['social-ai', 'job', owner, jobId],
    queryFn: ({ signal }) => fetchAiJob(owner, jobId!, signal),
    enabled: jobId !== undefined,
    retry: false,
    refetchOnWindowFocus: false,
    refetchInterval: (query) => query.state.data && !['review', 'failed', 'rejected', 'awaiting_confirmation'].includes(query.state.data.status) ? 5000 : false,
  });
  const update = (value: AiJobView) => {
    client.setQueryData(['social-ai', 'job', owner, value.id], value);
    setSelectedJob(value.id);
  };
  const retry = useMutation({ mutationFn: () => retryAiJob(owner, jobId!), onSuccess: update });
  useEffect(() => {
    if (!job.data) return;
    const key = `${job.data.id}-${job.data.status}`;
    if (key === refreshed.current) return;
    refreshed.current = key;
    void onRefresh();
  }, [job.data, onRefresh]);
  const canStart = consent.data?.external_ai_consent === true && consent.data.global_allowed === true && !consent.isFetching;
  const reload = async () => {
    const loaded = await job.refetch();
    if (!loaded.isSuccess) return false;
    retry.reset();
    return onRefresh();
  };
  return <section className="social-ai publish-ai-revision" aria-label="KI-Fassung überarbeiten">
    <header className="publish-review-header"><div><h3>KI-Fassung überarbeiten</h3><p>Die bisherige Datei bleibt zum Vergleich erhalten. Nach der Überarbeitung musst du die neue Fassung erneut freigeben.</p></div><button type="button" disabled={locked || retry.isPending} onClick={onClose}>Überarbeitung schließen</button></header>
    {!canStart && <p className="publish-note">Für neue KI-Änderungen brauchst du die Freigabe für externe KI im Bereich „KI-Videoschnitt“.</p>}
    {opening.isError || job.isError ? <div role="alert"><p>Die KI-Fassung konnte nicht geladen werden.</p><button type="button" onClick={() => { if (jobId) void job.refetch(); else void opening.refetch(); }}>Erneut laden</button></div> : job.data ? <AiJobDetails key={job.data.id} job={job.data} channel={owner} canStart={canStart} publishing={false} onBusyChange={reportBusy} onJob={update} onReload={reload} onRetry={() => { if (job.data && canRetryJob(job.data, { canStart, retryPending: retry.isPending, locked })) retry.mutate(); }} retryPending={retry.isPending} retryError={retry.isError} /> : <p role="status">Gespeicherte KI-Fassung wird geladen…</p>}
  </section>;
}
