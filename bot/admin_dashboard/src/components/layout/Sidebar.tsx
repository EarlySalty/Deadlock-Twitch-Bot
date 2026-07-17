import {
  Activity,
  AlertTriangle,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ClipboardList,
  CreditCard,
  Database,
  HandCoins,
  History,
  LayoutDashboard,
  Map,
  Megaphone,
  PieChart,
  MessageSquare,
  Power,
  Radio,
  ReceiptText,
  Search,
  ScrollText,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  Swords,
  Terminal,
  Users,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useEffect, useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';

interface SidebarProps {
  collapsed: boolean;
  onToggle: () => void;
}

interface NavigationItem {
  label: string;
  to: string;
  icon: LucideIcon;
  end?: boolean;
}

interface NavigationGroup {
  label: string;
  items: NavigationItem[];
}

const navigationGroups: NavigationGroup[] = [
  {
    label: 'Cockpit',
    items: [{ label: 'Home', to: '/', icon: LayoutDashboard, end: true }],
  },
  {
    label: 'Operations',
    items: [
      { label: 'System Health', to: '/operations/system', icon: Activity },
      { label: 'Scopes & OAuth', to: '/operations/scopes', icon: ShieldCheck },
      { label: 'EventSub', to: '/operations/eventsub', icon: Radio },
      { label: 'Database', to: '/operations/database', icon: Database },
      { label: 'DB Query', to: '/operations/query', icon: Terminal },
      { label: 'Error Logs', to: '/operations/errors', icon: AlertTriangle },
      { label: 'Bot Control', to: '/operations/bot', icon: Power },
    ],
  },
  {
    label: 'Community',
    items: [
      { label: 'Streamer', to: '/community/streamers', icon: Users },
      { label: 'Raids', to: '/community/raids', icon: Swords },
      { label: 'Markt-Dominanz', to: '/community/market', icon: PieChart },
      { label: 'Streamer-Research', to: '/community/research', icon: Search },
      { label: 'Engagement AI', to: '/community/engagement', icon: Sparkles },
      { label: 'Chat Actions', to: '/community/chat', icon: MessageSquare },
      { label: 'PLATZHALTER: <Globale Ban-Verwaltung>', to: '/community/global-bans', icon: ShieldAlert },
    ],
  },
  {
    label: 'Content & Comms',
    items: [
      { label: 'Announcements', to: '/content/announcements', icon: Megaphone },
      { label: 'Roadmap', to: '/content/roadmap', icon: Map },
      { label: 'Changelog', to: '/content/changelog', icon: ClipboardList },
      { label: 'Legal Pages', to: '/content/legal', icon: ScrollText },
    ],
  },
  {
    label: 'Money & Compliance',
    items: [
      { label: 'Subscriptions', to: '/money/subscriptions', icon: CreditCard },
      { label: 'Affiliates', to: '/money/affiliates', icon: HandCoins },
      { label: 'Gutschriften', to: '/money/gutschriften', icon: ReceiptText },
      { label: 'Audit Log', to: '/money/audit', icon: History },
    ],
  },
];

function isItemActive(pathname: string, to: string, end?: boolean) {
  if (end) {
    return pathname === to;
  }
  return pathname === to || pathname.startsWith(`${to}/`);
}

export function Sidebar({ collapsed, onToggle }: SidebarProps) {
  const location = useLocation();
  const activeGroup = navigationGroups.find((group) =>
    group.items.some((item) => isItemActive(location.pathname, item.to, item.end)),
  )?.label;
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(
      navigationGroups.map((group) => [
        group.label,
        group.items.some((item) => isItemActive(location.pathname, item.to, item.end)),
      ]),
    ),
  );

  useEffect(() => {
    if (!activeGroup) {
      return;
    }

    setOpenGroups((current) => {
      if (current[activeGroup]) {
        return current;
      }

      return { ...current, [activeGroup]: true };
    });
  }, [activeGroup]);

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
        <button className="rounded-2xl border border-white/10 bg-white/5 p-2 text-white/80" onClick={onToggle} type="button">
          {collapsed ? <ChevronRight className="h-4 w-4" /> : <ChevronLeft className="h-4 w-4" />}
        </button>
      </div>

      <nav className="mt-8 flex-1 space-y-4 overflow-y-auto pr-1">
        {navigationGroups.map((group) => {
          const groupOpen = collapsed ? true : openGroups[group.label];
          const groupIsActive = group.label === activeGroup;

          return (
            <div key={group.label} className="space-y-2">
              {collapsed ? null : (
                <button
                  aria-expanded={groupOpen}
                  className="flex w-full items-center justify-between px-2 text-left"
                  onClick={() => {
                    if (groupIsActive) {
                      return;
                    }

                    setOpenGroups((current) => ({ ...current, [group.label]: !current[group.label] }));
                  }}
                  type="button"
                >
                  <span className="text-[0.62rem] font-semibold uppercase tracking-[0.28em] text-text-secondary">
                    {group.label}
                  </span>
                  <ChevronDown
                    className={[
                      'h-4 w-4 text-text-secondary transition-transform',
                      groupOpen ? 'rotate-0' : '-rotate-90',
                    ].join(' ')}
                  />
                </button>
              )}

              {groupOpen ? (
                <div className="space-y-2">
                  {group.items.map((item) => (
                    <NavLink
                      key={item.to}
                      to={item.to}
                      end={item.end}
                      title={collapsed ? item.label : undefined}
                      className={({ isActive }) =>
                        [
                          'interactive-surface group flex items-center rounded-2xl border py-3',
                          collapsed ? 'justify-center px-2' : 'gap-3 px-3',
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
                </div>
              ) : null}
            </div>
          );
        })}
      </nav>
    </aside>
  );
}
