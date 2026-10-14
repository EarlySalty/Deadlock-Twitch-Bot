import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { AiEditorOptions, AiPublishLanguage } from './api';
import { fetchAiOptions, saveAiOptions } from './api';

const languages: { value: AiPublishLanguage; label: string }[] = [
  { value: 'de', label: 'Deutsch' },
  { value: 'en', label: 'Englisch' },
  { value: 'auto', label: 'Automatisch nach Clip-Sprache' },
];

const choices: { key: 'multi_output_enabled' | 'experimental_ai_layout'; label: string; help: string }[] = [
  { key: 'multi_output_enabled', label: 'Mehrere Videos aus einem Clip erlauben', help: 'Die KI erstellt zusätzlich zur Hauptfassung nur dann weitere Varianten, wenn der Clip genug eigenständiges Material hergibt. Ohne diese Einstellung entsteht genau ein Video.' },
  { key: 'experimental_ai_layout', label: 'Experimentelles KI-Layout', help: 'Vorbereitet für eigene Bildaufteilungen durch die KI. Vorerst bleibt dein gespeichertes Layout aktiv und es werden keine Item-Grafiken eingeblendet.' },
];

export function EditorOptionsPanel({ channel }: { channel: string }) {
  const client = useQueryClient();
  const key = ['social-ai', 'options', channel] as const;
  const options = useQuery({ queryKey: key, queryFn: ({ signal }) => fetchAiOptions(channel, signal), retry: false, refetchOnWindowFocus: false });
  const save = useMutation({
    mutationFn: (value: AiEditorOptions) => saveAiOptions(channel, value),
    onSuccess: (value) => client.setQueryData(key, value),
  });
  return <section className="social-ai-options" aria-label="Optionen für die KI-Bearbeitung">
    <div className="social-ai-section-head"><h3>Optionen für die KI-Bearbeitung</h3><p>Gilt für neue Bearbeitungen in deinem Kanal. Mehrere Videos und das KI-Layout sind standardmäßig aus.</p></div>
    {options.isPending ? <p role="status">Optionen werden geladen…</p> : options.isError ? <p className="social-ai-caution" role="alert">Die Optionen konnten nicht geladen werden.</p> : <div className="social-ai-option-list">
      {choices.map((choice) => <label key={choice.key}><input type="checkbox" checked={options.data[choice.key]} disabled={save.isPending || options.isFetching} onChange={(event) => save.mutate({ ...options.data, [choice.key]: event.target.checked })} /><span>{choice.label}</span><small>{choice.help}</small></label>)}
      <label className="social-ai-language"><span>Sprache für Titel, Beschreibung und Hashtags</span><select value={options.data.publish_language} disabled={save.isPending || options.isFetching} onChange={(event) => save.mutate({ ...options.data, publish_language: event.target.value as AiPublishLanguage })}>{languages.map((language) => <option key={language.value} value={language.value}>{language.label}</option>)}</select><small>Die Untertitel bleiben in der gesprochenen Sprache des Clips.</small></label>
    </div>}
    <div role="status" aria-live="polite">{save.isSuccess && <p>Optionen gespeichert.</p>}{save.isError && <p className="social-ai-caution">Speichern fehlgeschlagen. Bitte versuche es erneut.</p>}</div>
  </section>;
}
