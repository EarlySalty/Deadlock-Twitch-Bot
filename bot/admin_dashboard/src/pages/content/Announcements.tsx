import { RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { StickyActionBar } from '@/components/layout/StickyActionBar';
import { DeDateTimeInput, isoLocalToDe } from '@/components/shared/DeDateTimeInput';
import { TextPreview } from '@/components/shared/TextPreview';
import { Toast } from '@/components/shared/Toast';
import { useConfigOverview, usePromoConfigMutation } from '@/hooks/useAdmin';
import { berlinLocalInputToUtcIso, berlinNowLocalInput, utcIsoToBerlinLocalInput } from '@/utils/berlinTime';
import { coerceRecord, formatDateTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  tone: 'success' | 'error';
  message: string;
};

type PromoDraft = {
  enabled: boolean;
  body: string;
  startsAt: string;
  endsAt: string;
};

function readStr(record: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string') {
      return value;
    }
  }
  return '';
}

function readNullableStr(record: Record<string, unknown>, ...keys: string[]): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string') {
      return value;
    }
    if (value === null) {
      return null;
    }
  }
  return null;
}

function readBool(record: Record<string, unknown>, ...keys: string[]): boolean {
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
      if (normalized) {
        return !['0', 'false', 'off', 'no'].includes(normalized);
      }
    }
  }
  return false;
}

function draftFromConfig(record: Record<string, unknown>): PromoDraft {
  return {
    enabled: readBool(record, 'is_enabled', 'enabled'),
    body: readStr(record, 'custom_message', 'message', 'promo_message'),
    startsAt: utcIsoToBerlinLocalInput(readNullableStr(record, 'starts_at', 'startsAt')),
    endsAt: utcIsoToBerlinLocalInput(readNullableStr(record, 'ends_at', 'endsAt')),
  };
}

function computeStatus(draft: PromoDraft): string {
  if (!draft.enabled) return 'inaktiv';
  if (!draft.body.trim()) return 'kein Text';
  const now = berlinNowLocalInput();
  if (draft.startsAt && now < draft.startsAt) return 'geplant';
  if (draft.endsAt && now > draft.endsAt) return 'abgelaufen';
  return 'aktiv';
}

