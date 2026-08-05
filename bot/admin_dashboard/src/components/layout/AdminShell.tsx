import { AlertTriangle } from 'lucide-react';
import { useState } from 'react';
import { Outlet } from 'react-router';
import { Sidebar } from '@/components/layout/Sidebar';
import { TopBar } from '@/components/layout/TopBar';
import { useRequireAdminAuth, toAuthErrorMessage } from '@/hooks/useAuth';

export function AdminShell() {
  const [collapsed, setCollapsed] = useState(false);
  const authQuery = useRequireAdminAuth();

  if (authQuery.isLoading) {
    return (
      <div className="flex min-h-screen items-center justify-center px-4">
        <div className="panel-card rounded-[2rem] px-8 py-10 text-center">
          <p className="text-sm uppercase tracking-[0.28em] text-text-secondary">Admin Auth</p>
          <h2 className="mt-3 text-2xl font-semibold text-white">Prüfe Discord-Session …</h2>
        </div>
      </div>
    );
  }

  if (authQuery.isError) {
    return (
      <div className="flex min-h-screen items-center justify-center px-4">
        <div className="panel-card max-w-lg rounded-[2rem] p-8">
          <AlertTriangle className="h-8 w-8 text-warning" />
          <h2 className="mt-4 text-2xl font-semibold text-white">Admin-Zugriff konnte nicht geprüft werden</h2>
          <p className="mt-3 text-sm leading-6 text-text-secondary">
            {toAuthErrorMessage(authQuery.error)}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="admin-shell flex">
      <Sidebar collapsed={collapsed} onToggle={() => setCollapsed((current) => !current)} />
      <div className="min-h-screen flex-1 px-4 py-4 md:px-6">
        <TopBar auth={authQuery.data} />
        <main className="mx-auto mt-4 max-w-[1600px]">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
