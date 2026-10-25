import type { AiJobView, AiProviderAccess } from './api';
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
  provider_access: 'Der KI-Zugang ist auf dem Server nicht vollständig eingerichtet. Ein Admin muss ihn prüfen, danach kannst du die Phase erneut versuchen.',
  budget_exceeded: 'Das verfügbare Kostenlimit reicht für diesen Auftrag nicht aus.',
  consent_missing: 'Die erforderliche Freigabe für externe KI fehlt.',
  interrupted: 'Die Bearbeitung wurde durch eine Störung auf unserer Seite unterbrochen.',
  revision_unchanged: 'Die gewünschte Änderung ließ sich nicht umsetzen. Es wurde keine neue Fassung erstellt.',
};

const providerAccessTexts: Record<AiProviderAccess, string> = {
  ready: 'Der KI-Zugang ist eingerichtet und wurde erfolgreich geprüft.',
  key_missing: 'Auf dem Server ist kein Schlüssel für den KI-Anbieter hinterlegt. Ein Admin muss ihn eintragen.',
  workspace_required: 'Der Schlüssel für den KI-Anbieter ist hinterlegt, gehört aber zu keinem Workspace. Ein Admin muss die Workspace-ID ergänzen oder einen Workspace-Schlüssel eintragen.',
  workspace_invalid: 'Die hinterlegte Workspace-ID wird vom KI-Anbieter nicht angenommen. Ein Admin muss sie prüfen.',
  key_rejected: 'Der KI-Anbieter lehnt den hinterlegten Schlüssel ab. Ein Admin muss ihn prüfen.',
  unreachable: 'Der KI-Anbieter konnte gerade nicht erreicht werden. Der Zugang wird in wenigen Minuten erneut geprüft.',
};

export function providerAccessText(access: AiProviderAccess): string {
  return providerAccessTexts[access];
}

export function canRetryJob(job: Pick<AiJobView, 'status' | 'can_retry'>, state: { canStart: boolean; retryPending: boolean; locked: boolean }): boolean {
  return job.status === 'failed' && job.can_retry && state.canStart && !state.retryPending && !state.locked;
}

export function isUnchangedRevision(job: Pick<AiJobView, 'status' | 'failure_code'>): boolean {
  return job.status === 'failed' && job.failure_code === 'revision_unchanged';
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
