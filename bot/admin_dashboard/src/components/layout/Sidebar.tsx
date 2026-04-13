import {
  Activity,
  ChevronLeft,
  ChevronRight,
  CreditCard,
  LayoutDashboard,
  Settings,
  Users,
} from 'lucide-react';
import { NavLink } from 'react-router-dom';

interface SidebarProps {
  collapsed: boolean;
  onToggle: () => void;
}

const primaryItems = [
  { label: 'Dashboard', to: '/', icon: LayoutDashboard, end: true },
  { label: 'Streamer', to: '/streamers', icon: Users },
  { label: 'Monitoring', to: '/monitoring', icon: Activity },
  { label: 'Konfiguration', to: '/config', icon: Settings },
  { label: 'Billing', to: '/billing', icon: CreditCard },
];

export function Sidebar({ collapsed, onToggle }: SidebarProps) {
  return (
    <aside
      className={[
        'glass sticky top-0 flex h-screen flex-col border-r border-white/8 px-3 py-4 transition-all duration-200',
        collapsed ? 'w-[92px]' : 'w-[240px]',
      ].join(' ')}
    >
      <div className="flex items-center justify-between gap-2 px-2">
        <div className={collapsed ? 'hidden' : 'block'}>
          <p className="text-[0.68rem] font-semibold uppercase tracking-[0.24em] text-text-secondary">
            EarlySalty
          </p>
          <h1 className="display-font text-lg font-semibold text-white">Twitch Admin</h1>
        </div>
        <button onClick={onToggle} className="rounded-2xl border border-white/10 bg-white/5 p-2 text-white/80">
          {collapsed ? <ChevronRight className="h-4 w-4" /> : <ChevronLeft className="h-4 w-4" />}
        </button>
      </div>

      <nav className="mt-8 flex-1 space-y-2">
        {primaryItems.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.end}
            className={({ isActive }) =>
              [
                'group flex items-center gap-3 rounded-2xl border px-3 py-3 transition',
                isActive
                  ? 'border-primary/40 bg-primary/12 text-white'
                  : 'border-transparent bg-white/[0.03] text-text-secondary hover:border-white/10 hover:text-white',
              ].join(' ')
            }
          >
            <item.icon className="h-5 w-5 shrink-0" />
            <div className={collapsed ? 'hidden' : 'block'}>
              <p className="font-medium">{item.label}</p>
            </div>
          </NavLink>
        ))}
      </nav>

      <div className={collapsed ? 'hidden' : 'block'}>
        <div className="panel-card rounded-[1.6rem] p-4">
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-secondary">Host</p>
          <p className="mt-2 text-sm text-white">admin.deutsche-deadlock-community.de</p>
          <p className="mt-2 text-xs leading-5 text-text-secondary">
            Neue React-App unter <code>/twitch/admin</code> mit separater Admin-API.
          </p>
          <a
            href="/twitch/admin/legacy"
            className="mt-3 inline-flex w-full items-center justify-center rounded-2xl border border-white/10 bg-white/5 px-3 py-2 text-sm text-white transition hover:border-primary/40 hover:bg-primary/10"
          >
            Legacy Admin öffnen
          </a>
        </div>
      </div>
    </aside>
  );
}
