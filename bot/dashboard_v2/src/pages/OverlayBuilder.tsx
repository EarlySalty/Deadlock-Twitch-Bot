import { Rise } from '../motion/Rise';
import { useQuery } from '@tanstack/react-query';
import { fetchInternalHome } from '@/api/home';
import { OverlayBuilderSection } from '@/components/verwaltung/OverlayBuilderSection';
import { useAuthStatus } from '@/hooks/useAnalytics';
import { PREVIEW_VERWALTUNG_ROUTE } from '@/preview/routes';
import { ArrowRight, Loader2 } from 'lucide-react';

export function OverlayBuilderPage() {
  const { isLoading: loadingAuth } = useAuthStatus();

  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: ['internal-home', null],
    queryFn: () => fetchInternalHome(null),
    staleTime: Number.POSITIVE_INFINITY,
    enabled: !loadingAuth,
  });

  if (isLoading || loadingAuth) {
    return (
      <div className="panel-card rounded-2xl p-6 md:p-8">
        <div className="flex items-center gap-3 text-text-secondary">
          <Loader2 className="h-5 w-5 animate-spin text-primary" />
          <span>Konto wird geladen ...</span>
        </div>
      </div>
    );
  }

  if (isError) {
    const errorMessage = error instanceof Error ? error.message : 'Unbekannter Fehler';
    return (
      <div className="panel-card rounded-2xl p-6 md:p-8">
        <h2 className="text-xl font-bold text-white">Konto-Daten nicht verfügbar</h2>
        <p className="mt-1 text-sm text-text-secondary">{errorMessage}</p>
        <button
          onClick={() => void refetch()}
          className="mt-4 inline-flex items-center gap-2 rounded-lg border border-border bg-card px-4 py-2 text-sm font-semibold text-white transition-colors hover:border-border-hover hover:bg-card-hover"
        >
          <ArrowRight className="h-4 w-4" />
          Erneut laden
        </button>
      </div>
    );
  }

  const home = data ?? {};
  const twitchLogin = home.twitchLogin?.trim() || '';

  return (
    <>
      <Rise as="section" className="panel-card rounded-2xl p-5 md:p-6">
        <div className="mb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-primary">
          Stream-Overlay
        </div>
        <h1 className="display-font text-2xl font-extrabold text-white">Overlay einrichten</h1>
        <p className="mt-2 max-w-2xl text-sm text-text-secondary">
          Dein Browser-Overlay für OBS, mit Adresse zum Einbinden und den Bausteinen deiner Wahl.
        </p>
      </Rise>

      <Rise>
        <a
          href={PREVIEW_VERWALTUNG_ROUTE}
          className="inline-flex items-center gap-2 text-sm text-text-secondary transition-colors hover:text-white"
        >
          ← Zurück zur Verwaltung
        </a>
      </Rise>

      <OverlayBuilderSection login={twitchLogin} />
    </>
  );
}
