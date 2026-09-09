import { useEffect, useState } from 'react';
import { subReminderSettings, type SubReminderSettings } from '../../api/subReminder';

export function SubReminderSection() {
  const [settings, setSettings] = useState<SubReminderSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    subReminderSettings().then(value => { if (active) setSettings(value); })
      .catch(() => { if (active) setError('Status konnte nicht geladen werden. Bitte Seite neu laden.'); });
    return () => { active = false; };
  }, []);
  async function toggle() {
    if (!settings || busy) return;
    setBusy(true); setError('');
    try { setSettings(await subReminderSettings(!settings.enabled)); }
    catch { setError('Speichern fehlgeschlagen. Bitte nochmal versuchen.'); }
    finally { setBusy(false); }
  }
  return <section className="panel-card rounded-2xl p-5 md:p-6">
    <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Chat-Befehl</p>
    <h2 className="display-font text-2xl font-bold text-white mb-1">!sub und Abo-Erinnerungen</h2>
    <p className="text-sm text-text-secondary mb-4">Mit !sub bekommt dein Chat den Abo-Link zu deinem Kanal.</p>
    <p className="text-sm text-text-secondary mb-4">Erinnerungen sind freiwillig und zunächst aus. Wenn du sie einschaltest, kann jeder Zuschauer mit !sub erinnerung an selbst zustimmen. Meldet Twitch sein Abo danach als beendet, erinnert der Bot ihn einmal bei seiner nächsten Chatnachricht öffentlich im Chat. Mit !sub erinnerung aus kann er jederzeit abbestellen.</p>
    {error && <p role="alert" className="text-danger mb-4">{error}</p>}
    {!settings && !error && <p role="status" className="text-text-secondary">Status wird geladen …</p>}
    {settings && <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
      <p role="status" className={settings.enabled ? 'font-bold text-success' : 'font-bold text-text-secondary'}>{settings.enabled ? 'Abo-Erinnerungen sind an' : 'Abo-Erinnerungen sind aus'}</p>
      {!settings.available && <p className="text-sm text-text-secondary mt-2">Der Bot darf deine Abos noch nicht lesen. Verbinde Twitch unter „Konto & Verbindungen“ erneut. Erst dann kannst du Erinnerungen einschalten.</p>}
      <button type="button" disabled={busy || (!settings.available && !settings.enabled)} onClick={() => void toggle()} className="mt-3 rounded-lg border border-primary/40 bg-primary/10 px-5 py-2.5 font-semibold text-primary disabled:opacity-50 disabled:cursor-not-allowed">
        {busy ? 'Wird gespeichert …' : settings.enabled ? 'Erinnerungen ausschalten' : 'Erinnerungen einschalten'}
      </button>
    </div>}
  </section>;
}
