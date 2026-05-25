import { useEffect, useState } from 'react';
import { RadioTower, Save, Users } from 'lucide-react';
import type { AdminConfigScope } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { KpiCard } from '@/components/shared/KpiCard';
import { Toast } from '@/components/shared/Toast';
import { useConfigOverview, useRaidConfigMutation } from '@/hooks/useAdmin';

function formatCount(value: number | undefined) {
  return typeof value === 'number' ? String(value) : '—';
}

function formatStatus(value: boolean | undefined) {
  if (value === undefined) {
    return '—';
  }
  return value ? 'Ja' : 'Nein';
}

export function RaidConfig() {
  const [scope, setScope] = useState<AdminConfigScope>('active');
  const configQuery = useConfigOverview(scope);
  const mutation = useRaidConfigMutation();
  const snapshot = configQuery.data?.raids;
  const [raidBotEnabled, setRaidBotEnabled] = useState(false);
  const [livePingEnabled, setLivePingEnabled] = useState(false);
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
    setRaidBotEnabled(Boolean(snapshot.allRaidBotEnabled));
    setLivePingEnabled(Boolean(snapshot.allLivePingEnabled));
  }, [dirty, snapshot]);

  return (
    <section className="space-y-5">
      <PageHeader
        title="Raid Config"
        description="Raid-Bot und Live-Ping gesammelt ausrollen. Der Scope `all` wendet die Änderungen bewusst auf den gesamten Bestand an."
      />

      <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <KpiCard
          title="Verwaltete Streamer"
          value={formatCount(snapshot?.totalManagedStreamers)}
          hint="Basis für Bulk-Änderungen"
          icon={Users}
        />
        <KpiCard
          title="Raid Bot Aktiv"
          value={formatCount(snapshot?.raidBotEnabledCount)}
          hint="aktuell aktivierte Streamer"
          icon={RadioTower}
          tone="primary"
        />
        <KpiCard
          title="Live Ping Aktiv"
          value={formatCount(snapshot?.livePingEnabledCount)}
          hint="aktuell aktivierte Streamer"
          icon={RadioTower}
          tone="accent"
        />
        <KpiCard
          title="Alle Gleichgezogen"
          value={`${formatStatus(snapshot?.allRaidBotEnabled)} / ${formatStatus(snapshot?.allLivePingEnabled)}`}
          hint="Raid Bot / Live Ping"
          icon={Users}
        />
      </section>

      <article className="panel-card rounded-[1.8rem] p-6">
        <div className="grid gap-5 xl:grid-cols-[1.05fr_0.95fr]">
          <div className="space-y-4">
            <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Bulk Update</p>

            <label className="flex items-center justify-between rounded-[1.2rem] border border-white/10 bg-white/[0.03] px-4 py-3">
              <div>
                <p className="text-white">Raid Bot aktivieren</p>
                <p className="mt-1 text-xs text-text-secondary">Steuert die generelle Raid-Bot-Aktivierung im gewählten Scope.</p>
              </div>
              <input
                type="checkbox"
                checked={raidBotEnabled}
                onChange={(event) => {
                  setDirty(true);
                  setRaidBotEnabled(event.target.checked);
                }}
              />
            </label>

            <label className="flex items-center justify-between rounded-[1.2rem] border border-white/10 bg-white/[0.03] px-4 py-3">
              <div>
                <p className="text-white">Live Ping aktivieren</p>
                <p className="mt-1 text-xs text-text-secondary">Schaltet die Live-Ping-Auslösung für den gewählten Scope gesammelt um.</p>
              </div>
              <input
                type="checkbox"
                checked={livePingEnabled}
                onChange={(event) => {
                  setDirty(true);
                  setLivePingEnabled(event.target.checked);
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
                <span className="font-semibold text-white">All:</span> Rollt die beiden Flags bewusst auf den gesamten verwalteten Bestand aus.
              </p>
            </div>
            <button
              className="admin-button admin-button-primary"
              disabled={mutation.isPending}
              onClick={async () => {
                try {
                  await mutation.mutateAsync({
                    raid_bot_enabled: raidBotEnabled,
                    live_ping_enabled: livePingEnabled,
                    scope,
                  });
                  setDirty(false);
                  setToast({ open: true, tone: 'success', message: 'Raid-Konfiguration gespeichert.' });
                } catch (error) {
                  setToast({
                    open: true,
                    tone: 'error',
                    message: error instanceof Error ? error.message : 'Raid-Konfiguration konnte nicht gespeichert werden.',
                  });
                }
              }}
            >
              <Save className="h-4 w-4" />
              Raid-Konfiguration speichern
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
