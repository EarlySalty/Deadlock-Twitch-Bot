import { useT } from '../../context/LanguageContext';
import { useEffect, useState } from 'react';
import { Loader2, ShieldCheck } from 'lucide-react';
import { CommandRow, SectionCard } from './ui';
import {
  fetchModerationSettings,
  saveModerationSettings,
  type ModerationSettings,
} from '@/api/moderation';

type ModerationKey = keyof ModerationSettings;

const TASKS: Array<{ key: ModerationKey; label: string; desc: string }> = [
  {
    key: 'global_ban_enabled',
    label: 'Bekannte Betrüger fernhalten',
    desc: 'Accounts, die in der Community schon als Betrüger aufgefallen sind, kommen in deinem Chat gar nicht erst zum Zug.',
  },
  {
    key: 'scam_pitch_enabled',
    label: 'Vor Abzock-Maschen warnen',
    desc: 'Der Bot erkennt typische Anwerb- und Abzock-Nachrichten und warnt frühzeitig davor.',
  },
  {
    key: 'spam_autoban_enabled',
    label: 'Werbe-Spam automatisch stoppen',
    desc: 'Offensichtliche Werbung und Viewer-Kauf-Nachrichten fliegen automatisch aus deinem Chat.',
  },
  {
    key: 'sus_invite_enabled',
    label: 'Verdächtige Discord-Einladungen bremsen',
    desc: 'Fremde Discord-Einladungen von ganz neuen Zuschauern werden kurz ausgebremst, damit niemand in eine Falle gelockt wird.',
  },
];

function markSaved(key: ModerationKey) {
  return { key, at: Date.now() };
}

export function ModerationSection() {
  const t = useT();
  const [loading, setLoading] = useState(true);
  const [pendingKey, setPendingKey] = useState<ModerationKey | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [savedKey, setSavedKey] = useState<{ key: ModerationKey; at: number } | null>(null);
  const [settings, setSettings] = useState<ModerationSettings | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    fetchModerationSettings()
      .then((data) => { if (active) setSettings(data); })
      .catch((e) => active && setError(e instanceof Error ? e.message : 'Unbekannter Fehler'))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, []);

  const toggle = async (key: ModerationKey) => {
    if (!settings || pendingKey) return;
    const previous = settings;
    const next = { ...settings, [key]: !settings[key] };
    setSettings(next);
    setPendingKey(key);
    setError(null);
    setSavedKey(null);
    try {
      setSettings(await saveModerationSettings(next));
      setSavedKey(markSaved(key));
    } catch (e) {
      setSettings(previous);
      setError(e instanceof Error ? e.message : 'Speichern fehlgeschlagen');
    } finally {
      setPendingKey(null);
    }
  };

  return (
    <SectionCard
      eyebrow="Moderation"
      icon={<ShieldCheck className="h-4 w-4" />}
      title={t("Was der Bot übernimmt")}
      description={t("Lege fest, welche Aufgaben der Bot in deinem Chat automatisch übernimmt. Alles ist von Haus aus an. Schaltest du etwas aus, lässt der Bot diesen Bereich in Ruhe.")}
      attrs={{ tabIndex: -1, 'data-tour-id': 'onboarding-bot', 'data-tour-ready': !loading && settings !== null }}
    >
      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          {t("Einstellungen werden geladen ...")}
        </div>
      ) : !settings ? (
        <p role="alert" className="text-sm text-danger">{t(error ?? 'Einstellungen konnten nicht geladen werden.')}</p>
      ) : (
        <div>
          {error && <p role="alert" className="mb-2 text-sm text-danger">{t(error)}</p>}
          {TASKS.map((task) => (
            <CommandRow
              key={task.key}
              title={t(task.label)}
              description={t(task.desc)}
              inputSide="end"
              savedAt={savedKey?.key === task.key ? savedKey.at : null}
              toggle={{ checked: settings[task.key], onChange: () => void toggle(task.key), label: t(task.label), pending: pendingKey === task.key, disabled: pendingKey !== null }}
            />
          ))}
        </div>
      )}
    </SectionCard>
  );
}
