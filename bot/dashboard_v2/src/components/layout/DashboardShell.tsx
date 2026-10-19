import type { ReactNode } from 'react';
import { DashboardSidebar, type DashboardRoute } from '@/components/layout/DashboardSidebar';
import { isPublicTourPath } from '@/runtimeConfig';

export type { DashboardRoute };

function TourBanner() {
  return (
    <div
      role="status"
      className="rounded-2xl border border-warning/20 bg-warning/10 px-4 py-3 text-sm text-warning/90"
    >
      Das ist eine Demo. Du kannst dich umsehen, nichts wird gespeichert.{' '}
      <a className="font-semibold underline decoration-warning/40 underline-offset-2 hover:text-warning" href="/twitch/auth/login?next=%2Ftwitch%2Fdashboard">
        Zum echten Dashboard
      </a>
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
  const tourMode = typeof window !== 'undefined' && isPublicTourPath(window.location.pathname);
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
