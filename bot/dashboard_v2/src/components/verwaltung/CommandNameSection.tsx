import { useEffect, useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { Loader2, RotateCcw, Save } from 'lucide-react';
import {
  CommandNameApiError,
  fetchCommandNames,
  saveCommandName,
  type CommandNameSaveResult,
  type CommandNameSetting,
} from '../../api/commandNames';

type PendingMap = Record<string, boolean | undefined>;
type MessageMap = Record<string, string | undefined>;
type DraftMap = Record<string, string | undefined>;

function savedRow(row: CommandNameSetting, saved: CommandNameSaveResult): CommandNameSetting {
  return {
    ...row,
    custom_name: saved.custom_name,
    effective_name: saved.effective_name,
    effective_aliases: saved.effective_aliases,
  };
}

function saveError(error: unknown): string {
  if (error instanceof CommandNameApiError) {
    if (error.code === 'name_conflict') return error.message;
    if (error.code === 'invalid_name') return error.message;
  }
  return 'Speichern fehlgeschlagen. Bitte nochmal versuchen.';
}

export function CommandNameSection() {
  const [commands, setCommands] = useState<CommandNameSetting[] | null>(null);
  const [drafts, setDrafts] = useState<DraftMap>({});
  const [pending, setPending] = useState<PendingMap>({});
  const [messages, setMessages] = useState<MessageMap>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    fetchCommandNames()
      .then(({ commands }) => {
        if (!active) return;
        setCommands(commands);
        setDrafts(Object.fromEntries(commands.map(row => [row.command, row.custom_name ?? ''])));
      })
      .catch(() => {
        if (active) setError('Befehlsnamen konnten nicht geladen werden. Bitte Seite neu laden.');
      });
    return () => { active = false; };
  }, []);

  const groups = useMemo(() => {
    const result: Array<{ label: string; commands: CommandNameSetting[] }> = [];
    for (const command of commands ?? []) {
      const existing = result.find(group => group.label === command.group_label);
      if (existing) existing.commands.push(command);
      else result.push({ label: command.group_label, commands: [command] });
    }
    return result;
  }, [commands]);

  const save = async (command: string, name: string | null) => {
    if (pending[command]) return;
    setPending(previous => ({ ...previous, [command]: true }));
    setMessages(previous => ({ ...previous, [command]: '' }));
    try {
      const saved = await saveCommandName(command, name);
      setCommands(previous => previous?.map(row => row.command === command ? savedRow(row, saved) : row) ?? null);
      setDrafts(previous => ({ ...previous, [command]: saved.custom_name ?? '' }));
      setMessages(previous => ({
        ...previous,
        [command]: saved.custom_name ? 'Gespeichert: ' + saved.effective_name : 'Standardnamen wiederhergestellt.',
      }));
    } catch (cause) {
      setMessages(previous => ({ ...previous, [command]: saveError(cause) }));
    } finally {
      setPending(previous => ({ ...previous, [command]: false }));
    }
  };

  return (
    <motion.section
      className="panel-card rounded-2xl p-5 md:p-6"
      initial={{ opacity: 0, y: 16 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.32 }}
    >
      <div className="mb-5">
        <p className="mb-1 text-sm font-medium uppercase tracking-wider text-primary">Chat-Befehle</p>
        <h2 className="display-font mb-1 text-2xl font-bold text-white">Eigene Befehlsnamen</h2>
        <p className="text-sm leading-6 text-text-secondary">
          Passe jeden Bot-Befehl nur für deinen Kanal an. Sobald du einen eigenen Namen setzt,
          reagieren wir für diesen Befehl nicht mehr auf den Standardnamen oder seine Kurzformen.
          So kann zum Beispiel dein vorhandenes !raid unangetastet bleiben.
        </p>
      </div>

      {error && (
        <p role="alert" className="mb-4 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
          {error}
        </p>
      )}

      {!commands && !error ? (
        <p className="flex items-center gap-3 text-sm text-text-secondary">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          Befehlsnamen werden geladen …
        </p>
      ) : commands ? (
        <div className="space-y-6">
          {groups.map(group => (
            <div key={group.label}>
              <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-text-secondary">{group.label}</h3>
              <div className="space-y-3">
                {group.commands.map(row => (
                  <CommandNameRow
                    key={row.command}
                    row={row}
                    draft={drafts[row.command] ?? ''}
                    pending={Boolean(pending[row.command])}
                    message={messages[row.command] ?? ''}
                    onDraft={value => setDrafts(previous => ({ ...previous, [row.command]: value }))}
                    onSave={value => void save(row.command, value)}
                    onReset={() => void save(row.command, null)}
                  />
                ))}
              </div>
            </div>
          ))}
        </div>
      ) : null}
    </motion.section>
  );
}

export function CommandNameRow({
  row,
  draft,
  pending,
  message,
  onDraft,
  onSave,
  onReset,
}: {
  row: CommandNameSetting;
  draft: string;
  pending: boolean;
  message: string;
  onDraft: (value: string) => void;
  onSave: (value: string | null) => void;
  onReset: () => void;
}) {
  const current = row.custom_name ?? '';
  const changed = draft.trim() !== current;
  const defaultAliases = row.default_aliases.length ? row.default_aliases.join(', ') : 'keine';

  return (
    <div className="soft-elevate rounded-xl border border-border bg-background/60 p-4">
      <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(260px,360px)] lg:items-end">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="font-mono text-base font-bold text-white">{row.default_name}</h4>
            {row.custom_name && (
              <span className="rounded-full border border-primary/40 bg-primary/10 px-2 py-0.5 text-[11px] font-semibold text-primary">
                aktiv als {row.effective_name}
              </span>
            )}
          </div>
          <p className="mt-1 text-xs leading-5 text-text-secondary">{row.summary}</p>
          <p className="mt-1 text-[11px] text-text-secondary">
            Standard-Kurzformen: {defaultAliases}
          </p>
        </div>

        <div>
          <label className="block text-xs font-semibold text-text-secondary">
            Eigener Name
            <input
              type="text"
              value={draft}
              maxLength={32}
              spellCheck={false}
              autoCapitalize="none"
              autoCorrect="off"
              placeholder={row.default_name}
              aria-label={row.default_name + ' eigener Name'}
              onChange={event => onDraft(event.target.value)}
              className="mt-1.5 min-h-11 w-full rounded-lg border border-border bg-background px-3 py-2 font-mono text-sm text-white outline-none transition-colors placeholder:text-text-secondary/60 focus:border-primary"
            />
          </label>
          <div className="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              disabled={pending || !changed}
              onClick={() => onSave(draft.trim() ? draft.trim() : null)}
              className="inline-flex min-h-10 items-center gap-2 rounded-lg border border-primary/40 bg-primary/10 px-3 py-2 text-sm font-semibold text-primary transition-colors hover:bg-primary/20 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {pending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
              Speichern
            </button>
            {row.custom_name && (
              <button
                type="button"
                disabled={pending}
                onClick={onReset}
                className="inline-flex min-h-10 items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm font-semibold text-text-secondary transition-colors hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
              >
                <RotateCcw className="h-4 w-4" />
                Zurücksetzen
              </button>
            )}
          </div>
        </div>
      </div>
      {message && <p role="status" className="mt-2 text-sm text-text-secondary">{message}</p>}
    </div>
  );
}
