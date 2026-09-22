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
    <div className="relative min-h-screen bg-ui-root text-ui-text">
      <div className="relative px-3 py-4 md:px-5 md:py-5 lg:px-6">
        {withSidebar ? (
          <div className="grid gap-4 lg:grid-cols-[240px_minmax(0,1fr)] lg:gap-5">
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
