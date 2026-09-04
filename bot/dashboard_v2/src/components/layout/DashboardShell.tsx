import type { ReactNode } from 'react';
import { DashboardSidebar, type DashboardRoute } from '@/components/layout/DashboardSidebar';

export type { DashboardRoute };

function BackgroundBlobs() {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      <div className="absolute -top-32 right-[-8rem] h-[28rem] w-[28rem] rounded-full bg-primary/22 blur-3xl" />
      <div className="absolute top-[24%] -left-28 h-[22rem] w-[22rem] rounded-full bg-accent/24 blur-3xl" />
      <div className="absolute bottom-[-8rem] left-[34%] h-[24rem] w-[24rem] rounded-full bg-success/20 blur-3xl" />
    </div>
  );
}

export function DashboardShell({
  activeRoute,
  demoMode = false,
  children,
}: {
  activeRoute: DashboardRoute;
  demoMode?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="internal-home-vibe relative min-h-screen px-3 py-4 md:px-6 md:py-6">
      <BackgroundBlobs />
      <div className="relative mx-auto max-w-[2200px]">
        {demoMode ? (
          <main className="min-w-0 space-y-4 md:space-y-5">{children}</main>
        ) : (
          <div className="grid gap-4 md:gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
            <DashboardSidebar activeRoute={activeRoute} />
            <main className="min-w-0 space-y-4 md:space-y-5">{children}</main>
          </div>
        )}
      </div>
    </div>
  );
}
