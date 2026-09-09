import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { Loader2, Power, PowerOff } from 'lucide-react';
import { STAT_COMMANDS, fetchStatCommandSettings, toggleStatCommand, type StatCommand, type StatCommandSettings } from '../../api/statCommands';

export function StatCommandSection() {
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
      setMessages(previous => ({ ...previous, [command]: 'Gespeichert.' }));
    } catch {
      setMessages(previous => ({ ...previous, [command]: 'Speichern fehlgeschlagen. Bitte nochmal versuchen.' }));
    } finally {
      setPending(previous => ({ ...previous, [command]: false }));
    }
  };

  return (
    <motion.section className="panel-card rounded-2xl p-5 md:p-6" initial={{ opacity: 0, y: 16 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true }} transition={{ duration: 0.32 }}>
      <div className="mb-5">
        <p className="text-sm uppercase tracking-wider font-medium text-primary mb-1">Chat-Befehle</p>
        <h2 className="display-font text-2xl font-bold text-white mb-1">Spielstatistik im Chat</h2>
        <p className="text-sm text-text-secondary">Wähle einzeln, welche Statistikbefehle dein Chat nutzen kann. Aus heißt: Der Bot bleibt bei diesem Befehl und seinen Kurzformen still.</p>
      </div>
      {error && <p role="alert" className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">{error}</p>}
      {loading ? (
        <p className="flex items-center gap-3 text-text-secondary text-sm"><Loader2 className="h-4 w-4 animate-spin text-primary" />Status wird geladen …</p>
      ) : !settings ? (
        <p className="text-sm text-text-secondary">Einstellungen gerade nicht verfügbar.</p>
      ) : (
        <StatCommandRows settings={settings} pending={pending} messages={messages} onToggle={toggle} />
      )}
    </motion.section>
  );
}

export function StatCommandRows({ settings, pending, messages, onToggle }: { settings: StatCommandSettings; pending: Partial<Record<StatCommand, boolean>>; messages: Partial<Record<StatCommand, string>>; onToggle: (command: StatCommand) => void }) {
  return (
        <div className="space-y-3">
          {STAT_COMMANDS.map(({ command, label, description }) => {
            const enabled = settings[command];
            return (
              <div key={command} className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
                <div className="flex items-center justify-between gap-4 flex-wrap">
                  <div>
                    <h3 className={`text-base font-bold ${enabled ? 'text-success' : 'text-text-secondary'}`}>{label} {enabled ? 'ist aktiv' : 'ist aus'}</h3>
                    <p className="text-xs text-text-secondary mt-0.5">{description}</p>
                  </div>
                  <button type="button" disabled={pending[command]} onClick={() => void onToggle(command)} aria-label={`${label} ${enabled ? 'deaktivieren' : 'aktivieren'}`} className={`inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-semibold transition-colors ${enabled ? 'border border-danger/40 bg-danger/10 text-danger hover:bg-danger/20' : 'border border-primary/40 bg-primary/10 text-primary hover:bg-primary/20'} disabled:opacity-50 disabled:cursor-not-allowed`}>
                    {pending[command] ? <Loader2 className="h-4 w-4 animate-spin" /> : enabled ? <PowerOff className="h-4 w-4" /> : <Power className="h-4 w-4" />}
                    {enabled ? 'Deaktivieren' : 'Aktivieren'}
                  </button>
                </div>
                {messages[command] && <p role="status" className="mt-2 text-sm text-text-secondary">{messages[command]}</p>}
              </div>
            );
          })}
        </div>
  );
}
