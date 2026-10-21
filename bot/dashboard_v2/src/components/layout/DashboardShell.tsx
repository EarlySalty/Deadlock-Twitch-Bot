import { useEffect, useState, type ReactNode } from 'react';
import { DashboardSidebar, type DashboardRoute } from '@/components/layout/DashboardSidebar';
import { useT } from '@/context/LanguageContext';
import { TOUR_BLOCKED_EVENT } from '@/tourGuard';
import { onPublicTour } from '@/publicTour';

export type { DashboardRoute };

function TourBanner() {
  const t = useT();
  const [blocked, setBlocked] = useState(false);
  useEffect(() => {
    let timer: number | undefined;
    const show = () => {
      setBlocked(true);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setBlocked(false), 4000);
    };
    window.addEventListener(TOUR_BLOCKED_EVENT, show);
    return () => {
      window.removeEventListener(TOUR_BLOCKED_EVENT, show);
      window.clearTimeout(timer);
    };
  }, []);
  return (
    <div
      role="status"
      className="rounded-2xl border border-warning/20 bg-warning/10 px-4 py-3 text-sm text-warning/90"
    >
      {t('Das ist eine Demo. Du kannst dich umsehen, nichts wird gespeichert.')}{' '}
      <a
        data-tour-exit
        target="_self"
        className="font-semibold underline decoration-warning/40 underline-offset-2 hover:text-warning"
        href="/twitch/dashboard"
      >
        {t('Zum echten Dashboard')}
      </a>
      {blocked ? <p className="mt-2 font-semibold text-warning">{t('In der Demo kannst du nichts ändern.')}</p> : null}
    </div>
  );
}

export function DashboardShell({
  activeRoute,
  demoMode = false,
  showSidebar = true,
  children,
}: {
  activeRoute: DashboardRoute;
  demoMode?: boolean;
  showSidebar?: boolean;
  children: ReactNode;
}) {
  const tourMode = onPublicTour();
  const withSidebar = showSidebar && (!demoMode || tourMode);
  return (
    <div className="internal-home-vibe relative min-h-screen">
      <div className="relative px-3 py-4 md:px-5 md:py-5 lg:px-6">
        {withSidebar ? (
          <div className="grid gap-4 lg:grid-cols-[240px_minmax(0,1fr)] lg:gap-5">
            <DashboardSidebar activeRoute={activeRoute} />
            <main className="min-w-0 space-y-4 md:space-y-5">
              {tourMode ? <TourBanner /> : null}
              {children}
            </main>
          </div>
        ) : (
          <main className="min-w-0 space-y-4 md:space-y-5">
            {tourMode ? <TourBanner /> : null}
            {children}
          </main>
        )}
      </div>
    </div>
  );
}
