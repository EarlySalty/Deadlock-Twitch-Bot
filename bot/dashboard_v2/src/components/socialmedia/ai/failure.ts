import type { AiJobView } from './api';
import { dictionaryFor, type TranslateParams } from '@/i18n/dictionary';
import { FEHLER_TEXTE, fehlerText } from '../labels';

const explanations: Record<string, string> = {
  source_unavailable: 'Der Originalclip konnte nicht bereitgestellt werden.',
  transcription_failed: 'Die Spracherkennung konnte nicht abgeschlossen werden.',
  analysis_failed: 'Die Analyse des Clips konnte nicht abgeschlossen werden.',
  planning_failed: 'Die Schnittplanung konnte nicht abgeschlossen werden.',
  rendering_failed: 'Das bearbeitete Video konnte nicht erstellt werden.',
  render_failed: 'Das bearbeitete Video konnte nicht erstellt werden.',
  validation_failed: 'Die Prüfung des erstellten Videos ist fehlgeschlagen.',
  provider_unavailable: 'Der externe KI-Anbieter war nicht erreichbar.',
  budget_exceeded: 'Das verfügbare Kostenlimit reicht für diesen Auftrag nicht aus.',
  consent_missing: 'Die erforderliche Freigabe für externe KI fehlt.',
  interrupted: 'Die Bearbeitung wurde unterbrochen.',
};

export function canRetryJob(job: Pick<AiJobView, 'status' | 'can_retry'>, state: { canStart: boolean; retryPending: boolean; locked: boolean }): boolean {
  return job.status === 'failed' && job.can_retry && state.canStart && !state.retryPending && !state.locked;
}

export function failureExplanation(code: string | null): string {
  return code && Object.hasOwn(explanations, code)
    ? explanations[code]
    : 'Bitte prüfe den Auftrag und versuche die fehlgeschlagene Phase erneut, falls ein weiterer Versuch angeboten wird.';
}

export function editorErrorExplanation(
  error: unknown,
  t: (text: string, params?: TranslateParams) => string,
): string {
  if (error instanceof Error) {
    const code = (error as Error & { code?: string }).code;
    if (code && Object.hasOwn(FEHLER_TEXTE, code)) return fehlerText(error, t) ?? t('Die Änderung konnte nicht bestätigt werden.');
    if (code && Object.hasOwn(explanations, code)) return t(explanations[code]);
    if (Object.hasOwn(dictionaryFor('en'), error.message)) return t(error.message);
  }
  return t('Die Änderung konnte nicht bestätigt werden.');
}
