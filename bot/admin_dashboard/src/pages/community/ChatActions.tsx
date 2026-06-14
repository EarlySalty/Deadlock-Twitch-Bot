import { ChevronRight, Megaphone, SearchX, Send } from 'lucide-react';
import { useDeferredValue, useMemo, useState } from 'react';
import type { PartnerChatActionMode, StreamerRow } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { EmptyState } from '@/components/shared/EmptyState';
import { SearchInput } from '@/components/shared/SearchInput';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { Toast } from '@/components/shared/Toast';
import { usePartnerChatAction, useStreamers } from '@/hooks/useAdmin';
import { formatRelativeTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  tone: 'success' | 'error';
  message: string;
};

type HistoryEntry = {
  id: string;
  createdAt: string;
  login: string;
  displayName?: string;
  mode: string;
  color: string;
  messageSnippet: string;
};

const MODE_OPTIONS: Array<{ value: PartnerChatActionMode; label: string; hint: string }> = [
  { value: 'message', label: 'Message', hint: 'Normale Chat-Nachricht.' },
  { value: 'action', label: 'Action', hint: 'Sendet die Nachricht als /me-Action.' },
  { value: 'announcement', label: 'Announcement', hint: 'Nutzen, wenn der Bot Chat-Announcements senden darf.' },
];

const COLOR_SUGGESTIONS = ['purple', 'blue', 'green', 'orange', 'primary'];
const MESSAGE_LIMIT = 450;

function matchesStreamer(row: StreamerRow, query: string) {
  const normalizedQuery = query.toLowerCase();
  return [row.login, row.displayName, row.discordDisplayName]
    .filter(Boolean)
    .some((value) => String(value).toLowerCase().includes(normalizedQuery));
}

function rankStreamer(row: StreamerRow, query: string) {
  const normalizedQuery = query.toLowerCase();
  const login = row.login.toLowerCase();
  const displayName = String(row.displayName || '').toLowerCase();
  if (login === normalizedQuery) {
    return 0;
  }
  if (login.startsWith(normalizedQuery)) {
    return 1;
  }
  if (displayName.startsWith(normalizedQuery)) {
    return 2;
  }
  return 3;
}

function trimMessageSnippet(message: string, maxLength = 110) {
  if (message.length <= maxLength) {
    return message;
  }
  return `${message.slice(0, maxLength - 1).trimEnd()}…`;
}

