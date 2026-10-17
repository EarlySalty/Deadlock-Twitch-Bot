import { useT } from '../../context/LanguageContext';
import { useEffect, useState } from 'react';
import { Loader2, MessageSquare } from 'lucide-react';
import { fetchCommunityHints, saveCommunityHints } from '@/api/communityHinweise';
import { CommandRow, SectionCard } from './ui';

const HILFE =
  'Der Bot weist im Chat ab und zu auf den Discord der Deutschen Deadlock Community hin. Das ist standardmäßig an. Ab Netzwerk Plus kannst du es hier ausmachen.';

export function CommunityHinweiseSection() {
  const t = useT();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [enabled, setEnabled] = useState(true);
  const [canDisable, setCanDisable] = useState(false);
  const [pending, setPending] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);

  useEffect(() => {
    let aktiv = true;
    void (async () => {
      try {
        const data = await fetchCommunityHints();
        if (!aktiv) return;
        setEnabled(data.enabled !== false);
        setCanDisable(Boolean(data.can_disable));
      } catch (e) {
        if (aktiv) setError(e instanceof Error ? e.message : 'Unbekannter Fehler');
      } finally {
        if (aktiv) setLoading(false);
      }
    })();
    return () => {
      aktiv = false;
    };
  }, []);

  const onToggle = async () => {
    if (pending) return;
    const next = !enabled;
    if (!next && !canDisable) return;
    setPending(true);
    setError(null);
    setSavedAt(null);
    setEnabled(next);
    try {
      const saved = await saveCommunityHints(next);
      setEnabled(saved.enabled !== false);
      setCanDisable(Boolean(saved.can_disable));
      setSavedAt(Date.now());
    } catch (e) {
      setEnabled(enabled);
      setError(e instanceof Error ? e.message : 'Speichern fehlgeschlagen');
    } finally {
      setPending(false);
    }
  };

  return (
    <SectionCard
      eyebrow={t('Bot & Schutz')}
      icon={<MessageSquare className="h-4 w-4" />}
      title={t('Community-Hinweise im Chat')}
      description={t(HILFE)}
    >
      {error && (
        <p role="alert" className="mb-2 text-sm text-danger">
          {t(error)}
        </p>
      )}
      {loading ? (
        <div className="flex items-center gap-3 text-text-secondary text-sm">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          {t('Einstellungen werden geladen ...')}
        </div>
      ) : (
        <CommandRow
          icon={
            <MessageSquare
              className={`mt-0.5 h-5 w-5 shrink-0 ${enabled ? 'text-primary' : 'text-text-secondary'}`}
            />
          }
          title={t('Community-Hinweise im Chat')}
          savedAt={savedAt}
          toggle={{
            checked: enabled,
            onChange: () => void onToggle(),
            label: t('Community-Hinweise im Chat'),
            pending,
            disabled: pending || (enabled && !canDisable),
          }}
        />
      )}
    </SectionCard>
  );
}
