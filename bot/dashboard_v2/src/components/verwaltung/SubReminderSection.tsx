import { useT } from '../../context/LanguageContext';
import { useEffect, useState } from 'react';
import { subReminderSettings, type SubReminderSettings } from '../../api/subReminder';
import { useCommandNames } from './CommandNameSection';
import { CommandRow, SectionCard } from './ui';

export function SubReminderSection() {
  const t = useT();
  const { commands } = useCommandNames();
  const subName = commands.find(row => row.command === 'sub')?.effective_name ?? '!sub';
  const [settings, setSettings] = useState<SubReminderSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [savedAt, setSavedAt] = useState<number | null>(null);
  useEffect(() => {
    let active = true;
    subReminderSettings().then(value => { if (active) setSettings(value); })
      .catch(() => { if (active) setError('Status konnte nicht geladen werden. Bitte Seite neu laden.'); });
    return () => { active = false; };
  }, []);
  async function toggle() {
    if (!settings || busy) return;
    setBusy(true); setError(''); setSavedAt(null);
    try { setSettings(await subReminderSettings(!settings.enabled)); setSavedAt(Date.now()); }
    catch { setError('Speichern fehlgeschlagen. Bitte nochmal versuchen.'); }
    finally { setBusy(false); }
  }
  return <SectionCard eyebrow={t("Chat-Befehl")} title={t("Abo-Link und Erinnerungen")}>
    <p className="text-sm text-text-secondary mb-2">{t("Mit")} {subName} {t("bekommt dein Chat den Abo-Link zu deinem Kanal.")}</p>
    <p className="text-sm text-text-secondary mb-2">{t("Erinnerungen sind freiwillig und zunächst aus. Wenn du sie einschaltest, kann jeder Zuschauer mit")} {subName} {t("erinnerung an selbst zustimmen. Meldet Twitch sein Abo danach als beendet, erinnert der Bot ihn einmal bei seiner nächsten Chatnachricht öffentlich im Chat. Mit")} {subName} {t("erinnerung aus kann er jederzeit abbestellen.")}</p>
    {!settings && !error && <p role="status" className="text-sm text-text-secondary">{t("Status wird geladen …")}</p>}
    {!settings && error && <p role="alert" className="text-sm text-danger">{t(error)}</p>}
    {settings && <CommandRow
      command="sub"
      title={t("Abo-Erinnerungen")}
      description={!settings.available ? t("Der Bot darf deine Abos noch nicht lesen. Verbinde Twitch unter „Konto & Verbindungen“ erneut. Erst dann kannst du Erinnerungen einschalten.") : undefined}
      savedAt={savedAt}
      error={error || null}
      toggle={{ checked: settings.enabled, onChange: () => void toggle(), label: t("Abo-Erinnerungen"), pending: busy, disabled: !settings.available && !settings.enabled }}
    />}
  </SectionCard>;
}