export default function ChatActionsPage() {
  const streamersQuery = useStreamers('all');
  const partnerChatMutation = usePartnerChatAction();
  const [pickerValue, setPickerValue] = useState('');
  const [selectedLogin, setSelectedLogin] = useState('');
  const [mode, setMode] = useState<PartnerChatActionMode>('message');
  const [color, setColor] = useState('purple');
  const [message, setMessage] = useState('');
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const deferredPickerValue = useDeferredValue(pickerValue);
  const trimmedQuery = deferredPickerValue.trim().toLowerCase();
  const suggestions = useMemo(() => {
    if (!trimmedQuery) {
      return [];
    }
    return [...(streamersQuery.data ?? [])]
      .filter((row) => matchesStreamer(row, trimmedQuery))
      .sort((left, right) => {
        const rankDiff = rankStreamer(left, trimmedQuery) - rankStreamer(right, trimmedQuery);
        if (rankDiff !== 0) {
          return rankDiff;
        }
        return left.login.localeCompare(right.login, 'de');
      })
      .slice(0, 8);
  }, [streamersQuery.data, trimmedQuery]);

  const selectedStreamer = useMemo(
    () => (streamersQuery.data ?? []).find((row) => row.login === selectedLogin),
    [selectedLogin, streamersQuery.data],
  );
  const canSubmit = Boolean(selectedLogin && message.trim() && message.trim().length <= MESSAGE_LIMIT);

  const historyColumns: TableColumn<HistoryEntry>[] = [
    {
      key: 'createdAt',
      title: 'Zeit',
      sortable: true,
      sortValue: (row) => new Date(row.createdAt).getTime(),
      render: (row) => formatRelativeTime(row.createdAt),
    },
    {
      key: 'streamer',
      title: 'Streamer',
      sortable: true,
      sortValue: (row) => row.login,
      render: (row) => (
        <div>
          <div className="font-semibold text-white">{row.displayName || row.login}</div>
          <div className="text-xs uppercase tracking-[0.16em] text-text-secondary">{row.login}</div>
        </div>
      ),
    },
    {
      key: 'mode',
      title: 'Mode',
      sortable: true,
      sortValue: (row) => row.mode,
      render: (row) => <StatusBadge status={row.mode} />,
    },
    {
      key: 'color',
      title: 'Color',
      sortable: true,
      sortValue: (row) => row.color,
      render: (row) => row.color || '—',
    },
    {
      key: 'message',
      title: 'Message',
      render: (row) => <span className="text-white/90">{row.messageSnippet}</span>,
    },
  ];

  return (
    <section className="space-y-6">
      <PageHeader title="Chat Actions" description="Partner-Chat-Aktionen senden und nachverfolgen." />

      <Section title="Neue Nachricht senden" hint="Geht direkt in den Channel-Chat">
        <div className="grid gap-5 lg:grid-cols-[1.2fr_0.8fr]">
          <div className="space-y-5">
            <div className="space-y-3">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Streamer</p>
                <div className="mt-3">
                  <SearchInput
                    placeholder="Streamer suchen und auswählen …"
                    defaultValue={pickerValue}
                    onDebouncedChange={(value) => {
                      setPickerValue(value);
                      if (selectedLogin) {
                        const normalized = value.trim().toLowerCase();
                        const loginMatches = normalized === selectedLogin;
                        const displayNameMatches =
                          normalized === (selectedStreamer?.displayName || '').toLowerCase();
                        if (!loginMatches && !displayNameMatches) {
                          setSelectedLogin('');
                        }
                      }
                    }}
                  />
                </div>
              </div>

              {trimmedQuery ? (
                <div className="rounded-[1.3rem] border border-white/10 bg-slate-950/35 p-2">
                  {streamersQuery.isLoading && !streamersQuery.data ? (
                    <div className="px-3 py-3 text-sm text-text-secondary">Streamer werden geladen …</div>
                  ) : streamersQuery.isError ? (
                    <div className="px-3 py-3 text-sm text-text-secondary">Streamer-Suche ist gerade nicht verfuegbar.</div>
                  ) : suggestions.length ? (
                    <div className="space-y-1">
                      {suggestions.map((row) => (
                        <button
                          key={row.login}
                          type="button"
                          className="interactive-surface flex w-full items-center justify-between rounded-[1rem] px-3 py-3 text-left text-text-secondary hover:text-white"
                          onClick={() => {
                            setSelectedLogin(row.login);
                            setPickerValue(row.displayName || row.login);
                          }}
                        >
                          <div className="min-w-0">
                            <div className="truncate font-semibold text-white">{row.displayName || row.login}</div>
                            <div className="truncate text-xs uppercase tracking-[0.16em] text-text-secondary">{row.login}</div>
                          </div>
                          <ChevronRight className="h-4 w-4 shrink-0 text-text-secondary" />
                        </button>
                      ))}
                    </div>
                  ) : (
                    <EmptyState
                      icon={SearchX}
                      title="Keine Treffer"
                      description="Die Streamer-Suche hat keinen passenden verwalteten Channel gefunden."
                      className="!rounded-[1rem] !border-white/8 !bg-transparent !p-5"
                    />
                  )}
                </div>
              ) : null}

              <div className="flex flex-wrap items-center gap-3">
                <span className="stat-pill">Auswahl: {selectedLogin || '—'}</span>
                {selectedStreamer?.partnerStatus ? <StatusBadge status={selectedStreamer.partnerStatus} /> : <StatusBadge status="warning" />}
              </div>
            </div>

            <div className="grid gap-4 md:grid-cols-2">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Mode</p>
                <div className="mt-3 flex flex-wrap gap-2">
                  {MODE_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      className={[
                        'filter-chip',
                        mode === option.value ? '!border-primary/40 !bg-primary/15 !text-white' : 'text-text-secondary',
                      ].join(' ')}
                      onClick={() => setMode(option.value)}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
                <p className="mt-3 text-sm text-text-secondary">
                  {MODE_OPTIONS.find((option) => option.value === mode)?.hint}
                </p>
              </div>

              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Color</p>
                <div className="mt-3 space-y-3">
                  <input
                    value={color}
                    onChange={(event) => setColor(event.target.value.trim().toLowerCase())}
                    list="chat-action-colors"
                    className="admin-input"
                    placeholder="purple"
                  />
                  <datalist id="chat-action-colors">
                    {COLOR_SUGGESTIONS.map((entry) => (
                      <option key={entry} value={entry} />
                    ))}
                  </datalist>
                  <p className="text-sm text-text-secondary">Backend-sichere Defaults: {COLOR_SUGGESTIONS.join(', ')}.</p>
                </div>
              </div>
            </div>

            <div>
              <div className="flex items-center justify-between gap-3">
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Message</p>
                <span className={`text-xs font-medium ${message.length > MESSAGE_LIMIT ? 'text-red-200' : 'text-text-secondary'}`}>
                  {message.length}/{MESSAGE_LIMIT}
                </span>
              </div>
              <textarea
                value={message}
                onChange={(event) => setMessage(event.target.value.slice(0, MESSAGE_LIMIT))}
                rows={7}
                maxLength={MESSAGE_LIMIT}
                className="admin-input mt-3 min-h-[11rem] resize-y"
                placeholder="Nachricht eingeben …"
              />
            </div>

            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="text-sm text-text-secondary">
                {selectedStreamer ? `Ziel: ${selectedStreamer.displayName || selectedStreamer.login}` : 'Bitte zuerst einen Streamer auswählen.'}
              </div>
              <button
                type="button"
                className="admin-button admin-button-primary"
                disabled={!canSubmit || partnerChatMutation.isPending}
                onClick={() => setConfirmOpen(true)}
              >
                <Send className="h-4 w-4" />
                Nachricht senden
              </button>
            </div>
          </div>

          <article className="rounded-[1.5rem] border border-white/10 bg-white/[0.03] p-5">
            <div className="flex items-center gap-3">
              <Megaphone className="h-5 w-5 text-white/80" />
              <h3 className="text-base font-semibold text-white">Sendekontext</h3>
            </div>
            <div className="mt-4 space-y-3 text-sm text-text-secondary">
              <div className="flex items-center justify-between gap-3 rounded-[1rem] border border-white/10 bg-white/[0.03] px-4 py-3">
                <span>Streamer</span>
                <span className="text-white">{selectedLogin || '—'}</span>
              </div>
              <div className="flex items-center justify-between gap-3 rounded-[1rem] border border-white/10 bg-white/[0.03] px-4 py-3">
                <span>Mode</span>
                <span className="text-white">{mode}</span>
              </div>
              <div className="flex items-center justify-between gap-3 rounded-[1rem] border border-white/10 bg-white/[0.03] px-4 py-3">
                <span>Color</span>
                <span className="text-white">{color || '—'}</span>
              </div>
              <div className="flex items-center justify-between gap-3 rounded-[1rem] border border-white/10 bg-white/[0.03] px-4 py-3">
                <span>Mutation</span>
                <StatusBadge status={partnerChatMutation.isPending ? 'warning' : 'ok'} />
              </div>
            </div>
          </article>
        </div>
      </Section>

      <Section title="Letzte Aktionen" hint="Audit-Trail der gesendeten Nachrichten">
        {history.length ? (
          <DataTable columns={historyColumns} rows={history} rowKey={(row) => row.id} />
        ) : (
          <EmptyState
            icon={Megaphone}
            title="Noch keine Aktionen"
            description="In dieser Session wurden noch keine Chat-Aktionen ausgelöst."
          />
        )}
      </Section>

      <ConfirmDialog
        open={confirmOpen}
        title="Chat-Aktion senden?"
        description={
          selectedLogin
            ? `${mode} wird an ${selectedLogin} gesendet. Bitte Nachricht und Zielchannel noch einmal prüfen.`
            : 'Bitte zuerst einen Streamer auswählen.'
        }
        confirmLabel="Senden"
        busy={partnerChatMutation.isPending}
        onCancel={() => setConfirmOpen(false)}
        onConfirm={() => {
          if (!canSubmit || !selectedLogin) {
            return;
          }

          void partnerChatMutation
            .mutateAsync({
              login: selectedLogin,
              mode,
              color: color as never,
              message: message.trim(),
            })
            .then((result) => {
              if (!result.ok) {
                setToast({ open: true, tone: 'error', message: result.message });
                return;
              }

              const target = selectedStreamer;
              setHistory((previous) => [
                {
                  id: `${Date.now()}-${selectedLogin}`,
                  createdAt: new Date().toISOString(),
                  login: selectedLogin,
                  displayName: target?.displayName,
                  mode,
                  color,
                  messageSnippet: trimMessageSnippet(message.trim()),
                },
                ...previous,
              ].slice(0, 20));
              setToast({ open: true, tone: 'success', message: result.message });
              setSelectedLogin('');
              setPickerValue('');
              setMessage('');
              setConfirmOpen(false);
            })
            .catch((error: unknown) => {
              setToast({
                open: true,
                tone: 'error',
                message: error instanceof Error ? error.message : 'Chat-Aktion konnte nicht gesendet werden.',
              });
            });
        }}
      />

      <Toast open={toast.open} tone={toast.tone} message={toast.message} onClose={() => setToast((previous) => ({ ...previous, open: false }))} />
    </section>
  );
}
