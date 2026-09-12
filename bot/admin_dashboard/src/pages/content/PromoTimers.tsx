import { useState } from 'react';
import type { PromoTimerProfile, PromoTimerSettings } from '@/api/types';
import { Section } from '@/components/layout/Section';
import { useConfigOverview, usePromoTimerMutation } from '@/hooks/useAdmin';

const fields: { key: keyof PromoTimerProfile; label: string; min: number; max: number }[] = [
  { key: 'overallCooldownMinutes', label: 'Abstand zwischen Werbenachrichten (Minuten)', min: 1, max: 1440 },
  { key: 'activityCooldownMinMinutes', label: 'Kürzester Abstand bei aktivem Chat (Minuten)', min: 1, max: 1440 },
  { key: 'activityCooldownMaxMinutes', label: 'Längster Abstand bei aktivem Chat (Minuten)', min: 1, max: 1440 },
  { key: 'minMessages', label: 'Benötigte Chat-Nachrichten', min: 0, max: 1000 },
  { key: 'newChatters', label: 'Benötigte neue Chatter', min: 0, max: 100 },
  { key: 'attemptCooldownMinutes', label: 'Abstand zwischen Werbeversuchen (Minuten)', min: 1, max: 1440 },
  { key: 'viewerSpikeCooldownMinutes', label: 'Abstand bei Zuschauerzuwachs (Minuten)', min: 1, max: 1440 },
  { key: 'pitchCooldownMinutes', label: 'Abstand zwischen persönlichen Einladungen (Minuten)', min: 1, max: 1440 },
  { key: 'pitchMaxPerStream', label: 'Persönliche Einladungen je Stream höchstens', min: 1, max: 100 },
];

export function PromoTimers() {
  const query = useConfigOverview();
  const mutation = usePromoTimerMutation();
  const [draft, setDraft] = useState<PromoTimerSettings | null>(null);
  const [notice, setNotice] = useState<{ error: boolean; text: string } | null>(null);
  const settings = draft ?? query.data?.timerSettings;

  return (
    <Section title="Community-Werbung und Timer" hint="Zwei getrennte Einstellungen: für unseren Community-Kanal und für alle anderen Streamer. Zeitabstände werden in Minuten gespeichert.">
      {!settings ? (
        <div role="status" className="text-sm text-text-secondary">
          {query.isPending ? 'Timer werden geladen …' : 'Timer-Einstellungen konnten nicht geladen werden.'}
          {!query.isPending && <button type="button" className="admin-button admin-button-secondary ml-3" onClick={() => void query.refetch()}>Erneut laden</button>}
        </div>
      ) : (
        <form onSubmit={async (event) => {
          event.preventDefault();
          if (!draft || mutation.isPending) return;
          if ([draft.community, draft.defaults].some(profile => profile.activityCooldownMinMinutes > profile.activityCooldownMaxMinutes)) {
            setNotice({ error: true, text: 'Der kürzeste Abstand darf nicht größer als der längste Abstand sein.' });
            return;
          }
          setNotice(null);
          try {
            await mutation.mutateAsync(draft);
            setDraft(null);
            setNotice({ error: false, text: 'Timer für unseren Kanal und alle anderen Streamer gespeichert.' });
          } catch (error) {
            setNotice({ error: true, text: error instanceof Error ? error.message : 'Timer konnten nicht gespeichert werden.' });
          }
        }}>
          <fieldset disabled={mutation.isPending} className="grid min-w-0 gap-6 xl:grid-cols-2">
            {(['community', 'defaults'] as const).map(group => (
              <section key={group} aria-labelledby={`timer-${group}-title`} className="min-w-0 rounded-2xl border border-white/10 bg-black/25 p-4 sm:p-5">
                <h3 id={`timer-${group}-title`} className="text-base font-semibold text-white">{group === 'community' ? 'Unser Kanal · dach_lock' : 'Alle anderen Streamer'}</h3>
                <p className="mt-2 text-sm leading-6 text-text-secondary">{group === 'community'
                  ? 'Eigene Abstände für unseren Community-Kanal. Der Timer kann auch bei ruhigem Chat senden; die Chat-Schwellen gelten für Werbung aus dem Chat heraus.'
                  : 'Zentrale Standardwerte für alle übrigen Kanäle. Unser Community-Kanal behält seine eigenen Abstände.'}</p>
                <div className="mt-5 grid gap-4 sm:grid-cols-2">
                  {fields.map(field => (
                    <label key={field.key} className="flex min-w-0 flex-col gap-2 text-sm text-text-secondary">
                      <span>{field.label}</span>
                      <input className="admin-input mt-auto" type="number" required min={field.min} max={field.max} step={1}
                        aria-label={`${group === 'community' ? 'Unser Kanal' : 'Andere Streamer'}: ${field.label}`}
                        value={Number.isNaN(settings[group][field.key]) ? '' : settings[group][field.key]}
                        onChange={event => {
                          const value = event.currentTarget.valueAsNumber;
                          setDraft({ ...settings, [group]: { ...settings[group], [field.key]: value } });
                          setNotice(null);
                        }} />
                    </label>
                  ))}
                </div>
              </section>
            ))}
          </fieldset>
          <div className="mt-5 flex flex-wrap items-center gap-3">
            <button type="submit" className="admin-button admin-button-primary disabled:opacity-50" disabled={!draft || mutation.isPending}>{mutation.isPending ? 'Speichert …' : 'Timer speichern'}</button>
            <button type="button" className="admin-button admin-button-secondary disabled:opacity-50" disabled={!draft || mutation.isPending} onClick={() => { setDraft(null); setNotice(null); }}>Verwerfen</button>
            {draft && <span className="text-sm text-text-secondary">Ungespeicherte Änderungen</span>}
          </div>
          {notice && <p role={notice.error ? 'alert' : 'status'} className={`mt-4 text-sm ${notice.error ? 'text-red-300' : 'text-green-300'}`}>{notice.text}</p>}
        </form>
      )}
    </Section>
  );
}
