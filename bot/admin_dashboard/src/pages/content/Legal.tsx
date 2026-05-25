import { RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';
import type { LegalPageDocument, LegalPageSlug } from '@/api/types';
import { PageHeader } from '@/components/layout/PageHeader';
import { Section } from '@/components/layout/Section';
import { StickyActionBar } from '@/components/layout/StickyActionBar';
import { TextPreview } from '@/components/shared/TextPreview';
import { Toast } from '@/components/shared/Toast';
import { useLegalPage, useSaveLegalPage } from '@/hooks/useAdmin';
import { formatDateTime } from '@/utils/formatters';

type ToastState = {
  open: boolean;
  tone: 'success' | 'error';
  message: string;
};

type DraftState = {
  title: string;
  body: string;
};

const LEGAL_TABS: Array<{ slug: LegalPageSlug; label: string }> = [
  { slug: 'impressum', label: 'Impressum' },
  { slug: 'datenschutz', label: 'Datenschutz' },
  { slug: 'agb', label: 'AGB' },
];

export default function LegalPage() {
  const [activeSlug, setActiveSlug] = useState<LegalPageSlug>('impressum');
  const query = useLegalPage(activeSlug);
  const saveMutation = useSaveLegalPage(activeSlug);
  const [savedDocs, setSavedDocs] = useState<Partial<Record<LegalPageSlug, LegalPageDocument>>>({});
  const [drafts, setDrafts] = useState<Partial<Record<LegalPageSlug, DraftState>>>({});
  const [toast, setToast] = useState<ToastState>({ open: false, tone: 'success', message: '' });

  const currentSaved = savedDocs[activeSlug];
  const currentDraft = drafts[activeSlug] ?? {
    title: currentSaved?.title ?? '',
    body: currentSaved?.body ?? '',
  };
  const dirty =
    currentDraft.title !== (currentSaved?.title ?? '') || currentDraft.body !== (currentSaved?.body ?? '');

  useEffect(() => {
    if (!query.data) {
      return;
    }

    setSavedDocs((previous) => ({
      ...previous,
      [activeSlug]: query.data,
    }));

    setDrafts((previous) => {
      const existingDraft = previous[activeSlug];
      const existingSaved = currentSaved;
      const isDirty =
        existingDraft !== undefined &&
        existingSaved !== undefined &&
        (existingDraft.title !== existingSaved.title || existingDraft.body !== existingSaved.body);

      if (isDirty) {
        return previous;
      }

      return {
        ...previous,
        [activeSlug]: {
          title: query.data.title,
          body: query.data.body,
        },
      };
    });
  }, [activeSlug, currentSaved, query.data]);

  async function handleSave() {
    try {
      const response = await saveMutation.mutateAsync({
        title: currentDraft.title,
        body: currentDraft.body,
      });
      setSavedDocs((previous) => ({ ...previous, [activeSlug]: response }));
      setDrafts((previous) => ({
        ...previous,
        [activeSlug]: {
          title: response.title,
          body: response.body,
        },
      }));
      setToast({ open: true, tone: 'success', message: `${response.title} gespeichert.` });
    } catch (error) {
      setToast({
        open: true,
        tone: 'error',
        message: error instanceof Error ? error.message : 'Legal-Page konnte nicht gespeichert werden.',
      });
    }
  }

  if (query.isLoading && !currentSaved) {
    return <div className="panel-card rounded-[1.8rem] p-8 text-white">Legal-Page wird geladen …</div>;
  }

  return (
    <section className="space-y-6">
      <PageHeader
        title="Legal Pages"
        description="Drei editierbare Rechtsseiten mit gemeinsamer Persistenz für Admin-JSON und die öffentlichen /twitch/*-Seiten."
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
        secondaryChips={
          <>
            {LEGAL_TABS.map((tab) => (
              <button
                key={tab.slug}
                type="button"
                onClick={() => setActiveSlug(tab.slug)}
                className={[
                  'rounded-full border px-4 py-2 text-sm transition',
                  activeSlug === tab.slug
                    ? 'border-orange-300/40 bg-orange-500/15 text-orange-100'
                    : 'border-white/10 bg-white/[0.03] text-text-secondary hover:border-white/20 hover:text-white',
                ].join(' ')}
              >
                {tab.label}
              </button>
            ))}
          </>
        }
      />

      {query.isError && !currentSaved ? (
        <div className="panel-card rounded-[1.8rem] p-8 text-white">
          {query.error instanceof Error ? query.error.message : 'Legal-Page konnte nicht geladen werden.'}
        </div>
      ) : (
        <>
          <div className="grid gap-6 xl:grid-cols-[1.1fr_0.9fr]">
            <Section title="Editor" hint={`Bearbeitet ${LEGAL_TABS.find((tab) => tab.slug === activeSlug)?.label ?? activeSlug}.`}>
              <div className="space-y-5">
                <label className="block space-y-3">
                  <span className="text-sm font-medium text-white">Titel</span>
                  <input
                    value={currentDraft.title}
                    onChange={(event) =>
                      setDrafts((previous) => ({
                        ...previous,
                        [activeSlug]: {
                          ...currentDraft,
                          title: event.target.value,
                        },
                      }))
                    }
                    className="admin-input"
                    placeholder="Seitentitel"
                  />
                </label>

                <label className="block space-y-3">
                  <span className="text-sm font-medium text-white">Body</span>
                  <textarea
                    rows={24}
                    value={currentDraft.body}
                    onChange={(event) =>
                      setDrafts((previous) => ({
                        ...previous,
                        [activeSlug]: {
                          ...currentDraft,
                          body: event.target.value,
                        },
                      }))
                    }
                    className="admin-input min-h-[32rem] resize-y font-mono text-sm leading-6"
                    placeholder="HTML- oder Text-Body der Legal-Page"
                  />
                </label>

                <p className="text-xs text-text-secondary">
                  Letzte Aenderung am{' '}
                  {currentSaved?.lastUpdatedAt ? formatDateTime(currentSaved.lastUpdatedAt) : 'Standard-Fallback'} durch{' '}
                  {currentSaved?.lastUpdatedBy || 'System'}.
                </p>
              </div>
            </Section>

            <Section title="Preview" hint="Sichere Vorschau des gespeicherten Inhalts.">
              <div className="rounded-[1.5rem] border border-white/10 bg-slate-950/35 p-5">
                <div className="space-y-4">
                  <h3 className="text-2xl font-semibold text-white">{currentDraft.title || 'Ohne Titel'}</h3>
                  <TextPreview value={currentDraft.body} emptyMessage="Noch kein Body vorhanden." />
                </div>
              </div>
            </Section>
          </div>

          <StickyActionBar
            lastSavedAt={currentSaved?.lastUpdatedAt ? formatDateTime(currentSaved.lastUpdatedAt) : null}
            dirty={dirty}
            onSave={() => void handleSave()}
            onDiscard={() =>
              setDrafts((previous) => ({
                ...previous,
                [activeSlug]: {
                  title: currentSaved?.title ?? '',
                  body: currentSaved?.body ?? '',
                },
              }))
            }
            saving={saveMutation.isPending}
          >
            {currentSaved?.lastUpdatedBy ? <span className="stat-pill">Zuletzt von {currentSaved.lastUpdatedBy}</span> : null}
          </StickyActionBar>
        </>
      )}

      <Toast
        open={toast.open}
        tone={toast.tone}
        message={toast.message}
        onClose={() => setToast((current) => ({ ...current, open: false }))}
      />
    </section>
  );
}