export default function AnnouncementsPage() {
  const query = useConfigOverview();
  const promoMutation = usePromoConfigMutation();
  const [draft, setDraft] = useState<PromoDraft>({ enabled: false, body: '', startsAt: '', endsAt: '' });
  const [saved, setSaved] = useState<PromoDraft>({ enabled: false, body: '', startsAt: '', endsAt: '' });
  const [lastSavedAt, setLastSavedAt] = useState<string | null>(null);
  const [lastSavedBy, setLastSavedBy] = useState<string | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const dirty =
    draft.enabled !== saved.enabled ||
    draft.body !== saved.body ||
    draft.startsAt !== saved.startsAt ||
    draft.endsAt !== saved.endsAt;

  useEffect(() => {
    if (!query.data) {
      return;
    }
    if (initialized && dirty) {
      return;
    }
    const config = coerceRecord(query.data.announcements);
    const next = draftFromConfig(config);
    setDraft(next);
    setSaved(next);
    setLastSavedAt(readNullableStr(config, 'updated_at', 'updatedAt'));
    setLastSavedBy(readNullableStr(config, 'updated_by', 'updatedBy') || null);
    setInitialized(true);
  }, [dirty, initialized, query.data]);

  async function handleSave() {
    try {
      const payload = {
        mode: draft.enabled ? 'custom_event' : 'standard',
        custom_message: draft.body,
        starts_at: draft.startsAt ? berlinLocalInputToUtcIso(draft.startsAt) : null,
        ends_at: draft.endsAt ? berlinLocalInputToUtcIso(draft.endsAt) : null,
        is_enabled: draft.enabled,
      };
      const response = coerceRecord(await promoMutation.mutateAsync(payload));
      const next = draftFromConfig(response);
      setDraft(next);
      setSaved(next);
      setLastSavedAt(readNullableStr(response, 'updated_at', 'updatedAt'));
      setLastSavedBy(readNullableStr(response, 'updated_by', 'updatedBy') || null);
      setInitialized(true);
      setToast({ open: true, tone: 'success', message: 'Announcement gespeichert.' });
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message: error instanceof Error ? error.message : 'Announcement konnte nicht gespeichert werden.',
      });
    }
  }

  if (query.isLoading && !initialized) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Announcements werden geladen …</div>;
  }

  if (query.isError && !initialized) {
    return (
      <div className="panel-card rounded-[1.8rem] p-8 text-white">
        {query.error instanceof Error ? query.error.message : 'Announcements konnten nicht geladen werden.'}
      </div>
    );
  }

  const status = computeStatus(draft);

  return (
    <section className="space-y-6">
      <PageHeader
        title="Announcements"
        description="Globaler Announcement-Text mit Aktivierung und Zeitfenster (deutsche Ortszeit) für den Bot."
        primaryAction={
          <button
            className="admin-button admin-button-secondary"
            onClick={() => void query.refetch()}
            disabled={query.isFetching}
          >
            <RefreshCw className={`h-4 w-4 ${query.isFetching ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        }
      />

      <div className="grid gap-6 xl:grid-cols-[1.1fr_0.9fr]">
        <Section title="Editor" hint="Text, Aktivierung und Zeitfenster für den globalen Announcement-Modus.">
          <div className="space-y-4">
            <label className="flex items-center justify-between rounded-[1.2rem] border border-white/10 bg-white/[0.03] px-4 py-3">
              <span className="text-sm font-medium text-white">Announcement aktiv</span>
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(event) => setDraft((current) => ({ ...current, enabled: event.target.checked }))}
              />
            </label>

            <label className="block space-y-2">
              <span className="text-sm font-medium text-white">Body</span>
              <textarea
                rows={18}
                value={draft.body}
                onChange={(event) => setDraft((current) => ({ ...current, body: event.target.value }))}
                className="admin-input min-h-[22rem] resize-y font-mono text-sm leading-6"
                placeholder="Event-Announcement eingeben"
              />
            </label>

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label className="flex flex-col gap-1">
                <span className="text-xs font-semibold uppercase tracking-widest text-text-secondary">Start (Ortszeit)</span>
                <DeDateTimeInput
                  value={draft.startsAt}
                  onChange={(next) => setDraft((current) => ({ ...current, startsAt: next }))}
                />
                <span className="text-xs text-text-secondary">{draft.startsAt ? '' : 'Leer = ab sofort'}</span>
              </label>
              <label className="flex flex-col gap-1">
                <span className="text-xs font-semibold uppercase tracking-widest text-text-secondary">Ende (Ortszeit)</span>
                <DeDateTimeInput
                  value={draft.endsAt}
                  onChange={(next) => setDraft((current) => ({ ...current, endsAt: next }))}
                />
                <span className="text-xs text-text-secondary">{draft.endsAt ? '' : 'Leer = kein Ende'}</span>
              </label>
            </div>
          </div>
        </Section>

        <Section title="Preview" hint="Sichere Text-Vorschau ohne HTML-Ausfuehrung.">
          <div className="space-y-4">
            <div className="rounded-[1.5rem] border border-white/10 bg-bg/35 p-5">
              <TextPreview value={draft.body} emptyMessage="Noch kein Announcement-Text vorhanden." />
            </div>
            <div className="rounded-[1.5rem] border border-white/10 bg-bg/35 p-5 text-sm text-white">
              <p>
                Status: <span className="font-semibold">{status}</span>
              </p>
              <p className="mt-1 text-text-secondary">
                Zeitfenster: {draft.startsAt ? isoLocalToDe(draft.startsAt) : 'ab sofort'} bis{' '}
                {draft.endsAt ? isoLocalToDe(draft.endsAt) : 'unbegrenzt'} (Ortszeit)
              </p>
            </div>
          </div>
        </Section>
      </div>

      <StickyActionBar
        lastSavedAt={lastSavedAt ? formatDateTime(lastSavedAt) : null}
        dirty={dirty}
        onSave={() => void handleSave()}
        onDiscard={() => setDraft(saved)}
        saving={promoMutation.isPending}
      >
        {lastSavedBy ? <span className="stat-pill">Zuletzt von {lastSavedBy}</span> : null}
      </StickyActionBar>

      <Toast
        open={toast.open}
        tone={toast.tone}
        message={toast.message}
        onClose={() => setToast((current) => ({ ...current, open: false }))}
      />
    </section>
  );
}
