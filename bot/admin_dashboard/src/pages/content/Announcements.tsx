import { RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { StickyActionBar } from '@/components/layout/StickyActionBar';
import { TextPreview } from '@/components/shared/TextPreview';
import { Toast } from '@/components/shared/Toast';
import { useAnnouncements, useSaveAnnouncements } from '@/hooks/useAdmin';
import { formatDateTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  tone: 'success' | 'error';
  message: string;
};

export default function AnnouncementsPage() {
  const query = useAnnouncements();
  const saveMutation = useSaveAnnouncements();
  const [body, setBody] = useState('');
  const [savedBody, setSavedBody] = useState('');
  const [lastSavedAt, setLastSavedAt] = useState<string | null>(null);
  const [lastSavedBy, setLastSavedBy] = useState<string | null>(null);
  const [initialized, setInitialized] = useState(false);
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const dirty = body !== savedBody;

  useEffect(() => {
    if (!query.data) {
      return;
    }
    if (!initialized || !dirty) {
      setBody(query.data.body);
      setSavedBody(query.data.body);
      setLastSavedAt(query.data.lastUpdatedAt ?? null);
      setLastSavedBy(query.data.lastUpdatedBy ?? null);
      setInitialized(true);
    }
  }, [dirty, initialized, query.data]);

  async function handleSave() {
    try {
      const response = await saveMutation.mutateAsync(body);
      setBody(response.body);
      setSavedBody(response.body);
      setLastSavedAt(response.lastUpdatedAt ?? null);
      setLastSavedBy(response.lastUpdatedBy ?? null);
      setInitialized(true);
      setToast({ open: true, tone: 'success', message: 'Announcements gespeichert.' });
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message: error instanceof Error ? error.message : 'Announcements konnten nicht gespeichert werden.',
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

  return (
    <section className="space-y-6">
      <PageHeader
        title="Announcements"
        description="Bearbeitet den globalen Announcement-Text für den Bot. Timing und Modus bleiben in den bestehenden Bot-Control-Flows."
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
        <Section title="Editor" hint="Direkter Text-Body für den globalen Announcement-Modus.">
          <label className="block space-y-3">
            <span className="text-sm font-medium text-white">Body</span>
            <textarea
              rows={24}
              value={body}
              onChange={(event) => setBody(event.target.value)}
              className="admin-input min-h-[32rem] resize-y font-mono text-sm leading-6"
              placeholder="Event-Announcement eingeben"
            />
          </label>
        </Section>

        <Section title="Preview" hint="Sichere Text-Vorschau ohne HTML-Ausfuehrung.">
          <div className="rounded-[1.5rem] border border-white/10 bg-bg/35 p-5">
            <TextPreview value={body} emptyMessage="Noch kein Announcement-Text vorhanden." />
          </div>
        </Section>
      </div>

      <StickyActionBar
        lastSavedAt={lastSavedAt ? formatDateTime(lastSavedAt) : null}
        dirty={dirty}
        onSave={() => void handleSave()}
        onDiscard={() => setBody(savedBody)}
        saving={saveMutation.isPending}
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
