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
  color: AnnouncementColor;
};

const announcementColors = [
  { value: 'primary', label: 'Kanalfarbe', swatch: '#b8b8b8' },
  { value: 'blue', label: 'Blau', swatch: '#47adff' },
  { value: 'green', label: 'Grün', swatch: '#00d69b' },
  { value: 'orange', label: 'Orange', swatch: '#ffb31a' },
  { value: 'purple', label: 'Lila', swatch: '#c299ff' },
] as const;
type AnnouncementColor = typeof announcementColors[number]['value'];

function readColor(record: Record<string, unknown>): AnnouncementColor {
  return announcementColors.find(color => color.value === record.announcement_color)?.value ?? 'purple';
}

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
    color: readColor(record),
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
  const [draft, setDraft] = useState<PromoDraft>({ enabled: false, body: '', startsAt: '', endsAt: '', color: 'purple' });
  const [saved, setSaved] = useState<PromoDraft>({ enabled: false, body: '', startsAt: '', endsAt: '', color: 'purple' });
  const [lastSavedAt, setLastSavedAt] = useState<string | null>(null);
  const [lastSavedBy, setLastSavedBy] = useState<string | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const dirty =
    draft.enabled !== saved.enabled ||
    draft.body !== saved.body ||
    draft.startsAt !== saved.startsAt ||
    draft.endsAt !== saved.endsAt ||
    draft.color !== saved.color;

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
        announcement_color: draft.color,
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
          <fieldset disabled={promoMutation.isPending} className="min-w-0 space-y-4">
            <label className="flex items-center justify-between rounded-[1.2rem] border border-white/10 bg-white/[0.03] px-4 py-3">
              <span className="text-sm font-medium text-white">Announcement aktiv</span>
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(event) => setDraft((current) => ({ ...current, enabled: event.target.checked }))}
              />
            </label>

            <fieldset className="min-w-0">
              <legend className="mb-2 text-sm font-medium text-white">Announcement-Farbe</legend>
              <div className="flex flex-wrap gap-2">
                {announcementColors.map(color => (
                  <label key={color.value} className={`flex cursor-pointer items-center gap-2 rounded-xl border px-3 py-2 text-sm text-white ${draft.color === color.value ? 'border-white/70 bg-white/10' : 'border-white/15 bg-black/20'}`}>
                    <input type="radio" name="announcement-color" value={color.value} checked={draft.color === color.value}
                      onChange={() => setDraft(current => ({ ...current, color: color.value }))} />
                    <span aria-hidden="true" className="h-3 w-3 rounded-full" style={{ backgroundColor: color.swatch }} />
                    {color.label}
                  </label>
                ))}
              </div>
              <p className="mt-2 text-xs leading-5 text-text-secondary">Kanalfarbe übernimmt die Akzentfarbe des jeweiligen Twitch-Kanals. Die Auswahl gilt für dieses globale Announcement.</p>
            </fieldset>

            <label className="block space-y-2">
              <span className="text-sm font-medium text-white">Text</span>
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
          </fieldset>
        </Section>

        <Section title="Vorschau" hint="Die Vorschau zeigt die gewählte Farbe ungefähr; die Darstellung im Chat übernimmt Twitch.">
          <div className="space-y-4">
            <div className="rounded-[1.5rem] border border-white/10 border-l-4 bg-bg/35 p-5" style={{ borderLeftColor: announcementColors.find(color => color.value === draft.color)?.swatch }}>
              <p className="mb-3 text-xs font-semibold uppercase tracking-wider text-white">Announcement · {announcementColors.find(color => color.value === draft.color)?.label}</p>
              <TextPreview value={draft.body} emptyMessage="Noch kein Announcement-Text vorhanden." />
              {draft.color === 'primary' && <p className="mt-3 text-xs text-text-secondary">Die Kanalfarbe unterscheidet sich je Twitch-Kanal.</p>}
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
