import { useT } from '../../context/LanguageContext';
import { useEffect, useState } from 'react';
import { BellOff, Loader2, MessageSquare, ShieldAlert } from 'lucide-react';
import {
  fetchSilentSettings,
  saveSilentSettings,
  type SilentSettings,
} from '@/api/silentNotifications';
import { CommandRow, SectionCard } from './ui';

type FlagKey = keyof SilentSettings;

const ROWS: Array<{
  key: FlagKey;
  icon: typeof ShieldAlert;
  title: string;
  desc: string;
}> = [
  {
    key: 'silent_ban',
    icon: ShieldAlert,
    title: 'Auto-Ban-Hinweise stummschalten',
    desc: 'Wenn aktiv, postet der Bot keine Chat-Notiz mehr, sobald er jemanden automatisch bannt.',
  },
  {
    key: 'silent_raid',
    icon: MessageSquare,
    title: 'Raid-Hinweise stummschalten',
    desc: 'Wenn aktiv, postet der Bot keine Chat-Notiz mehr zu automatischen Raids.',
  },
];

function markSaved(key: FlagKey) {
  return { key, at: Date.now() };
}

export function SilentNotificationsSection() {
  const t = useT();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState<SilentSettings>({
    silent_ban: false,
    silent_raid: false,
  });
  const [pendingKey, setPendingKey] = useState<FlagKey | null>(null);
  const [savedKey, setSavedKey] = useState<{ key: FlagKey; at: number } | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchSilentSettings();
      setSettings({
        silent_ban: Boolean(data.silent_ban),
        silent_raid: Boolean(data.silent_raid),
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Unbekannter Fehler');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const onToggle = async (key: FlagKey) => {
    if (pendingKey) return;
    const next = { ...settings, [key]: !settings[key] };
    setPendingKey(key);
    setError(null);
    setSavedKey(null);
    setSettings(next);
    try {
      const saved = await saveSilentSettings(next);
      setSettings({
        silent_ban: Boolean(saved.silent_ban),
        silent_raid: Boolean(saved.silent_raid),
      });
      setSavedKey(markSaved(key));
    } catch (e) {
      setSettings(settings);
      setError(e instanceof Error ? e.message : 'Speichern fehlgeschlagen');
    } finally {
      setPendingKey(null);
    }
  };

  return (
    <SectionCard
      eyebrow={t("Chat-Benachrichtigungen")}
      icon={<BellOff className="h-4 w-4" />}
      title={t("Stille Hinweise")}
      description={t("Steuere, ob der Bot Chat-Notizen zu Auto-Bans und Raids postet. Das geht auch mit den Chat-Befehlen in den Feldern daneben.")}
    >
      {error && <p role="alert" className="mb-2 text-sm text-danger">{t(error)}</p>}
      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          {t("Einstellungen werden geladen ...")}
        </div>
      ) : (
        <div>
          {ROWS.map(({ key, icon: Icon, title, desc }) => (
            <CommandRow
              key={key}
              command={key === 'silent_ban' ? 'silentban' : 'silentraid'}
              inputSide="end"
              icon={<Icon className={`mt-0.5 h-5 w-5 shrink-0 ${settings[key] ? 'text-primary' : 'text-text-secondary'}`} />}
              title={t(title)}
              description={t(desc)}
              savedAt={savedKey?.key === key ? savedKey.at : null}
              toggle={{ checked: settings[key], onChange: () => void onToggle(key), label: t(title), pending: pendingKey === key, disabled: pendingKey !== null }}
            />
          ))}
        </div>
      )}
    </SectionCard>
  );
}
