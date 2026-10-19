import { approvalVideos } from './approvalVideos';
import type { StudioOverviewData } from './StudioOverview';
import type { SocialMediaViewTarget } from './labels';
import type { QueueStage } from './queuePresentation';
import type { ProductionHold, ScheduleQueueSnapshot } from './autopilot/schedule-queue/api';

type Translate = (text: string, params?: Record<string, string | number>) => string;

export interface StudioNextStep {
  text: string;
  view: SocialMediaViewTarget;
  stage?: QueueStage;
  tone: 'neutral' | 'warning' | 'danger' | 'success';
}

type HoldStep = (schedule: ScheduleQueueSnapshot, t: Translate, formatTime: (iso: string) => string) => StudioNextStep;

const HOLD_STEPS: Record<Exclude<ProductionHold, null>, HoldStep> = {
  disabled: (_schedule, t) => ({
    text: t('Der Auto-Pilot ist ausgeschaltet. Auf Planen kannst du ihn einschalten oder die Pipeline testen.'),
    view: 'planen',
    tone: 'neutral',
  }),
  paused: (_schedule, t) => ({
    text: t('Der Auto-Pilot ist pausiert. Laufende Produktionen werden fertig, fortsetzen kannst du ihn auf Planen.'),
    view: 'planen',
    tone: 'warning',
  }),
  review_backlog: (schedule, t) => {
    const inPlan = schedule.jobs.some((job) => job.status === 'review');
    return {
      text: t(inPlan
        ? 'Auto-Pilot wartet, bis du die offenen Produktionen auf Planen prüfst.'
        : 'Auto-Pilot wartet, bis du die fertigen Videos unter Clips freigibst.'),
      view: inPlan ? 'planen' : 'freigabe',
      tone: 'warning',
    };
  },
  failure_backlog: (_schedule, t) => ({
    text: t('Auto-Pilot wartet, weil die letzten Produktionen fehlgeschlagen sind. Starte auf Planen einen Testlauf.'),
    view: 'planen',
    tone: 'danger',
  }),
  empty_pool: (_schedule, t) => ({
    text: t('Auto-Pilot wartet: Keine passenden Clips verfügbar. Prüfe auf Planen deine Clips oder den Zeitraum.'),
    view: 'planen',
    tone: 'warning',
  }),
  no_schedule: (_schedule, t) => ({
    text: t('Auto-Pilot wartet: Wöchentliche Termine fehlen. Füge auf Planen mindestens eine Zeit hinzu.'),
    view: 'planen',
    tone: 'warning',
  }),
  outside_lead: (schedule, t, formatTime) => ({
    text: schedule.next_slot
      ? t('Auto-Pilot aktiv. Der nächste Termin {time} liegt noch außerhalb des Vorlaufs. Kein Eingriff nötig.', {
        time: formatTime(schedule.next_slot),
      })
      : t('Auto-Pilot aktiv. Der nächste freie Termin liegt noch außerhalb des Vorlaufs. Kein Eingriff nötig.'),
    view: 'planen',
    tone: 'neutral',
  }),
};

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
  if (!data.publishFailed && approvalVideos(data.publish).length > 0) {
    return {
      text: t('Fertiges Video wartet auf Heute unter Prüfung & Freigabe.'),
      view: 'heute',
      tone: 'warning',
    };
  }
  const held = schedule.production_hold ? HOLD_STEPS[schedule.production_hold](schedule, t, formatTime) : undefined;
  if (held?.tone === 'danger') return held;
  if (data.publishFailed) {
    return {
      text: t('Ob fertige Videos auf Freigabe warten, ist gerade nicht abrufbar. Schau gleich noch einmal auf Heute.'),
      view: 'heute',
      tone: 'warning',
    };
  }
  if (held) return held;
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
