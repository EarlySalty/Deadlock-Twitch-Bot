import { useState } from 'react';
import type { CommunityAnnouncement, CommunityAnnouncements } from '@/api/types';
import { Section } from '@/components/layout/Section';
import { useCommunityAnnouncementsMutation, useConfigOverview } from '@/hooks/useAdmin';

const colors: { value: CommunityAnnouncement['color']; label: string; swatch: string }[] = [
  { value: 'purple', label: 'Lila', swatch: '#a970ff' },
  { value: 'blue', label: 'Blau', swatch: '#3b82f6' },
  { value: 'green', label: 'Grün', swatch: '#22c55e' },
  { value: 'orange', label: 'Orange', swatch: '#f97316' },
  { value: 'primary', label: 'Kanalfarbe', swatch: '#d4af37' },
];

export function CommunityAnnouncementsEditor() {
  const query = useConfigOverview();
  const mutation = useCommunityAnnouncementsMutation();
  const [draft, setDraft] = useState<CommunityAnnouncements | null>(null);
  const [notice, setNotice] = useState<{ error: boolean; text: string } | null>(null);
  const [conflict, setConflict] = useState(false);
  const settings = draft ?? query.data?.communityAnnouncements;
  function change(value: CommunityAnnouncements) { setDraft(value); setNotice(null); }
  function changeEntry(index: number, patch: Partial<CommunityAnnouncement>) {
    if (settings) change({ ...settings, entries: settings.entries.map((entry, i) => i === index ? { ...entry, ...patch } : entry) });
  }
  async function reload() {
    const result = await query.refetch();
    if (!result.isError) { setDraft(null); setConflict(false); setNotice(null); }
    else setNotice({ error: true, text: 'Ankündigungen konnten nicht neu geladen werden. Deine Änderungen bleiben erhalten.' });
  }
  return <Section title="Ankündigungen für dach_lock" hint="Diese Texte wechseln sich auf unserem Twitch-Kanal ab. Neue Ankündigungen sind zunächst deaktiviert. Änderungen gelten nach dem Speichern für den nächsten Versand.">
    {!settings ? <p role="status" className="text-sm text-text-secondary">{query.isPending ? 'Ankündigungen werden geladen …' : 'Ankündigungen konnten nicht geladen werden.'}</p> :
      <form onSubmit={async event => {
        event.preventDefault();
        if (!draft || mutation.isPending || conflict) return;
        setNotice(null);
        try {
          await mutation.mutateAsync(draft);
          setDraft(null);
          setNotice({ error: false, text: 'Ankündigungen für dach_lock gespeichert.' });
        } catch (error) {
          setConflict(error instanceof Error && 'status' in error && error.status === 409);
          setNotice({ error: true, text: error instanceof Error ? error.message : 'Ankündigungen konnten nicht gespeichert werden.' });
        }
      }}>
        <fieldset disabled={mutation.isPending} className="min-w-0 space-y-5">
          <label className="flex items-center gap-3 text-sm font-semibold text-white">
            <input type="checkbox" className="h-5 w-5 accent-[#d4af37]" checked={settings.enabled} onChange={event => change({ ...settings, enabled: event.target.checked })} />
            Ankündigungen für dach_lock aktiv
          </label>
          <p className="text-sm text-text-secondary">{settings.enabled ? `${settings.entries.filter(entry => entry.enabled).length} Kanaltexte aktiv. Die Zeitabstände stellst du unten unter „Unser Kanal · dach_lock“ ein.` : 'Alle Ankündigungen aus dieser Rotation sind pausiert, auch globale Event-Einblendungen.'}</p>
          <label className="flex items-start gap-3 text-sm text-white">
            <input type="checkbox" className="mt-0.5 h-5 w-5 shrink-0 accent-[#d4af37]" checked={settings.includeGlobalEvent} onChange={event => change({ ...settings, includeGlobalEvent: event.target.checked })} />
            <span>Globale Event-Ankündigung auch auf dach_lock einblenden<span className="mt-1 block text-text-secondary">Wenn oben ein globales Event aktiv ist, erscheint es einmal pro Runde. Dieser Schalter ändert nur dach_lock; Text und Zeitfenster des globalen Events gelten weiterhin für alle Kanäle.</span></span>
          </label>
          {settings.enabled && !settings.entries.some(entry => entry.enabled) && <p role="status" className="text-sm text-amber-200">{settings.includeGlobalEvent ? 'Keine Kanaltexte aktiv. Nur ein aktives globales Event kann noch eingeblendet werden.' : 'Keine Ankündigung aktiv. Der Bot sendet aus dieser Rotation nichts.'}</p>}
          <div className="grid min-w-0 gap-5 xl:grid-cols-2">
            {settings.entries.map((entry, index) => <article key={index} aria-label={`Kanal-Ankündigung ${index + 1}`} className="min-w-0 rounded-2xl border border-[#d4af37]/50 bg-black/40 p-5 shadow-lg">
              <div className="mb-4 flex items-center justify-between gap-3">
                <h3 className="font-semibold text-white">Ankündigung {index + 1}</h3>
                <label className="flex items-center gap-2 text-sm text-white">
                  <input type="checkbox" className="h-4 w-4 accent-[#d4af37]" aria-label={`Ankündigung ${index + 1} aktiv`} checked={entry.enabled} onChange={event => changeEntry(index, { enabled: event.target.checked })} />
                  {entry.enabled ? 'Aktiv' : 'Deaktiviert'}
                </label>
              </div>
              <label className="block text-sm text-text-secondary">Text
                <textarea className="admin-input mt-2 min-h-32 w-full resize-y" required maxLength={450} value={entry.text} aria-label={`Text für Ankündigung ${index + 1}`} onChange={event => changeEntry(index, { text: event.target.value })} />
              </label>
              <p className="mt-2 text-xs leading-5 text-text-secondary">{Array.from(entry.text).length}/450 Zeichen · {'{invite}'} fügt einmal unseren Discord-Einladungslink ein (höchstens 400 weitere Zeichen). Einzeiliger Text; andere Platzhalter sind nicht erlaubt.</p>
              <label className="mt-4 block text-sm text-text-secondary">Farbe
                <select className="admin-input mt-2" value={entry.color} aria-label={`Farbe für Ankündigung ${index + 1}`} onChange={event => changeEntry(index, { color: event.target.value as CommunityAnnouncement['color'] })}>
                  {colors.map(color => <option key={color.value} value={color.value}>{color.label}</option>)}
                </select>
              </label>
              <div className="mt-4 break-words rounded-lg border-l-4 bg-black/50 p-3 text-sm leading-6 text-white" style={{ borderLeftColor: colors.find(color => color.value === entry.color)?.swatch }}>
                <span className="mb-1 block text-xs text-text-secondary">Vorschau</span>{entry.text.replaceAll('{invite}', '[Discord-Einladungslink]') || 'Dein Ankündigungstext erscheint hier.'}
              </div>
            </article>)}
          </div>
          {!settings.entries.length && <p className="text-sm text-text-secondary">Noch keine Kanaltexte vorhanden. Füge die erste Ankündigung hinzu.</p>}
          <button type="button" className="admin-button admin-button-secondary disabled:opacity-50" disabled={settings.entries.length >= 50} onClick={() => change({ ...settings, entries: [...settings.entries, { text: '', enabled: false, color: 'purple' }] })}>Neue Ankündigung hinzufügen</button>
        </fieldset>
        <div className="mt-6 flex flex-wrap items-center gap-3">
          <button type="submit" className="admin-button admin-button-primary disabled:opacity-50" disabled={!draft || mutation.isPending || conflict}>{mutation.isPending ? 'Speichert …' : 'Kanal-Ankündigungen speichern'}</button>
          <button type="button" className="admin-button admin-button-secondary disabled:opacity-50" disabled={!draft || mutation.isPending} onClick={() => void reload()}>{conflict ? 'Aktuellen Stand laden und Entwurf verwerfen' : 'Verwerfen'}</button>
          {draft && <span className="text-sm text-text-secondary">Ungespeicherte Änderungen</span>}
        </div>
        {notice && <p role={notice.error ? 'alert' : 'status'} className={`mt-4 text-sm ${notice.error ? 'text-red-300' : 'text-green-300'}`}>{notice.text}</p>}
      </form>}
  </Section>;
}
