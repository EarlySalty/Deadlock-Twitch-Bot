import { RefreshCw } from 'lucide-react';
import { useMemo, useState } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { StickyActionBar } from '@/components/layout/StickyActionBar';
import { TextPreview } from '@/components/shared/TextPreview';
import { Toast } from '@/components/shared/Toast';
import { useCreateChangelogEntry, useDashboardOverview } from '@/hooks/useAdmin';
import { formatDateTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  tone: 'success' | 'error';
  message: string;
};

function todayIsoDate() {
  return new Date().toISOString().slice(0, 10);
}

export default function ChangelogPage() {
  const overviewQuery = useDashboardOverview();
  const createMutation = useCreateChangelogEntry();
  const initialEntryDate = useMemo(() => todayIsoDate(), []);
  const [title, setTitle] = useState('');
  const [content, setContent] = useState('');
  const [entryDate, setEntryDate] = useState(initialEntryDate);
  const [lastSavedAt, setLastSavedAt] = useState<string | null>(null);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const dirty = title.trim() !== '' || content.trim() !== '' || entryDate !== initialEntryDate;
  const history = overviewQuery.data?.changelog?.slice(0, 20) ?? [];

  async function handleSave() {
    try {
      const response = await createMutation.mutateAsync({
        title: title.trim(),
        content,
        entry_date: entryDate,
      });
      setLastSavedAt(response.createdAt ?? response.entryDate ?? null);
      setTitle('');
      setContent('');
      setEntryDate(todayIsoDate());
      setToast({ open: true, tone: 'success', message: 'Changelog-Eintrag angelegt.' });
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message: error instanceof Error ? error.message : 'Changelog-Eintrag konnte nicht angelegt werden.',
      });
    }
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="Changelog"
        description="Erfasst neue Internal-Home-Changelog-Einträge. Falls Verlauf verfügbar ist, werden die letzten Einträge darunter gespiegelt."
        primaryAction={
          <button
            className="admin-button admin-button-secondary"
            onClick={() => void overviewQuery.refetch()}
            disabled={overviewQuery.isFetching}
          >
            <RefreshCw className={`h-4 w-4 ${overviewQuery.isFetching ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        }
      />

      <div className="grid gap-6 xl:grid-cols-[1.1fr_0.9fr]">
        <Section title="Editor" hint="Titel optional, Content als Bullet-Markdown oder Freitext.">
          <div className="space-y-5">
            <label className="block space-y-3">
              <span className="text-sm font-medium text-white">Titel</span>
              <input
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                className="admin-input"
                placeholder="Kurzüberschrift"
              />
            </label>

            <label className="block max-w-xs space-y-3">
              <span className="text-sm font-medium text-white">Eintragsdatum</span>
              <input
                type="date"
                value={entryDate}
                onChange={(event) => setEntryDate(event.target.value)}
                className="admin-input"
              />
            </label>

            <label className="block space-y-3">
              <span className="text-sm font-medium text-white">Content</span>
              <textarea
                rows={24}
                value={content}
                onChange={(event) => setContent(event.target.value)}
                className="admin-input min-h-[28rem] resize-y font-mono text-sm leading-6"
                placeholder="- Neuer Punkt&#10;- Noch ein Update"
              />
            </label>
          </div>
        </Section>

        <Section title="Preview" hint="Vorschau des nächsten Changelog-Eintrags.">
          <div className="rounded-[1.5rem] border border-white/10 bg-slate-950/35 p-5">
            <div className="space-y-4">
              <div>
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Datum</p>
                <p className="mt-2 text-sm text-white">{entryDate || '—'}</p>
              </div>
              {title.trim() ? (
                <div>
                  <p className="text-xs font-semibold uppercase tracking-[0.18em] text-text-secondary">Titel</p>
                  <h3 className="mt-2 text-xl font-semibold text-white">{title.trim()}</h3>
                </div>
              ) : null}
              <TextPreview value={content} emptyMessage="Noch kein Content vorhanden." />
            </div>
          </div>
        </Section>
      </div>

      <Section title="History" hint="Die letzten 20 Internal-Home-Changelog-Einträge, sofern das Backend sie im Home-Payload liefert.">
        {history.length === 0 ? (
          <div className="rounded-[1.5rem] border border-dashed border-white/10 bg-white/[0.02] p-5 text-sm text-text-secondary">
            Kein Verlauf verfügbar.
          </div>
        ) : (
          <div className="space-y-4">
            {history.map((entry, index) => (
              <article key={String(entry.id ?? `history-${index}`)} className="rounded-[1.4rem] border border-white/10 bg-white/[0.03] p-5">
                <div className="flex flex-wrap items-center gap-3">
                  <span className="stat-pill">{entry.entryDate || 'ohne Datum'}</span>
                  <span className="text-xs text-text-secondary">
                    {entry.createdAt ? `Erstellt ${formatDateTime(entry.createdAt)}` : 'Ohne Timestamp'}
                  </span>
                </div>
                {entry.title ? <h3 className="mt-4 text-lg font-semibold text-white">{entry.title}</h3> : null}
                <div className="mt-4">
                  <TextPreview value={entry.content} emptyMessage="Kein Content." />
                </div>
              </article>
            ))}
          </div>
        )}
      </Section>

      <StickyActionBar
        lastSavedAt={lastSavedAt ? formatDateTime(lastSavedAt) : null}
        dirty={dirty}
        onSave={() => void handleSave()}
        onDiscard={() => {
          setTitle('');
          setContent('');
          setEntryDate(initialEntryDate);
        }}
        saving={createMutation.isPending}
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
