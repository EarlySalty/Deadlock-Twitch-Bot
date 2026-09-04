import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { Rise } from '@/motion/Rise';
import { useDashboardProfile } from '@/hooks/useDashboardProfile';
import {
  PREVIEW_CHANGELOG_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  analyticsTabHref,
} from '@/preview/routes';
import { resetWelcomeTour } from '@/components/onboarding/WelcomeTour';
import {
  BarChart3,
  BookOpen,
  FileText,
  Film,
  Home,
  Loader2,
  MonitorPlay,
  Radio,
  RotateCcw,
  Settings,
  ShieldCheck,
  Sparkles,
  type LucideIcon,
} from 'lucide-react';

export type DashboardRoute =
  | 'home'
  | 'analyse'
  | 'social'
  | 'uplink'
  | 'verwaltung'
  | 'overlay'
  | 'pricing';

function SidebarLink({
  href,
  icon: Icon,
  label,
  active = false,
}: {
  href: string;
  icon: LucideIcon;
  label: string;
  active?: boolean;
}) {
  const activeClasses =
    'border border-primary/25 bg-primary/10 text-primary lg:rounded-l-none lg:border-y-0 lg:border-r-0 lg:border-t-0 lg:border-l-2 lg:border-primary lg:pl-2.5';
  const inactiveClasses =
    'border border-transparent text-text-secondary hover:bg-white/5 hover:text-white';

  return (
    <a
      href={href}
      aria-current={active ? 'page' : undefined}
      className={`flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-semibold no-underline transition-colors whitespace-nowrap ${active ? activeClasses : inactiveClasses}`}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span>{label}</span>
    </a>
  );
}

interface SidebarNavItem {
  href: string;
  label: string;
  icon: LucideIcon;
  active?: boolean;
}

