import { useMutation } from '@tanstack/react-query';
import { FlaskConical, Trash2, Zap } from 'lucide-react';
import { useLanguage, type LanguageContextValue } from '@/context/LanguageContext';
import type { ScheduleQueueApi, TestRun } from './autopilot/schedule-queue/api';

export type TestRunApi = Pick<ScheduleQueueApi, 'startTestRun' | 'discardTestRun'>;
type Translate = LanguageContextValue['t'];

const DONE_STEP = 4;

function currentStep(run: TestRun): number {
  if (run.video_status === 'review') return DONE_STEP;
  if (run.status === 'queued' || ['starting', 'preparing', 'queued'].includes(run.stage)) return 1;
  if (['rendered', 'validating'].includes(run.stage) || run.status === 'completed') return 3;
  return 2;
}

function stepLabels(run: TestRun, t: Translate): string[] {
  return [
    t('Clip aus Pool geladen'),
    t(run.mode === 'ai' ? 'KI-Analyse / Highlight-Schnitt läuft' : 'Standard-Layout wird angewendet'),
    t('Untertitel & Texte werden generiert'),
    t('Fertig → Bereit zur Vorschau & Freigabe unter „Prüfung & Freigabe“'),
  ];
}

function errorText(error: unknown, t: Translate): string {
  return error instanceof Error && error.message ? error.message : t('Die Aktion ist fehlgeschlagen.');
}

export function TestRunButton({ testRun, available, api, owner, onChanged }: {
  testRun: TestRun | null | undefined;
  available: boolean;
  api: TestRunApi;
  owner?: string;
  onChanged?: () => void;
}) {
  const { t } = useLanguage();
  const start = useMutation({ mutationFn: () => api.startTestRun(owner), onSuccess: () => onChanged?.() });
  const active = Boolean(testRun);
  return <div className="studio-testrun-start">
    <button type="button" className="studio-button studio-testrun-button" disabled={!available || active || start.isPending}
      title={active ? t('Es läuft bereits ein Testlauf. Prüfe oder verwirf ihn zuerst.') : undefined} onClick={() => start.mutate()}>
      <Zap className="h-4 w-4" aria-hidden />{t(start.isPending ? 'Testlauf wird gestartet …' : 'Pipeline jetzt testen (1 Clip vorbereiten)')}
    </button>
    {start.error && <p className="studio-testrun-error" role="alert">{errorText(start.error, t)}</p>}
  </div>;
}

export function TestRunStatus({ testRun, api, owner, onChanged }: {
  testRun: TestRun;
  api: TestRunApi;
  owner?: string;
  onChanged?: () => void;
}) {
  const { t } = useLanguage();
  const discard = useMutation({ mutationFn: () => api.discardTestRun(testRun.job_id, owner), onSuccess: () => onChanged?.() });
  const step = currentStep(testRun);
  const failed = testRun.status === 'failed';
  const waiting = testRun.status === 'review' && testRun.video_status !== 'review';
  const title = testRun.clip_title || t('Clip {id}', { id: testRun.clip_id });
  return <div className="studio-testrun" data-state={failed ? 'failed' : waiting ? 'waiting' : step === DONE_STEP ? 'done' : 'running'} role="status">
    <div className="studio-testrun-head">
      <span className="studio-mode-badge" data-mode="test"><FlaskConical className="h-3 w-3" aria-hidden />{t('Testlauf')}</span>
      <strong title={title}>{title}</strong>
    </div>
    {failed ? <p className="studio-testrun-error">{t('Der Testlauf ist fehlgeschlagen: {error}', { error: testRun.last_error || t('Unbekannter Fehler') })}</p>
      : waiting ? <p className="studio-testrun-note">{t('Der Testlauf wartet auf deine Entscheidung.')}</p>
        : <ol className="studio-testrun-steps">{stepLabels(testRun, t).map((label, index) => {
          const number = index + 1;
          const state = number < step || step === DONE_STEP ? 'done' : number === step ? 'current' : 'pending';
          return <li key={number} data-state={state} aria-current={state === 'current' ? 'step' : undefined}>{label}</li>;
        })}</ol>}
    {testRun.status !== 'running' && <button type="button" className="studio-button studio-testrun-discard" disabled={discard.isPending} onClick={() => discard.mutate()}>
      <Trash2 className="h-4 w-4" aria-hidden />{t('Test verwerfen (Clip zurück in den Vorrat)')}
    </button>}
    {discard.error && <p className="studio-testrun-error" role="alert">{errorText(discard.error, t)}</p>}
  </div>;
}
