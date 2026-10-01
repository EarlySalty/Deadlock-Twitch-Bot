import { AlertTriangle, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { useState } from 'react';
import { ApiError } from '@/api/client';
import type { PartnerSignupBlockEntry, PartnerSignupTagBlockEntry } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { Toast } from '@/components/shared/Toast';
import {
  useAddPartnerSignupBlock,
  useAddPartnerSignupTagBlock,
  usePartnerSignupBlocks,
  usePartnerSignupTagBlocks,
  useRemovePartnerSignupBlock,
  useRemovePartnerSignupTagBlock,
} from '@/hooks/useAdmin';
import { formatDateTime } from '@/utils/formatters';

type ToastState = { open: boolean; tone: 'success' | 'error'; message: string };

/** Was ein Ausschluss zusätzlich zum Listeneintrag auslöst. */
const ADD_STEPS = [
  'Der Kanal wird als Raid-Ziel gesperrt.',
  'Gespeicherte Raid-Zugänge des Kanals werden gelöscht.',
  'Ein noch aktiver Partner wird stillgelegt.',
];

const TAG_ADD_STEPS = [
  'Der Kanal kommt auf die Sperrliste der Partneraufnahme.',
  'Scout spricht ihn nicht mehr an.',
  'Der Kanal wird als Raid-Ziel gesperrt.',
  'Bestehende Partner sind davon nicht betroffen.',
];

/**
 * Fehlertext für einen fehlgeschlagenen Schreibvorgang.
 *
 * Bei 401/403 trägt `ApiError.message` die Login-URL statt einer Meldung
 * (siehe `client.ts`): die würde als Fehlertext im Toast landen. Deshalb dort
 * ein Hinweis auf die abgelaufene Sitzung statt der durchgereichten Message.
 */
function fehlerText(error: unknown, vorgang: string): string {
  if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
    return `Die Sitzung ist abgelaufen. Bitte neu anmelden und den ${vorgang} erneut versuchen.`;
  }
  if (error instanceof Error && error.message) {
    return `${vorgang} nicht gespeichert: ${error.message}`;
  }
  return `${vorgang} konnte nicht gespeichert werden.`;
}

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
  const tagQuery = usePartnerSignupTagBlocks();
  const tagAddMutation = useAddPartnerSignupTagBlock();
  const tagRemoveMutation = useRemovePartnerSignupTagBlock();
  const [login, setLogin] = useState('');
  const [reason, setReason] = useState('');
  const [publicMessage, setPublicMessage] = useState('');
  const [tag, setTag] = useState('');
  const [tagReason, setTagReason] = useState('');
  const [tagPublicMessage, setTagPublicMessage] = useState('');
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  async function submitAdd() {
    const candidate = login.trim().toLowerCase();
    if (!candidate) {
      setToast({ open: true, tone: 'error', message: 'Bitte einen Login eingeben.' });
      return;
    }
    if (!reason.trim()) {
      setToast({ open: true, tone: 'error', message: 'Bitte einen internen Grund angeben.' });
      return;
    }
    try {
      const result = await addMutation.mutateAsync({
        login: candidate,
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
        message: fehlerText(error, 'Ausschluss'),
      });
    }
  }

  async function submitRemove(entry: PartnerSignupBlockEntry) {
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
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message:
          error instanceof ApiError && (error.status === 401 || error.status === 403)
            ? 'Die Sitzung ist abgelaufen. Bitte neu anmelden und die Aufhebung erneut versuchen.'
            : 'Ausschluss konnte nicht aufgehoben werden.',
      });
    }
  }

  async function submitTagAdd() {
    const candidate = tag.trim();
    if (!candidate) {
      setToast({ open: true, tone: 'error', message: 'Bitte einen Tag eingeben.' });
      return;
    }
    try {
      const result = await tagAddMutation.mutateAsync({
        tag: candidate,
        reason: tagReason.trim() || undefined,
        publicMessage: tagPublicMessage.trim() || undefined,
      });
      setTag('');
      setTagReason('');
      setTagPublicMessage('');
      setToast({
        open: true,
        tone: 'success',
        message: `${result.display_tag} ist gesperrt. Kanäle mit diesem Tag sind von der Partneraufnahme ausgeschlossen.`,
      });
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message: fehlerText(error, 'Tag-Sperre'),
      });
    }
  }

  async function submitTagRemove(entry: PartnerSignupTagBlockEntry) {
    try {
      const result = await tagRemoveMutation.mutateAsync(entry.tag);
      setToast({
        open: true,
        tone: result.removed ? 'success' : 'error',
        message: result.removed
          ? `${entry.display_tag} wird nicht mehr automatisch ausgeschlossen. Bestehende Kanal-Ausschlüsse bleiben bestehen.`
          : `Für ${entry.display_tag} gab es keine Sperre mehr.`,
      });
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message:
          error instanceof ApiError && (error.status === 401 || error.status === 403)
            ? 'Die Sitzung ist abgelaufen. Bitte neu anmelden und die Aufhebung erneut versuchen.'
            : 'Tag-Sperre konnte nicht aufgehoben werden.',
      });
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
          onClick={() => void submitRemove(entry)}
          type="button"
        >
          <Trash2 className="h-4 w-4" />
          Aufheben
        </button>
      ),
    },
  ];

  const tagColumns: TableColumn<PartnerSignupTagBlockEntry>[] = [
    {
      key: 'tag',
      title: 'Tag',
      sortable: true,
      sortValue: (entry) => entry.display_tag,
      render: (entry) => (
        <div>
          <div className="font-semibold">{entry.display_tag}</div>
          <div className="text-xs text-white/50">{entry.tag}</div>
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
          aria-label="Tag-Sperre aufheben"
          className="admin-button admin-button-secondary"
          disabled={tagRemoveMutation.isPending}
          onClick={() => void submitTagRemove(entry)}
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
            onClick={() => void submitAdd()}
            type="button"
          >
            <Plus className="h-4 w-4" />
            Ausschließen
          </button>
        </div>

        <div className="mt-4 flex gap-3 rounded-2xl border border-primary/35 bg-primary/10 p-4 text-sm text-white/80">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
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
        title="Tags automatisch ausschließen"
        hint="Die grauen Tags am Twitch-Stream, zum Beispiel Deutsch oder English. Wer so einen Tag live oder in einer gespeicherten Session hat, wird automatisch von der Partneraufnahme ausgeschlossen. Bestehende Partner bleiben unangetastet."
      >
        <div className="grid gap-4 md:grid-cols-[0.9fr_1.2fr_1.4fr_auto] md:items-end">
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Tag</span>
            <input
              className="admin-input"
              placeholder="z. B. Deutsch"
              value={tag}
              onChange={(event) => setTag(event.target.value)}
            />
          </label>
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Interner Grund</span>
            <input
              className="admin-input"
              value={tagReason}
              onChange={(event) => setTagReason(event.target.value)}
            />
          </label>
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Absagetext (optional)</span>
            <input
              className="admin-input"
              value={tagPublicMessage}
              onChange={(event) => setTagPublicMessage(event.target.value)}
            />
          </label>
          <button
            className="admin-button admin-button-primary"
            disabled={tagAddMutation.isPending}
            onClick={() => void submitTagAdd()}
            type="button"
          >
            <Plus className="h-4 w-4" />
            Tag sperren
          </button>
        </div>

        <div className="mt-4 flex gap-3 rounded-2xl border border-primary/35 bg-primary/10 p-4 text-sm text-white/80">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-primary" />
          <div>
            <div className="font-medium text-white">Das passiert bei einem Treffer</div>
            <ul className="mt-1 list-disc space-y-1 pl-5">
              {TAG_ADD_STEPS.map((step) => (
                <li key={step}>{step}</li>
              ))}
            </ul>
          </div>
        </div>
      </Section>

      <Section
        title="Gesperrte Tags"
        hint="Ein Treffer schließt den Kanal dauerhaft aus. Das Aufheben der Regel hebt bestehende Kanal-Ausschlüsse nicht auf."
      >
        {tagQuery.isError ? (
          <div className="text-sm text-white/70">Gesperrte Tags konnten nicht geladen werden.</div>
        ) : (
          <DataTable
            columns={tagColumns}
            rows={tagQuery.data?.items ?? []}
            rowKey={(entry) => entry.tag}
            emptyLabel="Kein Tag ist gesperrt."
          />
        )}
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

      <Toast
        open={toast.open}
        tone={toast.tone}
        message={toast.message}
        onClose={() => setToast((current) => ({ ...current, open: false }))}
      />
    </section>
  );
}