export function DashboardSidebar({ activeRoute }: { activeRoute: DashboardRoute }) {
  const {
    displayName,
    avatarUrl,
    planName,
    adminEligible,
    adminMode,
    adminModeMutation,
    canAccessAnalyticsDashboard,
  } = useDashboardProfile();
  const queryClient = useQueryClient();
  const [avatarFailed, setAvatarFailed] = useState(false);
  const shownAvatar = avatarFailed ? null : avatarUrl;

  const mainNavItems: SidebarNavItem[] = [
    { href: PREVIEW_HOME_ROUTE, label: 'Home', icon: Home, active: activeRoute === 'home' },
    ...(canAccessAnalyticsDashboard
      ? [
          {
            href: analyticsTabHref('overview'),
            label: 'Analyse',
            icon: BarChart3,
            active: activeRoute === 'analyse',
          },
        ]
      : []),
    {
      href: '/social-media-admin',
      label: 'Social Media Dashboard',
      icon: Film,
      active: activeRoute === 'social',
    },
    { href: PREVIEW_UPLINK_ROUTE, label: 'Uplink', icon: Radio, active: activeRoute === 'uplink' },
  ];
  const toolNavItems: SidebarNavItem[] = [
    {
      href: PREVIEW_VERWALTUNG_ROUTE,
      label: 'Verwaltung',
      icon: Settings,
      active: activeRoute === 'verwaltung',
    },
    {
      href: PREVIEW_OVERLAY_ROUTE,
      label: 'Stream-Overlay',
      icon: MonitorPlay,
      active: activeRoute === 'overlay',
    },
    {
      href: PREVIEW_PRICING_ROUTE,
      label: `Plan: ${planName}`,
      icon: Sparkles,
      active: activeRoute === 'pricing',
    },
    { href: PREVIEW_CHANGELOG_ROUTE, label: 'Changelog', icon: FileText },
  ];

  return (
    <Rise as="aside" className="panel-card card-glow self-start rounded-2xl p-4 lg:sticky lg:top-4">
      <div className="space-y-4">
        <div className="flex items-center gap-3">
          {shownAvatar ? (
            <img
              src={shownAvatar}
              alt=""
              onError={() => setAvatarFailed(true)}
              className="sidebar-avatar-glow h-10 w-10 shrink-0 rounded-full border border-border object-cover"
            />
          ) : (
            <div className="gradient-accent sidebar-avatar-glow flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-sm font-bold">
              {displayName?.[0]?.toUpperCase() ?? '?'}
            </div>
          )}
          <div data-tour-id="tour-plan" className="min-w-0">
            <div className="truncate text-sm font-semibold text-white">{displayName}</div>
            <div className="mt-1 inline-flex max-w-full items-center rounded-full border border-accent/30 bg-accent/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-accent">
              {planName}
            </div>
          </div>
        </div>

        <div className="border-t border-border" />

        <div className="space-y-2">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            Main
          </div>
          <nav
            data-tour-id="tour-nav"
            className="flex gap-2 overflow-x-auto pb-1 lg:block lg:space-y-1 lg:overflow-visible lg:pb-0"
          >
            {mainNavItems.map((item, index) => (
              <motion.div
                key={item.href}
                initial={{ opacity: 0, x: -6 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.22, delay: Math.min(0.05 + index * 0.04, 0.24) }}
              >
                <SidebarLink href={item.href} icon={item.icon} label={item.label} active={item.active} />
              </motion.div>
            ))}
          </nav>
        </div>

        <div className="space-y-2">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            Tools
          </div>
          <div className="flex gap-2 overflow-x-auto pb-1 lg:block lg:space-y-1 lg:overflow-visible lg:pb-0">
            {toolNavItems.map((item, index) => (
              <motion.div
                key={item.href}
                initial={{ opacity: 0, x: -6 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.22, delay: Math.min(0.1 + index * 0.04, 0.24) }}
              >
                <SidebarLink href={item.href} icon={item.icon} label={item.label} active={item.active} />
              </motion.div>
            ))}
          </div>
        </div>

        {adminEligible ? (
          <>
            <div className="border-t border-border" />
            <div className="space-y-2">
              <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
                Admin
              </div>
              <button
                type="button"
                onClick={() =>
                  adminModeMutation.mutate(!adminMode, {
                    onSuccess: () =>
                      queryClient.invalidateQueries({
                        predicate: (query) => {
                          const key = query.queryKey;
                          if (
                            Array.isArray(key) &&
                            key[0] === 'internal-home' &&
                            key[1] != null
                          ) {
                            return false;
                          }
                          return true;
                        },
                      }),
                  })
                }
                disabled={adminModeMutation.isPending}
                aria-pressed={adminMode}
                className={`flex w-full items-center gap-2 rounded-xl border px-3 py-2 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${
                  adminMode
                    ? 'border-warning/40 bg-warning/10 text-warning hover:border-warning/60'
                    : 'border-border bg-background/60 text-text-secondary hover:border-border-hover hover:text-white'
                }`}
              >
                {adminModeMutation.isPending ? (
                  <Loader2 className="h-4 w-4 shrink-0 animate-spin" />
                ) : (
                  <ShieldCheck className="h-4 w-4 shrink-0" />
                )}
                {adminMode ? 'Admin-Modus beenden' : 'Admin-Modus aktivieren'}
              </button>
              <p className="text-[11px] leading-snug text-text-secondary">
                {adminMode
                  ? 'Voller Zugriff aktiv, nicht die echte Nutzer-Ansicht.'
                  : 'Du siehst das Dashboard wie ein normaler Nutzer.'}
              </p>
            </div>
          </>
        ) : null}

        <div className="border-t border-border" />
        <div data-tour-id="tour-help" className="space-y-2">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            Hilfe
          </div>
          <a
            href="/twitch/faq"
            className="flex items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-2 text-sm font-medium text-text-secondary transition-colors hover:border-border-hover hover:text-white"
          >
            <BookOpen className="h-4 w-4" />
            FAQ &amp; Hilfe
          </a>
          <button
            type="button"
            onClick={() => {
              resetWelcomeTour();
              window.location.reload();
            }}
            className="flex w-full items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-2 text-sm font-medium text-text-secondary transition-colors hover:border-border-hover hover:text-white"
          >
            <RotateCcw className="h-4 w-4" />
            Tour neu starten
          </button>
        </div>
      </div>
    </Rise>
  );
}
