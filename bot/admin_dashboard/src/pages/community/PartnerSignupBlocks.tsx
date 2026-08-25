import { AlertTriangle, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { useState } from 'react';
import type { PartnerSignupBlockEntry } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { ConfirmTypedDialog } from '@/components/shared/ConfirmTypedDialog';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { Toast } from '@/components/shared/Toast';
import {
  useAddPartnerSignupBlock,
  usePartnerSignupBlocks,
  useRemovePartnerSignupBlock,
} from '@/hooks/useAdmin';
import { formatDateTime } from '@/utils/formatters';

type ToastState = { open: boolean; tone: 'success' | 'error'; message: string };

/** Was ein Ausschluss zusätzlich zum Listeneintrag auslöst. */
const ADD_STEPS = [
  'Der Kanal wird als Raid-Ziel gesperrt.',
  'Gespeicherte Raid-Zugänge des Kanals werden gelöscht.',
  'Ein noch aktiver Partner wird stillgelegt.',
];

const REMOVE_STEPS = [
  'Der Kanal kann wieder ins Partnerprogramm aufgenommen werden.',
  'Die Raid-Sperre aus diesem Ausschluss wird aufgehoben, andere Sperrgründe bleiben.',
  'Gelöschte Zugänge kommen nicht zurück, der Kanal muss neu autorisieren.',
];

/**
 * Partneraufnahme-Sperrliste (`twitch_partner_signup_denylist`). Bewusst eine
 * eigene Seite statt eines Filters in der Streamer-Liste: hier stehen auch
 * Kanäle, die noch nie eine Partnerzeile hatten. Nicht zu verwechseln mit der
 * Ausschlussliste des Audio-Archivs.
 */
export default function PartnerSignupBlocksPage() {
  const query = usePartnerSignupBlocks();
  const addMutation = useAddPartnerSignupBlock();
  const removeMutation = useRemovePartnerSignupBlock();
  const [login, setLogin] = useState('');
  const [reason, setReason] = useState('');
  const [publicMessage, setPublicMessage] = useState('');
  const [pendingAdd, setPendingAdd] = useState<string | null>(null);
  const [pendingRemove, setPendingRemove] = useState<PartnerSignupBlockEntry | null>(null);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  function openAddConfirm() {
    const candidate = login.trim().toLowerCase();
    if (!candidate) {
      setToast({ open: true, tone: 'error', message: 'Bitte einen Login eingeben.' });
      return;
    }
    if (!reason.trim()) {
      setToast({ open: true, tone: 'error', message: 'Bitte einen internen Grund angeben.' });
      return;
    }
    setPendingAdd(candidate);
  }

  async function confirmAdd() {
    if (!pendingAdd) {
      return;
    }
    try {
      const result = await addMutation.mutateAsync({
        login: pendingAdd,
        reason: reason.trim(),
        publicMessage: publicMessage.trim() || undefined,
      });
      const effects = [
        result.raid_blacklisted ? 'Raid gesperrt' : null,
        result.credentials_deleted ? 'Zugänge gelöscht' : null,
        result.active_partner_paused ? 'Partner stillgelegt' : null,
      ].filter(Boolean);
      setLogin('');
      setReason('');
      setPublicMessage('');
      setToast({
        open: true,
        tone: 'success',
        message: `${result.login} ist von der Partneraufnahme ausgeschlossen${
          effects.length ? ` (${effects.join(', ')})` : ''
        }.`,
      });
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message:
          error instanceof Error && error.message
            ? `Ausschluss nicht gespeichert: ${error.message}`
            : 'Ausschluss konnte nicht gespeichert werden.',
      });
    } finally {
      setPendingAdd(null);
    }
  }

  async function confirmRemove() {
    if (!pendingRemove) {
      return;
    }
    const entry = pendingRemove;
    try {
      const result = await removeMutation.mutateAsync({
        login: entry.login,
        twitchUserId: entry.twitch_user_id,
      });
      setToast({
        open: true,
        tone: result.removed ? 'success' : 'error',
        message: result.removed
          ? `${entry.login} ist wieder für die Partneraufnahme zugelassen.`
          : `Für ${entry.login} gab es keinen Eintrag mehr.`,
      });
    } catch {
      setToast({
        open: true,
        tone: 'error',
        message: 'Ausschluss konnte nicht aufgehoben werden.',
      });
    } finally {
      setPendingRemove(null);
    }
  }

  const columns: TableColumn<PartnerSignupBlockEntry>[] = [
    {
      key: 'login',
      title: 'Kanal',
      sortable: true,
      sortValue: (entry) => entry.login,
      render: (entry) => (
        <div>
          <div className="font-semibold">{entry.login}</div>
          <div className="text-xs text-white/50">ID {entry.twitch_user_id}</div>
        </div>
      ),
    },
    {
      key: 'reason',
      title: 'Interner Grund',
      render: (entry) => entry.reason || '—',
    },
    {
      key: 'public_message',
      title: 'Absagetext',
      render: (entry) => entry.public_message || 'Standardtext',
    },
    {
      key: 'added_by',
      title: 'Eingetragen von',
      render: (entry) => entry.added_by || '—',
    },
    {
      key: 'added_at',
      title: 'Eingetragen am',
      sortable: true,
      sortValue: (entry) => entry.added_at,
      render: (entry) => (entry.added_at ? formatDateTime(entry.added_at) : '—'),
    },
    {
      key: 'actions',
      title: 'Aktionen',
      render: (entry) => (
        <button
          aria-label="Ausschluss aufheben"
          className="admin-button admin-button-secondary"
          disabled={removeMutation.isPending}
          onClick={() => setPendingRemove(entry)}
          type="button"
        >
          <Trash2 className="h-4 w-4" />
          Aufheben
        </button>
      ),
    },
  ];

  if (query.isLoading) {
    return (
      <div className="panel-card rounded-[1.8rem] p-8 text-white">
        Ausschlussliste wird geladen …
      </div>
    );
  }

  if (query.isError || !query.data) {
    return (
      <div className="panel-card rounded-[1.8rem] p-8 text-white">
        Ausschlussliste konnte nicht geladen werden.
      </div>
    );
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="Partneraufnahme"
        description="Kanäle, die nicht ins Partnerprogramm aufgenommen werden. Getrennt von globalen Bans und von der Ausschlussliste des Audio-Archivs."
        primaryAction={
          <button
            className="admin-button admin-button-secondary"
            onClick={() => void query.refetch()}
            type="button"
          >
            <RefreshCw className={`h-4 w-4 ${query.isFetching ? 'animate-spin' : ''}`} />
            Aktualisieren
          </button>
        }
      />

      <Section
        title="Kanal ausschließen"
        hint="Der interne Grund bleibt intern. Der Absagetext ist optional und ersetzt den Standardtext gegenüber dem Kanal."
      >
        <div className="grid gap-4 md:grid-cols-[1fr_1.2fr_1.4fr_auto] md:items-end">
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Login</span>
            <input
              className="admin-input"
              value={login}
              onChange={(event) => setLogin(event.target.value)}
            />
          </label>
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Interner Grund</span>
            <input
              className="admin-input"
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
          </label>
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Absagetext (optional)</span>
            <input
              className="admin-input"
              value={publicMessage}
              onChange={(event) => setPublicMessage(event.target.value)}
            />
          </label>
          <button
            className="admin-button admin-button-primary"
            disabled={addMutation.isPending}
            onClick={openAddConfirm}
            type="button"
          >
            <Plus className="h-4 w-4" />
            Ausschließen
          </button>
        </div>

        <div className="mt-4 flex gap-3 rounded-2xl border border-amber-400/30 bg-amber-400/10 p-4 text-sm text-white/80">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-300" />
          <div>
            <div className="font-medium text-white">Das passiert zusätzlich</div>
            <ul className="mt-1 list-disc space-y-1 pl-5">
              {ADD_STEPS.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ul>
          </div>
        </div>
      </Section>

      <Section
        title="Von Partneraufnahme ausgeschlossen"
        hint="Die stabile Twitch-ID hält den Ausschluss auch nach einer Umbenennung."
      >
        <DataTable
          columns={columns}
          rows={query.data.items}
          rowKey={(entry) => entry.twitch_user_id}
          emptyLabel="Kein Kanal ist ausgeschlossen."
        />
      </Section>

      <ConfirmTypedDialog
        open={Boolean(pendingAdd)}
        title="Kanal von der Partneraufnahme ausschließen"
        description={`${pendingAdd ?? ''} wird ausgeschlossen. Zum Bestätigen den Login eintippen.`}
        expected={pendingAdd ?? ''}
        steps={ADD_STEPS}
        confirmLabel="Ausschließen"
        busy={addMutation.isPending}
        onConfirm={() => void confirmAdd()}
        onCancel={() => setPendingAdd(null)}
      />

      <ConfirmTypedDialog
        open={Boolean(pendingRemove)}
        title="Ausschluss aufheben"
        description={`${pendingRemove?.login ?? ''} darf danach wieder aufgenommen werden. Zum Bestätigen den Login eintippen.`}
        expected={pendingRemove?.login ?? ''}
        steps={REMOVE_STEPS}
        confirmLabel="Aufheben"
        busy={removeMutation.isPending}
        onConfirm={() => void confirmRemove()}
        onCancel={() => setPendingRemove(null)}
      />

      <Toast
        open={toast.open}
        tone={toast.tone}
        message={toast.message}
        onClose={() => setToast((current) => ({ ...current, open: false }))}
      />
    </section>
  );
}
