import { approvalVideos } from './approvalVideos';
import type { StudioOverviewData } from './StudioOverview';
import type { SocialMediaViewTarget } from './labels';
import type { QueueStage } from './queuePresentation';

type Translate = (text: string, params?: Record<string, string | number>) => string;

export interface StudioNextStep {
  text: string;
  view: SocialMediaViewTarget;
  stage?: QueueStage;
  tone: 'neutral' | 'warning' | 'danger' | 'success';
}

const SOURCE_STAGES = new Set(['queued', 'starting', 'preparing', 'source', 'downloading']);

export function sentenceFragment(text: string): string {
  return text.replace(/[.!?…]+$/u, '').trimEnd();
}

export function studioNextStep(
  data: StudioOverviewData,
  t: Translate,
  formatTime: (iso: string) => string,
): StudioNextStep {
  if (data.scheduleFailed) {
    return { text: t('Der aktuelle Schritt ist nicht abrufbar.'), view: 'heute', tone: 'warning' };
  }
  const schedule = data.schedule;
  if (!schedule) {
    return { text: t('Stand wird geladen.'), view: 'heute', tone: 'neutral' };
  }
  const run = schedule.test_run;
  if (run) {
    if (run.status === 'failed' || (run.status === 'review' && run.video_status !== 'review')) {
      return {
        text: t('Testlauf fehlgeschlagen: {error}. Auf Heute erneut versuchen oder verwerfen.', {
          error: sentenceFragment(run.last_error || t('Unbekannter Fehler')),
        }),
        view: 'heute',
        tone: 'danger',
      };
    }
    if (run.video_status === 'review') {
      return {
        text: t('Fertiges Video wartet auf Heute unter Prüfung & Freigabe.'),
        view: 'heute',
        tone: 'warning',
      };
    }
    if (run.status === 'queued' || run.status === 'running') {
      const downloading = SOURCE_STAGES.has(run.stage);
      return {
        text: t(downloading ? 'Testlauf: Datei wird von Twitch geladen.' : 'Testlauf: Clip wird vorbereitet.'),
        view: 'heute',
        tone: 'neutral',
      };
    }
  }
  const publish = data.publishFailed ? undefined : data.publish;
  if (approvalVideos(publish).length > 0) {
    return {
      text: t('Fertiges Video wartet auf Heute unter Prüfung & Freigabe.'),
      view: 'heute',
      tone: 'warning',
    };
  }
  if (schedule.production_hold === 'failure_backlog') {
    return {
      text: t('Auto-Pilot wartet, weil die letzten Produktionen fehlgeschlagen sind. Starte auf Planen einen Testlauf.'),
      view: 'planen',
      tone: 'danger',
    };
  }
  const failedPrep = schedule.jobs.some((job) => job.status === 'failed' && job.source !== 'test');
  if (schedule.settings.enabled && failedPrep) {
    return {
      text: t('Auto-Pilot aktiv, aber die letzte Vorbereitung ist fehlgeschlagen. Öffne Heute.'),
      view: 'heute',
      tone: 'danger',
    };
  }
  if (schedule.settings.enabled && !schedule.settings.paused) {
    if (schedule.next_slot) {
      return {
        text: t('Auto-Pilot aktiv. Nächster Termin {time}. Kein Eingriff nötig.', {
          time: formatTime(schedule.next_slot),
        }),
        view: 'planen',
        tone: 'success',
      };
    }
    return { text: t('Auto-Pilot aktiv. Kein Eingriff nötig.'), view: 'planen', tone: 'success' };
  }
  return { text: t('Nichts läuft gerade. Auf Planen kannst du die Pipeline testen.'), view: 'planen', tone: 'neutral' };
}
