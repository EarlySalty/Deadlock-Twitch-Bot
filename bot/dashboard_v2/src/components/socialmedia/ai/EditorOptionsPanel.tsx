import { useT } from '@/context/LanguageContext';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { AiEditorOptions, AiFrameFormat, AiPublishLanguage } from './api';
import { fetchAiConsent, fetchAiOptions, saveAiOptions } from './api';
import { providerAccessText } from './failure';

const languages: { value: AiPublishLanguage; label: string }[] = [
  { value: 'de', label: 'Deutsch' },
  { value: 'en', label: 'Englisch' },
  { value: 'auto', label: 'Automatisch nach Clip-Sprache' },
];

const frames: { value: AiFrameFormat; label: string }[] = [
  { value: 'layout', label: 'Dein Layout' },
  { value: 'fill', label: 'Hochformat füllen' },
  { value: 'full', label: 'Ganzes Bild' },
];

const frameHelp: Record<AiFrameFormat, string> = {
  layout: 'Nutzt dein gespeichertes Layout aus dem Layout-Editor. Ohne gespeichertes Layout siehst du das ganze Bild mit unscharfem Hintergrund.',
  fill: 'Das Spielbild füllt das ganze 9:16-Video aus. Die Ränder links und rechts werden abgeschnitten, die Facecam ist nicht zu sehen.',
  full: 'Das ganze 16:9-Bild steht mittig im Video, oben und unten mit unscharfem Hintergrund.',
};

const choices: { key: 'multi_output_enabled' | 'experimental_ai_layout'; label: string; help: string }[] = [
  { key: 'multi_output_enabled', label: 'Mehrere Videos aus einem Clip erlauben', help: 'Die KI erstellt zusätzlich zur Hauptfassung nur dann weitere Varianten, wenn der Clip genug eigenständiges Material hergibt. Ohne diese Einstellung entsteht genau ein Video.' },
  { key: 'experimental_ai_layout', label: 'Experimentelles KI-Layout', help: 'Vorbereitet für eigene Bildaufteilungen durch die KI. Vorerst gilt das gewählte Bildformat und es werden keine Item-Grafiken eingeblendet.' },
];

export function EditorOptionsPanel({ channel, disabled = false }: { channel: string; disabled?: boolean }) {
  const t = useT();
  const client = useQueryClient();
  const key = ['social-ai', 'options', channel] as const;
  const options = useQuery({ queryKey: key, queryFn: ({ signal }) => fetchAiOptions(channel, signal), retry: false, refetchOnWindowFocus: false });
  const save = useMutation({
    mutationFn: (value: AiEditorOptions) => saveAiOptions(channel, value),
    onSuccess: (value) => client.setQueryData(key, value),
  });
  return <section data-saving={save.isPending ? "true" : undefined} className="social-ai-options" aria-label={t("Optionen für die KI-Bearbeitung")}>
    <div className="social-ai-section-head"><h3>{t("Optionen für die KI-Bearbeitung")}</h3><p>{t("Gilt für neue Bearbeitungen in deinem Kanal. Mehrere Videos und das KI-Layout sind standardmäßig aus.")}</p></div>
    {options.isPending ? <p role="status">{t("Optionen werden geladen…")}</p> : options.isError ? <p className="social-ai-caution" role="alert">{t("Die Optionen konnten nicht geladen werden.")}</p> : <div className="social-ai-option-list">
      {choices.map((choice) => <label className="studio-switch" key={choice.key}><input type="checkbox" checked={options.data[choice.key]} disabled={disabled || save.isPending || options.isFetching} onChange={(event) => save.mutate({ ...options.data, [choice.key]: event.target.checked })} /><span>{t(choice.label)}</span><small>{t(choice.help)}</small></label>)}
      <label className="social-ai-language"><span>{t("Sprache für Titel, Beschreibung und Hashtags")}</span><select className="studio-select" value={options.data.publish_language} disabled={disabled || save.isPending || options.isFetching} onChange={(event) => save.mutate({ ...options.data, publish_language: event.target.value as AiPublishLanguage })}>{languages.map((language) => <option key={language.value} value={language.value}>{t(language.label)}</option>)}</select><small>{t("Die Untertitel bleiben in der gesprochenen Sprache des Clips.")}</small></label>
      <label className="social-ai-language"><span>{t("Bildformat im Hochkant-Video")}</span><select className="studio-select" value={options.data.frame_format} disabled={disabled || save.isPending || options.isFetching} onChange={(event) => save.mutate({ ...options.data, frame_format: event.target.value as AiFrameFormat })}>{frames.map((frame) => <option key={frame.value} value={frame.value}>{t(frame.label)}</option>)}</select><small>{t(frameHelp[options.data.frame_format])} {t("Du kannst das Format auch per Änderungswunsch an die KI umstellen.")}</small></label>
    </div>}
    <div role="status" aria-live="polite">{save.isSuccess && <p>{t("Optionen gespeichert.")}</p>}{save.isError && <p className="social-ai-caution">{t("Speichern fehlgeschlagen. Bitte versuche es erneut.")}</p>}</div>
  </section>;
}

export function AiPlanOptions({ channel, onOpenSettings }: { channel: string; onOpenSettings: () => void }) {
  const t = useT();
  const consent = useQuery({ queryKey: ['social-ai', 'consent', channel], queryFn: ({ signal }) => fetchAiConsent(channel, signal), retry: false, refetchOnWindowFocus: false });
  const access = consent.isSuccess ? consent.data.provider_access : null;
  const blocked = access != null && access !== 'ready';
  const ready = consent.isSuccess && consent.data.external_ai_consent && consent.data.global_allowed && !blocked;
  const status = consent.isPending ? t('Freigabe wird geladen…')
    : consent.isError ? t('Die Freigabe konnte nicht geladen werden.')
      : !consent.data.global_allowed ? t('Ein Admin muss die externe KI erst freigeben.')
        : !consent.data.external_ai_consent ? t('KI-Einwilligung fehlt. Ohne Einwilligung startet keine KI-Bearbeitung.')
          : blocked ? `${t('KI-Einwilligung erteilt.')} ${t(providerAccessText(access))}` : t('KI-Einwilligung erteilt.');
  return <div className="social-ai-plan-options">
    <p className={`social-ai-consent-status${ready ? '' : ' social-ai-caution'}`} role="status">{status} <button type="button" className="studio-link-button" onClick={onOpenSettings}>{t('Einwilligung in Einstellungen ändern')}</button></p>
    <EditorOptionsPanel key={channel} channel={channel} disabled={!consent.isSuccess || !consent.data.global_allowed} />
  </div>;
}
