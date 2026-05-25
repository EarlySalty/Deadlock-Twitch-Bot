import { useEffect, useState } from 'react';
import { MessageSquareWarning, Save, ShieldAlert, Users } from 'lucide-react';
import type { AdminConfigScope } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { KpiCard } from '@/components/shared/KpiCard';
import { Toast } from '@/components/shared/Toast';
import { useChatConfigMutation, useConfigOverview } from '@/hooks/useAdmin';

function formatCount(value: number | undefined) {
  return typeof value === 'number' ? String(value) : '—';
}

function formatStatus(value: boolean | undefined) {
  if (value === undefined) {
    return '—';
  }
  return value ? 'Ja' : 'Nein';
}

export function ChatConfig() {
  const [scope, setScope] = useState<AdminConfigScope>('active');
  const configQuery = useConfigOverview(scope);
  const mutation = useChatConfigMutation();
  const snapshot = configQuery.data?.chat;
  const [silentBan, setSilentBan] = useState(false);
  const [silentRaid, setSilentRaid] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [toast, setToast] = useState<{ open: boolean; tone: 'success' | 'error'; message: string }>({
    open: false,
    tone: 'success',
    message: '',
  });

  useEffect(() => {
    if (!snapshot || dirty) {
      return;
    }
    setSilentBan(Boolean(snapshot.allSilentBan));
    setSilentRaid(Boolean(snapshot.allSilentRaid));
  }, [dirty, snapshot]);

  return (
    <section className="space-y-5">
      <PageHeader
        title="Chat Config"
        description="Moderations-Flags gesammelt steuern. Der Scope `all` erweitert den Rollout bewusst auf den gesamten verwalteten Bestand."
      />

      <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <KpiCard
          title="Verwaltete Streamer"
          value={formatCount(snapshot?.totalManagedStreamers)}
          hint="Bestand im Config-Overview"
          icon={Users}
        />
        <KpiCard
          title="Silent Ban Aktiv"
          value={formatCount(snapshot?.silentBanCount)}
          hint="aktuell markierte Streamer"
          icon={ShieldAlert}
          tone="primary"
        />
        <KpiCard
          title="Silent Raid Aktiv"
          value={formatCount(snapshot?.silentRaidCount)}
          hint="aktuell markierte Streamer"
          icon={MessageSquareWarning}
          tone="accent"
        />
        <KpiCard
          title="Alle Gleichgezogen"
          value={`${formatStatus(snapshot?.allSilentBan)} / ${formatStatus(snapshot?.allSilentRaid)}`}
          hint="Silent Ban / Silent Raid"
          icon={Users}
        />
      </section>

      <article className="panel-card rounded-[1.8rem] p-6">
        <div className="grid gap-5 xl:grid-cols-[1.05fr_0.95fr]">
          <div className="space-y-4">
            <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Bulk Update</p>

            <label className="flex items-center justify-between rounded-[1.2rem] border border-white/10 bg-white/[0.03] px-4 py-3">
              <div>
                <p className="text-white">Silent Ban setzen</p>
                <p className="mt-1 text-xs text-text-secondary">Aktualisiert das Silent-Ban-Flag gesammelt im gewählten Scope.</p>
              </div>
              <input
                type="checkbox"
                checked={silentBan}
                onChange={(event) => {
                  setDirty(true);
                  setSilentBan(event.target.checked);
                }}
              />
            </label>

            <label className="flex items-center justify-between rounded-[1.2rem] border border-white/10 bg-white/[0.03] px-4 py-3">
              <div>
                <p className="text-white">Silent Raid setzen</p>
                <p className="mt-1 text-xs text-text-secondary">Aktualisiert das Silent-Raid-Flag gesammelt im gewählten Scope.</p>
              </div>
              <input
                type="checkbox"
                checked={silentRaid}
                onChange={(event) => {
                  setDirty(true);
                  setSilentRaid(event.target.checked);
                }}
              />
            </label>
          </div>

          <div className="space-y-4">
            <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Scope</p>
            <select
              value={scope}
              onChange={(event) => {
                setDirty(false);
                setScope(event.target.value === 'all' ? 'all' : 'active');
              }}
              className="admin-input"
            >
              <option value="active">Active Streamer</option>
              <option value="all">All Managed Streamers</option>
            </select>
            <div className="rounded-[1.2rem] border border-white/10 bg-white/[0.03] p-4 text-sm leading-7 text-text-secondary">
              <p>
                <span className="font-semibold text-white">Standard:</span> <code>active</code> wirkt nur auf aktive Streamer.
              </p>
              <p>
                <span className="font-semibold text-white">All:</span> Wendet beide Moderations-Flags gesammelt auf alle verwalteten Streamer an.
              </p>
            </div>
            <button
              className="admin-button admin-button-primary"
              disabled={mutation.isPending}
              onClick={async () => {
                try {
                  await mutation.mutateAsync({
                    silent_ban: silentBan,
                    silent_raid: silentRaid,
                    scope,
                  });
                  setDirty(false);
                  setToast({ open: true, tone: 'success', message: 'Chat-Konfiguration gespeichert.' });
                } catch (error) {
                  setToast({
                    open: true,
                    tone: 'error',
                    message: error instanceof Error ? error.message : 'Chat-Konfiguration konnte nicht gespeichert werden.',
                  });
                }
              }}
            >
              <Save className="h-4 w-4" />
              Chat-Konfiguration speichern
            </button>
          </div>
        </div>
      </article>

      <Toast
        open={toast.open}
        tone={toast.tone}
        message={toast.message}
        onClose={() => setToast((current) => ({ ...current, open: false }))}
      />
    </section>
  );
}
