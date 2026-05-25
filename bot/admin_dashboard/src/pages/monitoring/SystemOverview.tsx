import { ShieldAlert } from 'lucide-react';
import { motion } from 'framer-motion';
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { PageHeader } from '@/components/layout/PageHeader';
import { EmptyState } from '@/components/shared/EmptyState';
import { KpiCard } from '@/components/shared/KpiCard';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { useEventSubStatus, useSystemHealth } from '@/hooks/useAdmin';
import { formatBytes, formatDuration, formatRelativeTime } from '@/utils/formatters';

function formatFingerprint(value?: string) {
  const normalized = String(value || '').trim();
  if (!normalized) {
    return '—';
  }
  return normalized.length > 18 ? `${normalized.slice(0, 18)}…` : normalized;
}

export function SystemOverview() {
  const healthQuery = useSystemHealth();
  const eventSubQuery = useEventSubStatus();
  const warningRows = healthQuery.data?.serviceWarnings ?? [];
  const hasSystemWarning =
    Boolean(healthQuery.data?.analyticsDbFingerprintMismatch) ||
    Boolean(healthQuery.data?.rawChatLastError) ||
    warningRows.length > 0;
  const chartRows = [
    { name: 'Memory', value: Number(healthQuery.data?.memoryBytes ?? healthQuery.data?.memoryRssBytes ?? 0) / (1024 * 1024) },
    { name: 'Subs', value: eventSubQuery.data?.activeSubscriptionCount ?? 0 },
    { name: 'LastTickAge', value: healthQuery.data?.lastTickAgeSeconds ?? 0 },
    { name: 'RawChatLag', value: healthQuery.data?.rawChatLagSeconds ?? 0 },
  ];

  return (
    <motion.section initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="space-y-5">
      <PageHeader
        title="System Overview"
        description="Bot-Health, Runtime-Signale und aktuelle Service-Warnungen im Überblick."
        primaryAction={<StatusBadge status={hasSystemWarning ? 'warning' : healthQuery.isSuccess ? 'active' : 'warning'} />}
      />

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-5">
        <KpiCard title="Uptime" value={formatDuration(healthQuery.data?.uptimeSeconds)} hint="Prozesslaufzeit" />
        <KpiCard title="Memory" value={formatBytes(healthQuery.data?.memoryBytes ?? healthQuery.data?.memoryRssBytes)} hint="RSS / Process memory" tone="primary" />
        <KpiCard title="Last Tick" value={formatRelativeTime(healthQuery.data?.lastTickAt)} hint="vom Backend gemeldet" tone="accent" />
        <KpiCard title="EventSub Subs" value={String(eventSubQuery.data?.activeSubscriptionCount ?? 0)} hint="aktive Twitch-Subscriptions" />
        <KpiCard
          title="Raw Chat Lag"
          value={
            healthQuery.data?.rawChatLagSeconds != null
              ? formatDuration(healthQuery.data.rawChatLagSeconds)
              : '—'
          }
          hint={
            healthQuery.data?.rawChatLagStreamer
              ? `betroffen: ${healthQuery.data.rawChatLagStreamer}`
              : 'kein live Kanal im Snapshot'
          }
          tone={healthQuery.data?.rawChatLastError ? 'accent' : 'neutral'}
        />
      </div>

      <section className="grid gap-5 xl:grid-cols-[1.1fr_0.9fr]">
        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Signal Overlay</p>
          <div className="mt-6 h-72">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={chartRows}>
                <CartesianGrid stroke="rgba(255,255,255,0.06)" vertical={false} />
                <XAxis dataKey="name" stroke="#9bb3c5" tickLine={false} axisLine={false} />
                <YAxis stroke="#9bb3c5" tickLine={false} axisLine={false} />
                <Tooltip contentStyle={{ background: '#0f2431', border: '1px solid rgba(255,255,255,0.1)', borderRadius: '16px' }} />
                <Area type="monotone" dataKey="value" stroke="#10b7ad" fill="rgba(16,183,173,0.25)" />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </article>

        <article className="panel-card rounded-[1.8rem] p-6">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Warnings & DB</p>
          <div className="mt-4 rounded-[1.2rem] border border-white/10 bg-white/[0.03] p-4">
            <p className="text-xs uppercase tracking-[0.18em] text-text-secondary">Analytics DB</p>
            <p className="mt-2 font-medium text-white">{formatFingerprint(healthQuery.data?.analyticsDbFingerprint)}</p>
            <p className="mt-1 text-sm text-text-secondary">
              Internal API: {formatFingerprint(healthQuery.data?.internalAnalyticsDbFingerprint)}
            </p>
            {healthQuery.data?.analyticsDbFingerprintMismatch ? (
              <p className="mt-2 text-sm text-warning">Fingerprint-Mismatch zwischen Dashboard und Bot-Service.</p>
            ) : null}
          </div>
          <div className="mt-4 space-y-3">
            {warningRows.length ? (
              warningRows.slice(0, 6).map((warning, index) => (
                <div key={`${index}-${String(warning.message ?? warning.text ?? 'warning')}`} className="rounded-[1.2rem] border border-white/10 bg-white/[0.03] p-4">
                  <p className="font-medium text-white">{String(warning.message ?? warning.text ?? 'Warnung')}</p>
                  <p className="mt-1 text-sm text-text-secondary">{String(warning.timestamp ?? warning.ts ?? '')}</p>
                </div>
              ))
            ) : (
              <EmptyState
                icon={ShieldAlert}
                title="Keine Service-Warnungen"
                description="Im aktuellen Health-Payload wurden keine zusätzlichen Warnungen gemeldet."
              />
            )}
          </div>
        </article>
      </section>
    </motion.section>
  );
}
