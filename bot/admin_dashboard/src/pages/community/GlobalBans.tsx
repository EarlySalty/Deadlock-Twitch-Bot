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
      setToast({ open: true, tone: 'error', message: 'PLATZHALTER: <Hinweis fehlender Login>' });
      return;
    }
    try {
      await addMutation.mutateAsync({ login: login.trim(), reason: reason.trim() || undefined });
      setLogin('');
      setReason('');
      setToast({ open: true, tone: 'success', message: 'PLATZHALTER: <Erfolg globaler Ban gespeichert>' });
    } catch {
      setToast({
        open: true,
        tone: 'error',
        message: 'PLATZHALTER: <Fehler globaler Ban speichern>',
      });
    }
  }

  async function removeEntry(entryLogin: string) {
    try {
      await removeMutation.mutateAsync(entryLogin);
      setToast({ open: true, tone: 'success', message: 'PLATZHALTER: <Erfolg globaler Ban entfernt>' });
    } catch {
      setToast({
        open: true,
        tone: 'error',
        message: 'PLATZHALTER: <Fehler globaler Ban entfernen>',
      });
    }
  }

  async function setChannel(channel: GlobalBanChannel) {
    try {
      await channelMutation.mutateAsync({
        login: channel.twitch_login,
        enabled: !channel.global_ban_enforcement_enabled,
      });
      setToast({ open: true, tone: 'success', message: 'PLATZHALTER: <Erfolg Kanal-Enforcement geändert>' });
    } catch {
      setToast({
        open: true,
        tone: 'error',
        message: 'PLATZHALTER: <Fehler Kanal-Enforcement ändern>',
      });
    }
  }

  const entryColumns: TableColumn<GlobalBanEntry>[] = [
    {
      key: 'login',
      title: 'PLATZHALTER: <Spalte Login>',
      sortable: true,
      sortValue: (entry) => entry.chatter_login,
      render: (entry) => <span className="font-semibold">{entry.chatter_login}</span>,
    },
    {
      key: 'reason',
      title: 'PLATZHALTER: <Spalte Grund>',
      render: (entry) => entry.reason || '—',
    },
    {
      key: 'added',
      title: 'PLATZHALTER: <Spalte Hinzugefügt>',
      render: (entry) => (entry.added_at ? formatDateTime(entry.added_at) : '—'),
    },
    {
      key: 'actions',
      title: 'PLATZHALTER: <Spalte Aktionen>',
      render: (entry) => (
        <button
          aria-label="PLATZHALTER: <Globalen Ban entfernen>"
          className="admin-button admin-button-secondary"
          disabled={removeMutation.isPending}
          onClick={() => void removeEntry(entry.chatter_login)}
          type="button"
        >
          <Trash2 className="h-4 w-4" />
          PLATZHALTER: &lt;Entfernen&gt;
        </button>
      ),
    },
  ];

  const channelColumns: TableColumn<GlobalBanChannel>[] = [
    {
      key: 'channel',
      title: 'PLATZHALTER: <Spalte Kanal>',
      sortable: true,
      sortValue: (channel) => channel.twitch_login,
      render: (channel) => <span className="font-semibold">{channel.twitch_login}</span>,
    },
    {
      key: 'enabled',
      title: 'PLATZHALTER: <Spalte Enforcement-Status>',
      render: (channel) => (
        <button
          aria-checked={channel.global_ban_enforcement_enabled}
          aria-label="PLATZHALTER: <Global-Ban-Enforcement umschalten>"
          className={[
            'admin-button',
            channel.global_ban_enforcement_enabled ? 'admin-button-primary' : 'admin-button-secondary',
          ].join(' ')}
          disabled={channelMutation.isPending}
          onClick={() => void setChannel(channel)}
          role="switch"
          type="button"
        >
          {channel.global_ban_enforcement_enabled
            ? 'PLATZHALTER: <Enforcement aktiv>'
            : 'PLATZHALTER: <Enforcement deaktiviert>'}
        </button>
      ),
    },
  ];

  if (query.isLoading) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">PLATZHALTER: &lt;Globale Bans werden geladen&gt;</div>;
  }

  if (query.isError || !query.data) {
    return (
      <div className="panel-card rounded-[1.8rem] p-8 text-white">
        PLATZHALTER: &lt;Globale Bans konnten nicht geladen werden&gt;
      </div>
    );
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="PLATZHALTER: <Seitentitel globale Ban-Verwaltung>"
        description="PLATZHALTER: <Beschreibung globale Ban-Liste und kanalbezogenes Opt-out>"
        primaryAction={
          <button className="admin-button admin-button-secondary" onClick={() => void query.refetch()} type="button">
            <RefreshCw className={`h-4 w-4 ${query.isFetching ? 'animate-spin' : ''}`} />
            PLATZHALTER: &lt;Aktualisieren&gt;
          </button>
        }
      />

      <Section title="PLATZHALTER: <Globalen Ban hinzufügen>" hint="PLATZHALTER: <Hilfetext Ban hinzufügen>">
        <div className="grid gap-4 md:grid-cols-[1fr_1.4fr_auto] md:items-end">
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">PLATZHALTER: &lt;Login&gt;</span>
            <input className="admin-input" value={login} onChange={(event) => setLogin(event.target.value)} />
          </label>
          <label className="space-y-2">
            <span className="text-sm font-medium text-white">PLATZHALTER: &lt;Grund&gt;</span>
            <input className="admin-input" value={reason} onChange={(event) => setReason(event.target.value)} />
          </label>
          <button className="admin-button admin-button-primary" disabled={addMutation.isPending} onClick={() => void addEntry()} type="button">
            <Plus className="h-4 w-4" />
            PLATZHALTER: &lt;Hinzufügen&gt;
          </button>
        </div>
      </Section>

      <Section title="PLATZHALTER: <Globale Ban-Liste>" hint="PLATZHALTER: <Hilfetext globale Ban-Liste>">
        <DataTable
          columns={entryColumns}
          rows={query.data.entries}
          rowKey={(entry) => entry.chatter_login}
          emptyLabel="PLATZHALTER: <Leere globale Ban-Liste>"
        />
      </Section>

      <Section title="PLATZHALTER: <Kanal-Enforcement>" hint="PLATZHALTER: <Hilfetext Kanal-Opt-out und Standard aktiv>">
        <DataTable
          columns={channelColumns}
          rows={query.data.channels}
          rowKey={(channel) => channel.twitch_login}
          emptyLabel="PLATZHALTER: <Keine Kanäle vorhanden>"
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
