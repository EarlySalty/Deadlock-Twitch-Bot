import { translateManagementError } from '../../i18n/managementErrors';
import { useT } from '../../context/LanguageContext';
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { Loader2, Plus, X } from 'lucide-react';
import { SavedIndicator } from './SavedIndicator';
import {
  CommandNameApiError,
  fetchCommandNames,
  saveCommandName,
  saveCommandAliases,
  applyNameSave,
  applyAliasSave,
  MAX_EXTRA_COMMAND_NAMES,
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
  updateAliases: (command: string, aliases: string[]) => Promise<void>;
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
    setCommands(previous => previous.map(row => row.command === command ? applyNameSave(row, saved) : row));
  }, []);

  const updateAliases = useCallback(async (command: string, aliases: string[]) => {
    const saved = await saveCommandAliases(command, aliases);
    setCommands(previous => previous.map(row => row.command === command ? applyAliasSave(row, saved) : row));
  }, []);

  return <CommandNamesContext.Provider value={{ commands, loading, error, update, updateAliases }}>{children}</CommandNamesContext.Provider>;
}

// eslint-disable-next-line react-refresh/only-export-components
export function useCommandNames() {
  const context = useContext(CommandNamesContext);
  if (!context) throw new Error('CommandNamesProvider fehlt');
  return context;
}

function messageFor(error: unknown): string {
  if (error instanceof CommandNameApiError && (error.code === 'name_conflict' || error.code === 'invalid_name' || error.code === 'too_many_aliases')) {
    return error.message;
  }
  return 'Speichern fehlgeschlagen. Bitte erneut tippen.';
}

type ExtraMessage = { text: string; error: boolean };

function useExtraNames(command: string, saved: string[] | undefined) {
  const { updateAliases } = useCommandNames();
  const [drafts, setDrafts] = useState<string[] | null>(null);
  const [message, setMessage] = useState<ExtraMessage>({ text: '', error: false });
  const latest = useRef<string[] | null>(null);
  const dirty = useRef(false);
  const tail = useRef<Promise<void>>(Promise.resolve());
  const mounted = useRef(true);
  const values = drafts ?? (saved ?? []).map(name => name.slice(1));

  const flush = useCallback(() => {
    const requested = latest.current;
    if (!dirty.current || requested === null) return;
    const names = requested.filter(name => name !== '');
    if (!names.every(validCommandNameInput)) return;
    dirty.current = false;
    if (mounted.current) setMessage({ text: 'Wird gespeichert …', error: false });
    tail.current = tail.current.catch(() => {}).then(() => updateAliases(command, names.map(name => `!${name}`)));
    void tail.current.then(() => {
      if (!mounted.current || latest.current !== requested) return;
      setMessage({ text: 'Gespeichert.', error: false });
      if (!requested.includes('')) setDrafts(null);
    }).catch(cause => {
      if (latest.current === requested) dirty.current = true;
      if (mounted.current) setMessage({ text: messageFor(cause), error: true });
    });
  }, [command, updateAliases]);

  const edit = useCallback((next: string[]) => {
    latest.current = next;
    dirty.current = true;
    setDrafts(next);
    const invalid = next.some(name => !validCommandNameInput(name));
    setMessage(invalid ? { text: 'Nur Buchstaben und Zahlen eingeben; das ! steht bereits davor.', error: true } : { text: '', error: false });
  }, []);

  useEffect(() => {
    if (drafts === null || !dirty.current) return;
    const timer = window.setTimeout(flush, 650);
    return () => window.clearTimeout(timer);
  }, [drafts, flush]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      flush();
    };
  }, [flush]);

  return {
    values,
    message,
    flush,
    add: () => {
      if (values.length >= MAX_EXTRA_COMMAND_NAMES) return;
      latest.current = [...values, ''];
      setDrafts(latest.current);
    },
    change: (index: number, value: string) => edit(values.map((name, position) => position === index ? value : name)),
    remove: (index: number) => {
      const removed = values[index];
      edit(values.filter((_, position) => position !== index));
      if (removed === '') dirty.current = false;
      else window.setTimeout(flush, 0);
    },
  };
}

