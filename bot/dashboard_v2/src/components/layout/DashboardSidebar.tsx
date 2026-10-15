import { useAiFeatures } from '@/hooks/useAiFeatures';
import { useId, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { Rise } from '@/motion/Rise';
import { useDashboardProfile } from '@/hooks/useDashboardProfile';
import {
  PREVIEW_CHANGELOG_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_TITLE_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  analyticsTabHref,
} from '@/preview/routes';
import { useOnboarding } from '@/components/onboarding/onboardingState';
import { FeedbackBadge } from '@/components/feedback/FeedbackBadge';
import { useT } from '@/context/LanguageContext';
import {
  BarChart3,
  BookOpen,
  BadgeCheck,
  CircleHelp,
  FileText,
  Film,
  Home,
  Loader2,
  Menu,
  MonitorPlay,
  MessageSquare,
  Radio,
  RotateCcw,
  Settings,
  ShieldCheck,
  Sparkles,
  Trophy,
  type LucideIcon,
} from 'lucide-react';

export type DashboardRoute =
  | 'category'
  | 'home'
  | 'challenges'
  | 'analyse'
  | 'social'
  | 'uplink'
  | 'verwaltung'
  | 'overlay'
  | 'title'
  | 'pricing'
  | 'hilfe'
  | 'feedback';

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
      className={`flex items-center gap-3 rounded-xl px-3 py-2 text-sm font-semibold no-underline transition-colors whitespace-nowrap lg:whitespace-normal ${active ? activeClasses : inactiveClasses}`}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span className="min-w-0 lg:break-words">{label}</span>
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
  const aiFeatures = useAiFeatures();
  const {
    authStatus,
    displayName,
    avatarUrl,
    planName,
    adminEligible,
    adminMode,
    adminModeMutation,
    canAccessAnalyticsDashboard,
    profileReady,
  } = useDashboardProfile();
  const t = useT();
  const queryClient = useQueryClient();
  const onboarding = useOnboarding();
  const [avatarFailed, setAvatarFailed] = useState(false);
  const [navigationOpen, setNavigationOpen] = useState(false);
  const navigationId = useId();
  const compactNavigation = activeRoute === 'social';
  const shownAvatar = avatarFailed ? null : avatarUrl;
  const canSeeChallenges = Boolean(
    authStatus?.authenticated && authStatus.twitchUserId && (authStatus.isAdmin || authStatus.partnerStatus === 'active'),
  );

  const mainNavItems: SidebarNavItem[] = [
    ...(adminEligible && adminMode ? [{ href: '/twitch/analyse?view=category', label: t('Deadlock weltweit'), icon: BarChart3, active: activeRoute === 'category' }] : []),
    { href: PREVIEW_HOME_ROUTE, label: t('Home'), icon: Home, active: activeRoute === 'home' },
    ...(canSeeChallenges
      ? [{ href: '/twitch/challenges', label: t('Rangliste & Erfolge'), icon: Trophy, active: activeRoute === 'challenges' }]
      : []),
    ...(canAccessAnalyticsDashboard
      ? [
          {
            href: analyticsTabHref('overview'),
            label: t('Analyse'),
            icon: BarChart3,
            active: activeRoute === 'analyse',
          },
        ]
      : []),
    {
      href: '/twitch/social-media',
      label: t('Social-Media-Manager'),
      icon: Film,
      active: activeRoute === 'social',
    },
    { href: PREVIEW_UPLINK_ROUTE, label: t('Uplink'), icon: Radio, active: activeRoute === 'uplink' },
  ];
  const toolNavItems: SidebarNavItem[] = [
    {
      href: PREVIEW_VERWALTUNG_ROUTE,
      label: t('Verwaltung'),
      icon: Settings,
      active: activeRoute === 'verwaltung',
    },
    {
      href: PREVIEW_OVERLAY_ROUTE,
      label: t('Stream-Overlay'),
      icon: MonitorPlay,
      active: activeRoute === 'overlay',
    },
    ...(aiFeatures ? [{
      href: PREVIEW_TITLE_ROUTE,
      label: t('Titel-Studio'),
      icon: Sparkles,
      active: activeRoute === 'title',
    }] : []),
    {
      href: PREVIEW_PRICING_ROUTE,
      label: t('Plan: {plan}', { plan: planName }),
      icon: BadgeCheck,
      active: activeRoute === 'pricing',
    },
    { href: PREVIEW_CHANGELOG_ROUTE, label: t('Changelog'), icon: FileText },
  ];

  return (
    <Rise as="aside" className="min-w-0 self-start lg:sticky lg:top-5">
      <div className="panel-card card-glow space-y-4 rounded-2xl p-4">
        <div className="flex items-center gap-3">
          {shownAvatar ? (
            <img
              src={shownAvatar}
              alt=""
              onError={() => setAvatarFailed(true)}
              className="sidebar-avatar-glow h-10 w-10 shrink-0 rounded-full border border-border object-cover"
            />
          ) : profileReady ? (
            <div className="gradient-accent sidebar-avatar-glow flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-sm font-bold">
              {displayName?.[0]?.toUpperCase() ?? '?'}
            </div>
          ) : (
            <div
              aria-hidden
              className="sidebar-avatar-skeleton h-10 w-10 shrink-0 animate-pulse rounded-full border border-border bg-white/5"
            />
          )}
          <div data-tour-id="tour-plan" className="min-w-0">
            <div className="truncate text-sm font-semibold text-white">{displayName}</div>
            <div className="mt-1 inline-flex max-w-full items-center rounded-full border border-accent/30 bg-accent/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-accent">
              {planName}
            </div>
          </div>
        </div>

        {compactNavigation && <button
          type="button"
          aria-expanded={navigationOpen}
          aria-controls={navigationId}
          onClick={() => setNavigationOpen(open => !open)}
          className="flex min-h-11 w-full items-center justify-between rounded-xl border border-border px-3 py-2 text-sm text-text-secondary lg:hidden"
        ><span>Navigation</span><Menu className="h-4 w-4" aria-hidden /></button>}
        <div id={navigationId} className={`space-y-4 ${compactNavigation && !navigationOpen ? 'hidden lg:block' : ''}`}>
        <div className="border-t border-border" />

        <div className="space-y-2">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            {t('Main')}
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
            {t('Tools')}
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
                {t('Admin')}
              </div>
              <button
                type="button"
                onClick={() =>
                  (!document.querySelector('[data-unsaved="true"]') || window.confirm(t('Ungespeicherte Änderungen verwerfen und die Ansicht wechseln?'))) && adminModeMutation.mutate(!adminMode, {
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
                {adminMode ? t('Admin-Modus beenden') : t('Admin-Modus aktivieren')}
              </button>
              <p className="text-[11px] leading-snug text-text-secondary">
                {adminMode
                  ? t('Voller Zugriff aktiv, nicht die echte Nutzer-Ansicht.')
                  : t('Du siehst das Dashboard wie ein normaler Nutzer.')}
              </p>
            </div>
          </>
        ) : null}

        <div className="border-t border-border" />
        <div data-tour-id="tour-help" className="space-y-2">
          <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-text-secondary">
            {t('Hilfe')}
          </div>
          <a
            href="/twitch/hilfe"
            className="flex items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-2 text-sm font-medium text-text-secondary transition-colors hover:border-border-hover hover:text-white"
          >
            <BookOpen className="h-4 w-4" />
            {t('Hilfe & Einrichtung')}
          </a>
          {onboarding.enabled && <button
            type="button"
            data-onboarding-restart
            disabled={onboarding.pending}
            onClick={() => {
              void onboarding.open('bookmark');
            }}
            className="flex w-full items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-2 text-sm font-medium text-text-secondary transition-colors hover:border-border-hover hover:text-white"
          >
            <RotateCcw className="h-4 w-4" />
            {t('Tour erneut zeigen')}
          </button>}
          <a href="/twitch/feedback" aria-current={activeRoute === 'feedback' ? 'page' : undefined} className="flex min-h-11 items-center gap-2 rounded-xl border border-border bg-background/60 px-3 py-2 text-sm font-medium text-text-secondary hover:text-white"><MessageSquare className="h-4 w-4 shrink-0" /><span className="flex-1">{t('Kritik & Wünsche')}</span><FeedbackBadge isAdmin={adminMode} /></a>
          <a href="/twitch/faq" className="flex items-center gap-2 px-3 py-2 text-sm text-text-secondary hover:text-white"><CircleHelp className="h-4 w-4 shrink-0" />{t('Häufige Fragen')}</a>
        </div>
        </div>
      </div>
    </Rise>
  );
}
