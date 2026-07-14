import { useEffect, useMemo, useState } from 'react';
import { Bot, RefreshCw, RotateCcw, TriangleAlert } from 'lucide-react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { ConfirmDialog } from '@/components/shared/ConfirmDialog';
import { Toast } from '@/components/shared/Toast';
import { StatusBadge } from '@/components/shared/StatusBadge';
import { useConfigOverview, usePromoConfigMutation, useReloadBot } from '@/hooks/useAdmin';
import { coerceRecord, formatRelativeTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  tone: 'success' | 'error';
  message: string;
};

function readString(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
}

function readBoolean(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'boolean') {
      return value;
    }
    if (typeof value === 'number') {
      return value !== 0;
    }
    if (typeof value === 'string') {
      const normalized = value.trim().toLowerCase();
      if (!normalized) {
        continue;
      }
      return !['0', 'false', 'off', 'no'].includes(normalized);
    }
  }
  return undefined;
}

function readNullableString(record: Record<string, unknown>, ...keys: string[]) {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string') {
      return value;
    }
    if (value === null) {
      return null;
    }
  }
  return undefined;
}

function buildPromoPayload(promo: Record<string, unknown>, promoConfig: Record<string, unknown>, enabled: boolean) {
  const mode = readString(promoConfig, 'mode') || readString(promo, 'mode') || 'custom_event';
  const customMessage =
    readString(promoConfig, 'custom_message', 'message', 'promo_message') ||
    readString(promo, 'active_message', 'message', 'promo_message');

  return {
    mode,
    custom_message: customMessage,
    starts_at: readNullableString(promoConfig, 'starts_at', 'startsAt') ?? null,
    ends_at: readNullableString(promoConfig, 'ends_at', 'endsAt') ?? null,
    is_enabled: enabled,
  };
}

function extractPromoSnapshot(promo: Record<string, unknown>) {
  const config = coerceRecord(promo.config);
  return {
    enabled:
      readBoolean(config, 'is_enabled', 'enabled') ??
      readBoolean(promo, 'is_enabled', 'enabled', 'is_active') ??
      false,
    mode: readString(config, 'mode') || readString(promo, 'mode'),
    lastUpdatedAt:
      readString(
        promo,
        'lastUpdatedAt',
        'last_updated_at',
        'updatedAt',
        'updated_at',
        'changedAt',
        'changed_at',
      ) ||
      readString(
        config,
        'lastUpdatedAt',
        'last_updated_at',
        'updatedAt',
        'updated_at',
        'changedAt',
        'changed_at',
      ),
    message:
      readString(config, 'custom_message', 'message', 'promo_message') ||
      readString(promo, 'active_message', 'message', 'promo_message'),
    config,
  };
}

function renderValueWithFallback(value: string) {
  if (!value) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-white">—</span>
        <StatusBadge status="warning" />
      </div>
    );
  }
  return <span className="text-white">{value}</span>;
}

