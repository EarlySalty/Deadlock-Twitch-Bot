import { useEffect, useRef } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useT } from '@/context/LanguageContext';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { fetchRecordingConsent, postRecordingChunk, saveRecordingConsent } from '@/api/recording';
import { isPreviewLocalhost } from '@/preview/routes';
import { onPublicTour } from '@/publicTour';
import {
  RECORDING_TEXT,
  shouldAskForConsent,
  shouldRecord,
  type RecordingConsent,
  type RecordingDecision,
} from './recordingPolicy';
import type { RecordingHandle } from './recorder';

const CONSENT_KEY = ['social-media-recording-consent'];

function recordingAvailable(): boolean {
  return !isPreviewLocalhost() && !onPublicTour();
}

function useRecorder(recording: boolean, csrfToken: string | null) {
  const csrfRef = useRef(csrfToken);
  const handleRef = useRef<RecordingHandle | null>(null);

  useEffect(() => {
    csrfRef.current = csrfToken;
  }, [csrfToken]);

  useEffect(() => {
    if (!recording) return;
    let cancelled = false;
    void import('./recorder').then(({ startRecording }) => {
      if (cancelled) return;
      handleRef.current = startRecording((body, keepalive) => postRecordingChunk(body, keepalive, csrfRef.current));
    });
    return () => {
      cancelled = true;
      handleRef.current?.stop(true);
      handleRef.current = null;
    };
  }, [recording]);

  return {
    csrfRef,
    discard() {
      handleRef.current?.stop(false);
      handleRef.current = null;
    },
  };
}

function ConsentCard({ onDecide, pending, failed }: { onDecide: (decision: RecordingDecision) => void; pending: boolean; failed: boolean }) {
  const t = useT();
  return (
    <div data-recording-block className="fixed bottom-4 right-4 z-40 w-[calc(100vw-2rem)] max-w-sm">
      <aside
        aria-labelledby="social-recording-consent-title"
        className="panel-card rounded-2xl p-4 shadow-lg"
      >
        <h3 id="social-recording-consent-title" className="text-sm font-semibold text-white">{t(RECORDING_TEXT.title)}</h3>
        <p className="mt-2 text-xs leading-relaxed text-text-secondary">{t(RECORDING_TEXT.body)}</p>
        <div className="mt-3 flex flex-wrap gap-2">
          <button
            type="button"
            disabled={pending}
            onClick={() => onDecide('granted')}
            className="rounded-lg bg-primary px-3 py-2 text-xs font-semibold text-[#0D0806] disabled:opacity-60"
          >
            {t(RECORDING_TEXT.allow)}
          </button>
          <button type="button" disabled={pending} onClick={() => onDecide('declined')} className="studio-button text-xs">
            {t(RECORDING_TEXT.decline)}
          </button>
        </div>
        {failed && <p role="alert" className="mt-2 text-xs text-warning">{t(RECORDING_TEXT.saveFailed)}</p>}
      </aside>
    </div>
  );
}

function ConsentToggle({ granted, onDecide, pending, failed }: { granted: boolean; onDecide: (decision: RecordingDecision) => void; pending: boolean; failed: boolean }) {
  const t = useT();
  return (
    <div data-recording-block className="flex flex-wrap items-center gap-2 text-xs text-text-secondary">
      <span>{t(granted ? RECORDING_TEXT.statusOn : RECORDING_TEXT.statusOff)}</span>
      <button
        type="button"
        disabled={pending}
        onClick={() => onDecide(granted ? 'declined' : 'granted')}
        className="underline underline-offset-2 hover:text-white disabled:opacity-60"
      >
        {t(granted ? RECORDING_TEXT.turnOff : RECORDING_TEXT.turnOn)}
      </button>
      {failed && <span role="alert" className="text-warning">{t(RECORDING_TEXT.saveFailed)}</span>}
    </div>
  );
}

export function SocialMediaRecording({ active }: { active: boolean }) {
  const enabled = active && recordingAvailable();
  const { data: auth } = useAuthStatus();
  const queryClient = useQueryClient();
  const consent = useQuery({
    queryKey: CONSENT_KEY,
    queryFn: fetchRecordingConsent,
    enabled,
    staleTime: 5 * 60 * 1000,
    retry: false,
  });
  const recording = enabled && shouldRecord(consent.data, true, window.location.pathname);
  const recorder = useRecorder(recording, auth?.csrfToken ?? auth?.csrf_token ?? null);
  const decide = useMutation({
    mutationFn: (decision: RecordingDecision) => saveRecordingConsent(decision, recorder.csrfRef.current),
    onMutate: (decision) => {
      if (decision === 'declined') recorder.discard();
    },
    onSuccess: (_result, decision) => {
      queryClient.setQueryData<RecordingConsent>(CONSENT_KEY, (old) => (old ? { ...old, decision } : old));
    },
  });

  if (!enabled || !consent.data?.eligible) return null;
  const onDecide = (decision: RecordingDecision) => decide.mutate(decision);
  if (shouldAskForConsent(consent.data, true)) {
    return <ConsentCard onDecide={onDecide} pending={decide.isPending} failed={decide.isError} />;
  }
  return <ConsentToggle granted={consent.data.decision === 'granted'} onDecide={onDecide} pending={decide.isPending} failed={decide.isError} />;
}
