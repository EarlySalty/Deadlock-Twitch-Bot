import { Plus, RefreshCw, Trash2 } from 'lucide-react';
import { useState } from 'react';
import type { GlobalBanChannel, GlobalBanEntry } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { DataTable, type TableColumn } from '@/components/shared/DataTable';
import { Toast } from '@/components/shared/Toast';
import {
  useAddGlobalBan,
  useGlobalBans,
  useRemoveGlobalBan,
  useSetGlobalBanChannelEnforcement,
} from '@/hooks/useAdmin';
import { formatDateTime } from '@/utils/formatters';

type ToastState = { open: boolean; tone: 'success' | 'error'; message: string };

export default function GlobalBansPage() {
  const query = useGlobalBans();
  const addMutation = useAddGlobalBan();
  const removeMutation = useRemoveGlobalBan();
  const channelMutation = useSetGlobalBanChannelEnforcement();
  const [login, setLogin] = useState('');
  const [reason, setReason] = useState('');
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  async function addEntry() {
    if (!login.trim()) {
      setToast({ open: true, tone: 'error', message: 'Bitte einen Login eingeben.' });
      return;
    }
    try {
      await addMutation.mutateAsync({ login: login.trim(), reason: reason.trim() || undefined });
      setLogin('');
      setReason('');
      setToast({ open: true, tone: 'success', message: 'Globaler Ban gespeichert.' });
    } catch {
      setToast({
        open: true,
        tone: 'error',
        message: 'Globaler Ban konnte nicht gespeichert werden.',
      });
    }
  }

  async function removeEntry(entryLogin: string) {
    try {
      await removeMutation.mutateAsync(entryLogin);
      setToast({ open: true, tone: 'success', message: 'Globaler Ban entfernt.' });
    } catch {
      setToast({
        open: true,
        tone: 'error',
        message: 'Globaler Ban konnte nicht entfernt werden.',
      });
    }
  }

  async function setChannel(channel: GlobalBanChannel) {
    try {
      await channelMutation.mutateAsync({
        login: channel.twitch_login,
        enabled: !channel.global_ban_enforcement_enabled,
      });
      setToast({ open: true, tone: 'success', message: 'Kanal-Einstellung geändert.' });
    } catch {
      setToast({
        open: true,
        tone: 'error',
        message: 'Kanal-Einstellung konnte nicht geändert werden.',
      });
    }
  }

  const entryColumns: TableColumn<GlobalBanEntry>[] = [
    {
      key: 'login',
      title: 'Login',
      sortable: true,
      sortValue: (entry) => entry.chatter_login,
      render: (entry) => <span className="font-semibold">{entry.chatter_login}</span>,
    },
    {
      key: 'reason',
      title: 'Grund',
      render: (entry) => entry.reason || '—',
    },
    {
      key: 'added',
      title: 'Hinzugefügt',
      render: (entry) => (entry.added_at ? formatDateTime(entry.added_at) : '—'),
    },
    {
      key: 'actions',
      title: 'Aktionen',
      render: (entry) => (
        <button
          aria-label="Globalen Ban entfernen"
          className="admin-button admin-button-secondary"
          disabled={removeMutation.isPending}
          onClick={() => void removeEntry(entry.chatter_login)}
          type="button"
        >
          <Trash2 className="h-4 w-4" />
          Entfernen
        </button>
      ),
    },
  ];

  const channelColumns: TableColumn<GlobalBanChannel>[] = [
    {
      key: 'channel',
      title: 'Kanal',
      sortable: true,
      sortValue: (channel) => channel.twitch_login,
      render: (channel) => <span className="font-semibold">{channel.twitch_login}</span>,
    },
    {
      key: 'enabled',
      title: 'Globale Bans',
      render: (channel) => (
        <button
          aria-checked={channel.global_ban_enforcement_enabled}
          aria-label="Globale Bans für diesen Kanal umschalten"
          className={[
            'admin-button',
            channel.global_ban_enforcement_enabled ? 'admin-button-primary' : 'admin-button-secondary',
          ].join(' ')}
          disabled={channelMutation.isPending}
          onClick={() => void setChannel(channel)}
          role="switch"
          type="button"
        >
          {channel.global_ban_enforcement_enabled ? 'Aktiv' : 'Aus'}
        </button>
      ),
    },
  ];

  if (query.isLoading) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Globale Bans werden geladen …</div>;
  }

  if (query.isError || !query.data) {
    return (
      <div className="panel-card rounded-[1.8rem] p-8 text-white">
        Globale Bans konnten nicht geladen werden.
      </div>
    );
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="Globale Ban-Verwaltung"
        description="Server-weit gesperrte Accounts pflegen und pro Kanal steuern, ob die Sperren dort greifen."
        primaryAction={
          <button className="admin-button admin-button-secondary" onClick={() => void query.refetch()} type="button">
            <RefreshCw className={`h-4 w-4 ${query.isFetching ? 'animate-spin' : ''}`} />
            Aktualisieren
          </button>
        }
      />

      <Section title="Globalen Ban hinzufügen" hint="Der Account wird in allen betreuten Kanälen gesperrt, außer der Kanal hat die globalen Bans abgeschaltet.">
        <div className="grid gap-4 md:grid-cols-[1fr_1.4fr_auto] md:items-end">
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Login</span>
            <input className="admin-input" value={login} onChange={(event) => setLogin(event.target.value)} />
          </label>
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">Grund</span>
            <input className="admin-input" value={reason} onChange={(event) => setReason(event.target.value)} />
          </label>
          <button className="admin-button admin-button-primary" disabled={addMutation.isPending} onClick={() => void addEntry()} type="button">
            <Plus className="h-4 w-4" />
            Hinzufügen
          </button>
        </div>
      </Section>

      <Section title="Globale Ban-Liste" hint="Diese Accounts sind server-weit gesperrt.">
        <DataTable
          columns={entryColumns}
          rows={query.data.entries}
          rowKey={(entry) => entry.chatter_login}
          emptyLabel="Noch keine globalen Bans."
        />
      </Section>

      <Section title="Kanal-Steuerung" hint="Standardmäßig gelten die globalen Bans in jedem Kanal. Hier lässt sich das pro Kanal abschalten.">
        <DataTable
          columns={channelColumns}
          rows={query.data.channels}
          rowKey={(channel) => channel.twitch_login}
          emptyLabel="Keine Kanäle vorhanden."
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