export function EditableCommandName({ command, className = '' }: { command: string; className?: string }) {
  const t = useT();
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

  const extras = useExtraNames(command, row?.custom_aliases);

  if (loading) return <span className="text-xs text-text-secondary">{t("Befehl wird geladen …")}</span>;
  if (error || !row) return <span role="alert" className="text-xs text-danger">{t(error || 'Befehl nicht verfügbar.')}</span>;

  return (
    <div className={className}>
      <div className="flex items-center gap-1.5">
      <label className="flex min-h-10 min-w-0 flex-1 items-center rounded-lg border border-primary/50 bg-[#0d0806] px-2 font-mono text-[13px] font-bold text-white shadow-inner transition-colors focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/25">
        <span aria-hidden="true" className="select-none text-primary">!</span>
        <input
          type="text"
          inputMode="text"
          pattern="[A-Za-z0-9]+"
          maxLength={31}
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          aria-label={t('{command} Befehlsname ändern', { command: row.default_name })}
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
          style={{ width: `${Math.max(value.length, 1) + 1}ch` }}
          className="min-w-0 flex-auto bg-transparent py-2 outline-none placeholder:text-text-secondary/60"
        />
        {pending && <Loader2 aria-label={t("Wird gespeichert")} className="ml-1 h-3.5 w-3.5 animate-spin text-primary" />}
      </label>
      <button
        type="button"
        onClick={extras.add}
        disabled={extras.values.length >= MAX_EXTRA_COMMAND_NAMES}
        aria-label={t('Weiteren Namen für {command} hinzufügen', { command: row.effective_name })}
        title={t('Weiteren Namen für {command} hinzufügen', { command: row.effective_name })}
        className="flex h-10 w-8 shrink-0 items-center justify-center rounded-lg border border-primary/40 text-primary transition-colors hover:border-primary hover:bg-primary/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 disabled:cursor-not-allowed disabled:opacity-30"
      >
        <Plus className="h-4 w-4" aria-hidden="true" />
      </button>
      </div>
      {extras.values.map((value, index) => (
        <div key={index} className="mt-1.5 flex items-center gap-1.5">
          <label className="flex min-h-8 min-w-0 flex-1 items-center rounded-lg border border-primary/30 bg-[#0d0806] px-2 font-mono text-[12px] font-bold text-white shadow-inner transition-colors focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/25">
            <span aria-hidden="true" className="select-none text-primary">!</span>
            <input
              type="text"
              inputMode="text"
              pattern="[A-Za-z0-9]+"
              maxLength={31}
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
              autoFocus={value === '' && index === extras.values.length - 1}
              aria-label={t('Zusatzname {name} für {command}', { name: String(index + 1), command: row.effective_name })}
              aria-invalid={!validCommandNameInput(value)}
              value={value}
              onBlur={extras.flush}
              onChange={event => extras.change(index, event.target.value)}
              style={{ width: `${Math.max(value.length, 1) + 1}ch` }}
              className="min-w-0 flex-auto bg-transparent py-1.5 outline-none"
            />
          </label>
          <button
            type="button"
            onClick={() => extras.remove(index)}
            aria-label={t('Zusatzname {name} entfernen', { name: value ? `!${value}` : String(index + 1) })}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-border/70 text-text-secondary transition-colors hover:border-danger/60 hover:text-danger focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
          >
            <X className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </div>
      ))}
      {extras.message.text === 'Gespeichert.' ? <SavedIndicator className="mt-1 block" /> : extras.message.text && <p role={extras.message.error ? 'alert' : 'status'} className={`mt-1 text-xs ${extras.message.error ? 'text-danger' : 'text-text-secondary'}`}>{translateManagementError(extras.message.text, t)}</p>}
      {message === 'Gespeichert.' ? <SavedIndicator className="mt-1 block" /> : message && message !== 'Wird gespeichert …' && <p role={validCommandNameInput(draft ?? '') ? 'status' : 'alert'} className={`mt-1 text-xs ${validCommandNameInput(draft ?? '') ? 'text-text-secondary' : 'text-danger'}`}>{translateManagementError(message, t)}</p>}
    </div>
  );
}
