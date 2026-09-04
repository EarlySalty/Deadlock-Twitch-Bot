import type { ReactNode } from 'react';
import { DashboardSidebar, type DashboardRoute } from '@/components/layout/DashboardSidebar';

export type { DashboardRoute };

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
  const withSidebar = !demoMode && showSidebar;
  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <div className="relative mx-auto max-w-[2200px]">
        {withSidebar ? (
          <div className="grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
            <DashboardSidebar activeRoute={activeRoute} />
            <main className="min-w-0 space-y-4 md:space-y-5">{children}</main>
          </div>
        ) : (
          <main className="min-w-0 space-y-4 md:space-y-5">{children}</main>
        )}
      </div>
    </div>
  );
}
