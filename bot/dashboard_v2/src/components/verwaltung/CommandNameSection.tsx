import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { Loader2 } from 'lucide-react';
import {
  CommandNameApiError,
  fetchCommandNames,
  saveCommandName,
  type CommandNameSetting,
  validCommandNameInput,
  enqueueCommandNameSave,
  type CommandNameSaveQueue,
} from '../../api/commandNames';

type CommandNamesContextValue = {
  commands: CommandNameSetting[];
  loading: boolean;
  error: string;
  update: (command: string, name: string | null) => Promise<void>;
};

const CommandNamesContext = createContext<CommandNamesContextValue | null>(null);

export function CommandNamesProvider({ children }: { children: ReactNode }) {
  const [commands, setCommands] = useState<CommandNameSetting[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    let active = true;
    fetchCommandNames()
      .then(result => { if (active) setCommands(result.commands); })
      .catch(() => { if (active) setError('Befehlsnamen konnten nicht geladen werden. Bitte Seite neu laden.'); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  const update = useCallback(async (command: string, name: string | null) => {
    const saved = await saveCommandName(command, name);
    setCommands(previous => previous.map(row => row.command === command ? {
      ...row,
      custom_name: saved.custom_name,
      effective_name: saved.effective_name,
      effective_aliases: saved.effective_aliases,
    } : row));
  }, []);

  return <CommandNamesContext.Provider value={{ commands, loading, error, update }}>{children}</CommandNamesContext.Provider>;
}

// Shared by the inline editor and the remaining command list.
// eslint-disable-next-line react-refresh/only-export-components
export function useCommandNames() {
  const context = useContext(CommandNamesContext);
  if (!context) throw new Error('CommandNamesProvider fehlt');
  return context;
}

function messageFor(error: unknown): string {
  if (error instanceof CommandNameApiError && (error.code === 'name_conflict' || error.code === 'invalid_name')) {
    return error.message;
  }
  return 'Speichern fehlgeschlagen. Bitte erneut tippen.';
}

export function EditableCommandName({ command, className = '' }: { command: string; className?: string }) {
  const { commands, loading, error, update } = useCommandNames();
  const row = commands.find(item => item.command === command);
  const [draft, setDraft] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [message, setMessage] = useState('');
  const latestDraft = useRef<string | null>(null);
  const saveQueue = useRef<CommandNameSaveQueue>({ savedName: '', lastAttempt: null, queued: 0, tail: Promise.resolve() });
  const mounted = useRef(true);
  const savedName = row?.effective_name.slice(1) ?? '';
  const value = draft ?? savedName;
  useEffect(() => { saveQueue.current.savedName = savedName; }, [savedName]);

  const saveNow = useCallback(() => {
    const requested = latestDraft.current;
    const queue = saveQueue.current;
    const operation = enqueueCommandNameSave(queue, requested, name =>
      update(command, name ? `!${name}` : null));
    if (!operation) return;
    if (mounted.current) {
      setPending(true);
      setMessage('Wird gespeichert …');
    }
    void operation.then(() => {
      if (mounted.current && latestDraft.current === requested) {
        setMessage('Gespeichert.');
        setDraft(null);
      }
    }).catch(cause => {
      if (mounted.current && latestDraft.current === requested) setMessage(messageFor(cause));
    }).finally(() => {
      if (mounted.current) setPending(queue.queued > 0);
    });
  }, [command, update]);

  useEffect(() => {
    if (draft === null || !validCommandNameInput(draft)) return;
    const timer = window.setTimeout(saveNow, 650);
    return () => window.clearTimeout(timer);
  }, [draft, saveNow]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      saveNow();
    };
  }, [saveNow]);

  if (loading) return <span className="text-xs text-text-secondary">Befehl wird geladen …</span>;
  if (error || !row) return <span role="alert" className="text-xs text-danger">{error || 'Befehl nicht verfügbar.'}</span>;

  return (
    <div className={className}>
      <label className="inline-flex min-h-11 max-w-full items-center rounded-lg border border-primary/50 bg-[#0d0806] px-3 font-mono text-base font-bold text-white shadow-inner transition-colors focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/25">
        <span aria-hidden="true" className="select-none text-primary">!</span>
        <input
          type="text"
          inputMode="text"
          pattern="[A-Za-z0-9]+"
          maxLength={31}
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          aria-label={`${row.default_name} Befehlsname ändern`}
          aria-describedby={`command-name-hint-${command}`}
          value={value}
          onBlur={saveNow}
          aria-invalid={draft !== null && !validCommandNameInput(draft)}
          onChange={event => {
            const next = event.target.value;
            latestDraft.current = next;
            saveQueue.current.lastAttempt = null;
            setDraft(next);
            setMessage(validCommandNameInput(next) ? '' : 'Nur Buchstaben und Zahlen eingeben; das ! steht bereits davor.');
          }}
          className="min-w-0 w-[12ch] max-w-[22ch] bg-transparent py-2 outline-none placeholder:text-text-secondary/60"
        />
        {pending && <Loader2 aria-label="Wird gespeichert" className="ml-1 h-3.5 w-3.5 animate-spin text-primary" />}
      </label>
      <p id={`command-name-hint-${command}`} className="mt-1 text-[11px] text-text-secondary">
        Namen ändern · speichert automatisch · nur Buchstaben und Zahlen
      </p>
      {message && <p role={validCommandNameInput(draft ?? '') ? 'status' : 'alert'} className={`mt-1 text-xs ${validCommandNameInput(draft ?? '') ? 'text-text-secondary' : 'text-danger'}`}>{message}</p>}
    </div>
  );
}

export function OtherCommandNames({ excluded }: { excluded: string[] }) {
  const { commands, loading, error } = useCommandNames();
  const remaining = commands.filter(row => !excluded.includes(row.command));
  if (loading) return <p className="text-sm text-text-secondary">Weitere Befehle werden geladen …</p>;
  if (error) return <p role="alert" className="text-sm text-danger">{error}</p>;
  if (!remaining.length) return null;
  return (
    <div className="mt-2">
      <h3 className="mb-3 text-lg font-bold text-white">Weitere Chat-Befehle</h3>
      <div className="grid gap-3 md:grid-cols-2">
        {remaining.map(row => (
          <div key={row.command} className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
            <EditableCommandName command={row.command} />
            <p className="mt-2 text-xs leading-5 text-text-secondary">{row.summary.split(row.default_name).join(row.effective_name)}</p>
          </div>
        ))}
      </div>
    </div>
  );
}
