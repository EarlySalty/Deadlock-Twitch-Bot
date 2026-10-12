import type { ReactNode } from 'react';
import { DashboardSidebar, type DashboardRoute } from '@/components/layout/DashboardSidebar';

export type { DashboardRoute };

type DashboardShellNav =
  | { activeRoute: DashboardRoute; sidebar?: never }
  /**
   * Eigene Navigation statt der Partner-Sidebar (Server-Admin-Dashboard). Die
   * Partner-Sidebar samt Admin-Modus wird dann gar nicht gerendert.
   */
  | { sidebar: ReactNode; activeRoute?: never };

export function DashboardShell({
  activeRoute,
  demoMode = false,
  showSidebar = true,
  sidebar,
  children,
}: DashboardShellNav & {
  demoMode?: boolean;
  showSidebar?: boolean;
  children: ReactNode;
}) {
  const withSidebar = !demoMode && showSidebar;
  return (
    <div className="internal-home-vibe relative min-h-screen">
      <div className="relative px-3 py-4 md:px-5 md:py-5 lg:px-6">
        {withSidebar ? (
          <div className="grid gap-4 lg:grid-cols-[240px_minmax(0,1fr)] lg:gap-5">
            {activeRoute ? <DashboardSidebar activeRoute={activeRoute} /> : sidebar}
            <main className="min-w-0 space-y-4 md:space-y-5">{children}</main>
          </div>
        ) : (
          <main className="min-w-0 space-y-4 md:space-y-5">{children}</main>
        )}
      </div>
    </div>
  );
}