export default function BotControlPage() {
  const configQuery = useConfigOverview();
  const promoMutation = usePromoConfigMutation();
  const reloadMutation = useReloadBot();
  const [confirmReloadOpen, setConfirmReloadOpen] = useState(false);
  const [confirmPromoOpen, setConfirmPromoOpen] = useState(false);
  const [promoIntentEnabled, setPromoIntentEnabled] = useState<boolean | null>(null);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const promo = coerceRecord(configQuery.data?.promo);
  const promoSnapshot = useMemo(() => extractPromoSnapshot(promo), [promo]);
  const announcementDefaults = coerceRecord(configQuery.data?.announcements);

  useEffect(() => {
    if (!confirmPromoOpen) {
      setPromoIntentEnabled(null);
    }
  }, [confirmPromoOpen]);

  if (configQuery.isLoading && !configQuery.data) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Bot-Control wird geladen …</div>;
  }

  if (configQuery.isError) {
    return (
      <section className="space-y-6">
        <PageHeader
          title="Bot Control"
          description="Globale Bot-Aktionen und globale Schalter."
          primaryAction={
            <button className="admin-button admin-button-secondary" onClick={() => void configQuery.refetch()}>
              <RefreshCw className="h-4 w-4" />
              Refresh
            </button>
          }
        />
        <div className="panel-card rounded-[1.8rem] p-8 text-white">
          {configQuery.error instanceof Error ? configQuery.error.message : 'Config-Overview konnte nicht geladen werden.'}
        </div>
      </section>
    );
  }

  const reloadStateLabel = reloadMutation.isPending ? 'Reload läuft …' : configQuery.isFetching ? 'Refreshing …' : 'Idle';
  const promoStatusLabel = promoSnapshot.enabled ? 'Aktiv' : 'Inaktiv';
  const promoTimeLabel = promoSnapshot.lastUpdatedAt
    ? `${promoSnapshot.enabled ? 'Aktiv seit' : 'Inaktiv seit'} ${formatRelativeTime(promoSnapshot.lastUpdatedAt)}`
    : '—';
  const announcementKeys = Object.keys(announcementDefaults);

  return (
    <section className="space-y-6">
      <PageHeader
        title="Bot Control"
        description="Globale Bot-Aktionen und globale Schalter."
        primaryAction={
          <button className="admin-button admin-button-secondary" onClick={() => void configQuery.refetch()} disabled={configQuery.isFetching}>
            <RefreshCw className={`h-4 w-4 ${configQuery.isFetching ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        }
      />

      <Section title="Bot Reload" hint="Lädt alle Discord-Cogs neu, hilft bei festgefahrenen Tasks">
        <div className="space-y-5">
          <div className="flex flex-wrap items-center gap-3">
            <span className="stat-pill">Status: {reloadStateLabel}</span>
            <StatusBadge status={reloadMutation.isPending ? 'warning' : 'ok'} />
          </div>

          <div className="rounded-[1.5rem] border border-danger/20 bg-danger/[0.04] p-5">
            <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
              <div className="max-w-2xl">
                <h3 className="text-lg font-semibold text-white">Reload aller Bot-Cogs</h3>
                <p className="mt-2 text-sm leading-6 text-text-secondary">
                  Nutze den Reload nur bei festgefahrenen Tasks oder nach kritischen Runtime-Aenderungen.
                </p>
              </div>
              <button
                className="admin-button admin-button-danger !px-5 !py-3"
                onClick={() => setConfirmReloadOpen(true)}
                disabled={reloadMutation.isPending}
              >
                <RotateCcw className="h-4 w-4" />
                Bot jetzt reloaden
              </button>
            </div>
          </div>
        </div>
      </Section>

      <Section title="Promo-Mode (Global)" hint="Steuert promotional chat-messages im Bot">
        <div className="grid gap-5 lg:grid-cols-[1.3fr_0.9fr]">
          <article className="rounded-[1.5rem] border border-white/10 bg-white/[0.03] p-5">
            <div className="flex items-start justify-between gap-4">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Aktueller Status</p>
                <div className="mt-3 flex flex-wrap items-center gap-3">
                  <StatusBadge status={promoSnapshot.enabled ? 'active' : 'inactive'} />
                  {promoSnapshot.mode ? <span className="stat-pill">Mode: {promoSnapshot.mode}</span> : <StatusBadge status="warning" />}
                </div>
                <p className="mt-4 text-sm text-text-secondary">{promoTimeLabel}</p>
              </div>

              <button
                type="button"
                role="switch"
                aria-checked={promoSnapshot.enabled}
                onClick={() => {
                  setPromoIntentEnabled(!promoSnapshot.enabled);
                  setConfirmPromoOpen(true);
                }}
                disabled={promoMutation.isPending}
                className={[
                  'relative inline-flex h-8 w-14 items-center rounded-full border transition',
                  promoSnapshot.enabled
                    ? 'border-success/35 bg-success/18'
                    : 'border-white/10 bg-white/10',
                ].join(' ')}
              >
                <span
                  className={[
                    'inline-block h-6 w-6 rounded-full bg-white transition',
                    promoSnapshot.enabled ? 'translate-x-7' : 'translate-x-1',
                  ].join(' ')}
                />
              </button>
            </div>

            <div className="mt-5 rounded-[1.2rem] border border-white/10 bg-bg/35 p-4">
              <p className="text-xs font-semibold uppercase tracking-[0.16em] text-text-secondary">Promo Message</p>
              <p className="mt-3 whitespace-pre-wrap text-sm leading-6 text-white/90">
                {promoSnapshot.message || '—'}
              </p>
            </div>
          </article>

          <article className="rounded-[1.5rem] border border-white/10 bg-white/[0.03] p-5">
            <div className="flex items-center gap-3">
              <Bot className="h-5 w-5 text-white/80" />
              <h3 className="text-base font-semibold text-white">Mutation-Zustand</h3>
            </div>
            <div className="mt-4 space-y-3 text-sm text-text-secondary">
              <div className="flex items-center justify-between gap-3 rounded-[1rem] border border-white/10 bg-white/[0.03] px-4 py-3">
                <span>Promo aktiv</span>
                <span className="text-white">{promoStatusLabel}</span>
              </div>
              <div className="flex items-center justify-between gap-3 rounded-[1rem] border border-white/10 bg-white/[0.03] px-4 py-3">
                <span>Letzte Aenderung</span>
                {renderValueWithFallback(promoSnapshot.lastUpdatedAt ? formatRelativeTime(promoSnapshot.lastUpdatedAt) : '')}
              </div>
              <div className="flex items-center justify-between gap-3 rounded-[1rem] border border-white/10 bg-white/[0.03] px-4 py-3">
                <span>Mutation</span>
                <StatusBadge status={promoMutation.isPending ? 'warning' : 'ok'} />
              </div>
            </div>
          </article>
        </div>
      </Section>

      <Section title="Live-Announcement (global)" hint="Defaults für alle Streamer">
        {announcementKeys.length ? (
          <div className="space-y-4">
            <div className="flex flex-wrap gap-3">
              {readBoolean(announcementDefaults, 'enabled', 'is_enabled', 'active', 'is_active') !== undefined ? (
                <span className="stat-pill">
                  Default: {readBoolean(announcementDefaults, 'enabled', 'is_enabled', 'active', 'is_active') ? 'aktiv' : 'inaktiv'}
                </span>
              ) : (
                <StatusBadge status="warning" />
              )}
              {readString(announcementDefaults, 'updatedAt', 'updated_at', 'lastUpdatedAt', 'last_updated_at') ? (
                <span className="stat-pill">
                  Update {formatRelativeTime(readString(announcementDefaults, 'updatedAt', 'updated_at', 'lastUpdatedAt', 'last_updated_at'))}
                </span>
              ) : (
                <div className="flex items-center gap-2">
                  <span className="text-white">—</span>
                  <StatusBadge status="warning" />
                </div>
              )}
            </div>

            <div className="grid gap-4 lg:grid-cols-2">
              <article className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4">
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Titel / Template</p>
                <p className="mt-3 text-sm leading-6 text-white/90">
                  {readString(announcementDefaults, 'title', 'template', 'message', 'default_message') || '—'}
                </p>
              </article>
              <article className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-4">
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Hinweis</p>
                <p className="mt-3 text-sm leading-6 text-text-secondary">
                  Editor folgt in Schritt 5 (Content & Comms).
                </p>
              </article>
            </div>
          </div>
        ) : (
          <div className="rounded-[1.5rem] border border-warning/20 bg-warning/[0.04] p-5">
            <div className="flex items-start gap-3">
              <TriangleAlert className="mt-0.5 h-5 w-5 text-warning" />
              <div>
                <p className="text-sm font-semibold text-white">Editor folgt in Schritt 5 (Content & Comms)</p>
                <p className="mt-2 text-sm leading-6 text-text-secondary">
                  Über `fetchConfigOverview()` ist aktuell kein belastbarer Default-Snapshot für Live-Announcements verfügbar.
                </p>
              </div>
            </div>
          </div>
        )}
      </Section>

      <ConfirmDialog
        open={confirmReloadOpen}
        title="Bot reloaden?"
        description="Der Reload stößt den Legacy-Reload für alle Cogs an. Laufende Tasks können kurz unterbrochen werden."
        confirmLabel="Reload ausführen"
        cancelLabel="Abbrechen"
        tone="danger"
        busy={reloadMutation.isPending}
        onCancel={() => setConfirmReloadOpen(false)}
        onConfirm={async () => {
          try {
            const result = await reloadMutation.mutateAsync();
            setToast({
              open: true,
              tone: result.ok ? 'success' : 'error',
              message: result.message || 'Reload ausgeführt.',
            });
            if (result.ok) {
              setConfirmReloadOpen(false);
              void configQuery.refetch();
            }
          } catch (error) {
            setToast({
              open: true,
              tone: 'error',
              message: error instanceof Error ? error.message : 'Reload fehlgeschlagen.',
            });
          }
        }}
      />

      <ConfirmDialog
        open={confirmPromoOpen}
        title={promoIntentEnabled ? 'Promo-Mode aktivieren?' : 'Promo-Mode deaktivieren?'}
        description="Die globale Promo-Schaltung wirkt direkt auf die zentralen Bot-Messages."
        confirmLabel={promoIntentEnabled ? 'Aktivieren' : 'Deaktivieren'}
        cancelLabel="Abbrechen"
        busy={promoMutation.isPending}
        onCancel={() => setConfirmPromoOpen(false)}
        onConfirm={async () => {
          const nextEnabled = promoIntentEnabled ?? !promoSnapshot.enabled;
          try {
            await promoMutation.mutateAsync(buildPromoPayload(promo, promoSnapshot.config, nextEnabled));
            setToast({
              open: true,
              tone: 'success',
              message: nextEnabled ? 'Promo-Mode aktiviert.' : 'Promo-Mode deaktiviert.',
            });
            setConfirmPromoOpen(false);
            void configQuery.refetch();
          } catch (error) {
            setToast({
              open: true,
              tone: 'error',
              message: error instanceof Error ? error.message : 'Promo-Mode konnte nicht aktualisiert werden.',
            });
            setConfirmPromoOpen(false);
            void configQuery.refetch();
          }
        }}
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
