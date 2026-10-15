import { useT } from '../../context/LanguageContext';
import { useEffect, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { STAT_COMMANDS, fetchStatCommandSettings, toggleStatCommand, type StatCommand, type StatCommandSettings } from '../../api/statCommands';
import { CommandRow, SectionCard } from './ui';

const SAVED_MESSAGE = 'Gespeichert.';

export function StatCommandSection() {
  const t = useT();
  const [settings, setSettings] = useState<StatCommandSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Partial<Record<StatCommand, boolean>>>({});
  const [messages, setMessages] = useState<Partial<Record<StatCommand, string>>>({});

  useEffect(() => {
    let active = true;
    fetchStatCommandSettings()
      .then(data => { if (active) setSettings(data.commands); })
      .catch(() => { if (active) setError('Status konnte nicht geladen werden. Bitte Seite neu laden.'); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  const toggle = async (command: StatCommand) => {
    if (!settings || pending[command]) return;
    const enabled = !settings[command];
    setPending(previous => ({ ...previous, [command]: true }));
    setMessages(previous => ({ ...previous, [command]: '' }));
    try {
      const saved = await toggleStatCommand(command, enabled);
      setSettings(previous => previous && ({ ...previous, [saved.command]: saved.enabled }));
      setMessages(previous => ({ ...previous, [command]: SAVED_MESSAGE }));
    } catch {
      setMessages(previous => ({ ...previous, [command]: 'Speichern fehlgeschlagen. Bitte nochmal versuchen.' }));
    } finally {
      setPending(previous => ({ ...previous, [command]: false }));
    }
  };

  return (
    <SectionCard eyebrow={t("Spielstatistik")} description={t("Wähle einzeln, welche Statistikbefehle dein Chat nutzen kann. Aus heißt: Der Bot bleibt bei diesem Befehl still.")}>
      {error && <p role="alert" className="mb-3 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">{t(error)}</p>}
      {loading ? (
        <p className="flex items-center gap-3 text-text-secondary text-sm"><Loader2 className="h-4 w-4 animate-spin text-primary" />{t("Status wird geladen …")}</p>
      ) : !settings ? (
        <p className="text-sm text-text-secondary">{t("Einstellungen gerade nicht verfügbar.")}</p>
      ) : (
        <StatCommandRows settings={settings} pending={pending} messages={messages} onToggle={toggle} />
      )}
    </SectionCard>
  );
}

export function StatCommandRows({ settings, pending, messages, onToggle }: { settings: StatCommandSettings; pending: Partial<Record<StatCommand, boolean>>; messages: Partial<Record<StatCommand, string>>; onToggle: (command: StatCommand) => void }) {
  const t = useT();
  return (
    <div className="grid gap-x-8 md:grid-cols-2">
      {STAT_COMMANDS.map(({ command, description }) => {
        const message = messages[command];
        const label = t(description).replace(/\.$/, '');
        return (
          <CommandRow
            key={command}
            command={command}
            description={t(description)}
            savedAt={message === SAVED_MESSAGE ? 1 : null}
            error={message && message !== SAVED_MESSAGE ? message : null}
            toggle={{ checked: settings[command], onChange: () => onToggle(command), label, pending: pending[command] }}
          />
        );
      })}
    </div>
  );
}
