import { useState, useEffect, useRef, useSyncExternalStore, lazy, Suspense, Component, type ReactNode, type ErrorInfo } from 'react';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/Header';
import { TabNavigation, type TabId } from '@/components/layout/TabNavigation';
import { fetchTitleStreamers } from '@/api/title';
import { resolveTabParam } from '@/tabAliases';
import { DashboardShell } from '@/components/layout/DashboardShell';
import { OnboardingProvider } from '@/components/onboarding/OnboardingContext';
import { EinrichtungCard } from '@/components/onboarding/EinrichtungCard';
import { FeedbackBox } from '@/components/feedback/FeedbackBox';
import { PlanProvider } from '@/context/PlanContext';
import { LanguageProvider, useT } from '@/context/LanguageContext';
import { DashboardAssistent } from '@/components/assistent/DashboardAssistent';
import { TrialBanner } from '@/components/banners/TrialBanner';
import { TrialExpiryModal } from '@/components/modals/TrialExpiryModal';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import { usePlan } from '@/context/PlanContext';
import type { TimeRange } from '@/types/analytics';
import { parseDaysParam, streamerAusUrlErlaubt } from '@/utils/zeitraum';
import {
  PREVIEW_ANALYTICS_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_TITLE_ROUTE,
  PREVIEW_UPLINK_ROUTE,
  PREVIEW_SOCIAL_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
  isPreviewModeEnabled,
} from '@/preview/routes';
import { shouldRetryApiQuery } from '@/api/httpError';
import {
  dashboardRuntimeConfig,
  hasDemoRuntimeConfig,
  resolveEffectiveDemoMode,
} from '@/runtimeConfig';
import { AlertTriangle } from 'lucide-react';
import { pageLoaders } from './pageLoaders';
import { navigationSnapshot, subscribeNavigation } from './navigation';
import { useDashboardProfile } from './hooks/useDashboardProfile';

const Overview = lazy(() => pageLoaders.Overview().then(module => ({ default: module.Overview })));
const Tagesform = lazy(() => pageLoaders.Tagesform().then(module => ({ default: module.Tagesform })));
const Sessions = lazy(() => pageLoaders.Sessions().then(module => ({ default: module.Sessions })));
const SocialMediaManager = lazy(() => pageLoaders.SocialMediaManager().then(module => ({ default: module.SocialMediaManager })));
const Monetization = lazy(() => pageLoaders.Monetization().then(module => ({ default: module.Monetization })));
const Publikum = lazy(() => pageLoaders.Publikum().then(module => ({ default: module.Publikum })));
const Wachstum = lazy(() => pageLoaders.Wachstum().then(module => ({ default: module.Wachstum })));
const Planung = lazy(() => pageLoaders.Planung().then(module => ({ default: module.Planung })));
const Community = lazy(() => pageLoaders.Community().then(module => ({ default: module.Community })));
const Challenges = lazy(() => pageLoaders.Challenges().then(module => ({ default: module.Challenges })));
const PartnerProfile = lazy(() => pageLoaders.PartnerProfile().then(module => ({ default: module.PartnerProfile })));
const CategoryCollector = lazy(() => pageLoaders.CategoryCollector().then(module => ({ default: module.CategoryCollector })));
const TitleGenerator = lazy(() => pageLoaders.TitleGenerator().then(module => ({ default: module.TitleGenerator })));
const WasTun = lazy(() => pageLoaders.WasTun().then(module => ({ default: module.WasTun })));
const SessionDetail = lazy(() => pageLoaders.SessionDetail().then(module => ({ default: module.SessionDetail })));
const InternalHomeLanding = lazy(() => pageLoaders.InternalHomeLanding().then(module => ({ default: module.InternalHomeLanding })));
const UplinkPage = lazy(() => pageLoaders.UplinkPage().then(module => ({ default: module.UplinkPage })));
const VerwaltungPage = lazy(() => pageLoaders.VerwaltungPage().then(module => ({ default: module.VerwaltungPage })));
const OverlayBuilderPage = lazy(() => pageLoaders.OverlayBuilderPage().then(module => ({ default: module.OverlayBuilderPage })));
const Pricing = lazy(pageLoaders.Pricing);
const FeedbackPage = lazy(() => pageLoaders.FeedbackPage().then(module => ({ default: module.FeedbackPage })));

// Error Boundary to prevent white screen on crashes
interface ErrorBoundaryProps {
  children: ReactNode;
}
interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Dashboard Error:', error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      return (
        <ErrorFallback
          message={this.state.error?.message ?? null}
          onRetry={() => this.setState({ hasError: false, error: null })}
        />
      );
    }
    return this.props.children;
  }
}

/**
 * Eigene Komponente, weil die Fehlergrenze eine Klasse ist und keine Hooks
 * benutzen kann. Der Provider liegt darueber, die Sprache gilt also auch im
 * Fehlerfall.
 */
function ErrorFallback({ message, onRetry }: { message: string | null; onRetry: () => void }) {
  const t = useT();
  return (
    <div className="min-h-screen bg-bg flex items-center justify-center p-8">
      <div className="panel-card rounded-2xl p-8 max-w-lg text-center">
        <AlertTriangle className="w-12 h-12 text-warning mx-auto mb-4" />
        <h2 className="text-xl font-bold text-white mb-2">{t('Dashboard-Fehler')}</h2>
        <p className="text-text-secondary mb-4">
          {message || t('Ein unerwarteter Fehler ist aufgetreten.')}
        </p>
        <button
          onClick={onRetry}
          className="px-4 py-2 bg-primary text-[#0D0806] rounded-lg hover:bg-primary-hover transition-colors"
        >
          {t('Erneut versuchen')}
        </button>
      </div>
    </div>
  );
}

// Create QueryClient
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: shouldRetryApiQuery,
      refetchOnWindowFocus: false,
      gcTime: 30 * 60 * 1000,
      staleTime: 60 * 1000,
    },
  },
});

function SessionBootstrap() {
  useDashboardProfile();
  return null;
}

function RouteSkeleton() {
  return (
    <div className="space-y-4" aria-busy="true">
      <div className="panel-card rounded-2xl p-6"><div className="h-8 w-48 max-w-full animate-pulse rounded-lg bg-white/5" /></div>
      <div className="grid gap-4 md:grid-cols-3">
        {[0, 1, 2].map(key => <div key={key} className="panel-card h-48 animate-pulse rounded-2xl bg-white/5" />)}
      </div>
    </div>
  );
}

function normalizePathname(pathname: string): string {
  const normalized = pathname.replace(/\/+$/, '');
  return normalized || '/';
}

function InternalHome() {
  return <InternalHomeLanding />;
}

/**
 * Wählt zwischen Tagesform (Free) und Overview (Paid/Admin/Demo).
 * Muss innerhalb von PlanProvider gerendert werden.
 */
interface OverviewOrTagesformProps {
  streamer: string | null;
  days: import('@/types/analytics').TimeRange;
  onSessionClick: (sessionId: number) => void;
}

function OverviewOrTagesform({ streamer, days, onSessionClick }: OverviewOrTagesformProps) {
  const { hasFullAccess, hasEntitlement } = usePlan();
  const hasPaidAnalytics = hasFullAccess || hasEntitlement('analytics');

  if (hasPaidAnalytics) {
    return <Overview streamer={streamer} days={days} onSessionClick={onSessionClick} />;
  }
  return <Tagesform streamer={streamer} days={days} onSessionClick={onSessionClick} />;
}

function AnalyticsDashboard() {
  const t = useT();
  const { data: authStatus } = useAuthStatus();
  const [streamer, setStreamer] = useState<string | null>(() => {
    const requested = new URLSearchParams(window.location.search).get('streamer')?.trim().toLowerCase();
    const demo = resolveEffectiveDemoMode({ pathname: window.location.pathname, runtimeConfig: dashboardRuntimeConfig });
    if (requested && streamerAusUrlErlaubt(requested, demo, dashboardRuntimeConfig.allowedDemoProfiles)) return requested;
    return authStatus?.twitchLogin ?? authStatus?.adminDefaultStreamer ?? (demo ? dashboardRuntimeConfig.defaultDemoProfile : null);
  });
  const [days, setDays] = useState<TimeRange>(() => parseDaysParam(new URLSearchParams(window.location.search).get('days')));
  const [activeTab, setActiveTab] = useState<TabId | 'session-detail'>('overview');
  const [selectedSessionId, setSelectedSessionId] = useState<number | null>(null);
  const [pendingSub, setPendingSub] = useState<string | null>(null);
  const [pendingMode, setPendingMode] = useState<string | null>(null);
  const hasExplicitTab = useRef(
    Boolean(resolveTabParam(new URLSearchParams(window.location.search).get('tab'))),
  );

  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const isDemoShell = resolveEffectiveDemoMode({
    pathname: window.location.pathname,
    runtimeConfig: dashboardRuntimeConfig,
  });
  const isDemoMode = resolveEffectiveDemoMode({
    pathname: window.location.pathname,
    runtimeConfig: dashboardRuntimeConfig,
  });

  const hasAutoSetStreamer = useRef(false);
  const urlParsed = useRef(false);
  const pendingUrlStreamer = useRef<string | null>(null);

  useEffect(() => {
    if (urlParsed.current) {
      return;
    }
    const params = new URLSearchParams(window.location.search);
    const urlStreamer = params.get('streamer');
    const urlDays = params.get('days');

    if (urlStreamer) {
      pendingUrlStreamer.current = urlStreamer.trim().toLowerCase();
    }
    if (urlDays) {
      setDays(parseDaysParam(urlDays));
    }
    const resolved = resolveTabParam(params.get('tab'));
    if (resolved) {
      hasExplicitTab.current = true;
      setActiveTab(resolved.tab);
      setPendingSub(params.get('sub') ?? resolved.sub ?? null);
      setPendingMode(params.get('mode') ?? resolved.mode ?? null);
    }
    urlParsed.current = true;
  }, []);

  useEffect(() => {
    const wunschStreamer = pendingUrlStreamer.current;
    if (!wunschStreamer || hasAutoSetStreamer.current) {
      return;
    }
    if (streamerAusUrlErlaubt(wunschStreamer, isDemoShell, dashboardRuntimeConfig.allowedDemoProfiles)) {
      setStreamer(wunschStreamer);
      hasAutoSetStreamer.current = true;
    }
    pendingUrlStreamer.current = null;
  }, [isDemoShell]);

  useEffect(() => {
    if (pendingUrlStreamer.current) {
      return;
    }
    const fallbackStreamer =
      authStatus?.twitchLogin ??
      authStatus?.adminDefaultStreamer ??
      (isDemoShell ? dashboardRuntimeConfig.defaultDemoProfile : null);
    if (!hasAutoSetStreamer.current && fallbackStreamer) {
      setStreamer(fallbackStreamer);
      hasAutoSetStreamer.current = true;
    }
  }, [authStatus, isDemoShell]);

  useEffect(() => {
    if (!urlParsed.current) {
      return;
    }
    const params = new URLSearchParams(window.location.search);

    if (streamer) {
      params.set('streamer', streamer);
    } else {
      params.delete('streamer');
    }
    params.set('days', String(days));
    if (activeTab !== 'session-detail') {
      params.set('tab', activeTab);
    }

    const newUrl = `${window.location.pathname}?${params.toString()}`;
    window.history.replaceState({}, '', newUrl);
  }, [streamer, days, activeTab]);

  const handleSessionClick = (sessionId: number) => {
    setSelectedSessionId(sessionId);
    setActiveTab('session-detail');
  };

  const handleTabChange = (tab: TabId) => {
    hasExplicitTab.current = true;
    setActiveTab(tab);
    setPendingSub(null);
    setPendingMode(null);
  };

  return (
    <DashboardShell activeRoute="analyse" demoMode={isDemoMode}>
        <PlanProvider
          plan={authStatus?.plan ?? null}
          isAdmin={authStatus?.isAdmin ?? false}
          isLocalhost={authStatus?.isLocalhost ?? false}
          isDemoMode={isDemoMode}
        >
          <TrialExpiryModal />
          <TrialBanner />

          <Header
            streamer={streamer}
            streamers={streamers}
            days={days}
            onStreamerChange={setStreamer}
            onDaysChange={setDays}
            isLoading={loadingStreamers}
            canViewAllStreamers={authStatus?.permissions?.viewAllStreamers || false}
            isDemoMode={isDemoMode}
          />

          {isDemoMode && (
            <div className="mb-4 rounded-2xl border border-warning/20 bg-warning/10 px-4 py-3 text-sm text-warning/90">
              {t(
                'Demo-Daten aus einem statischen Snapshot. Profilwechsel und Analysen laufen ausschließlich über den Demo-Namespace.',
              )}
            </div>
          )}

          {activeTab !== 'session-detail' && (
            <TabNavigation activeTab={activeTab as TabId} onTabChange={handleTabChange} />
          )}

          {/* Tab Content */}
          {activeTab === 'overview' && (
            <OverviewOrTagesform
              streamer={streamer}
              days={days}
              onSessionClick={handleSessionClick}
            />
          )}

          {activeTab === 'streams' && (
            <Sessions streamer={streamer || ''} days={days} onSessionClick={handleSessionClick} />
          )}

          {activeTab === 'audience' && (
            <Publikum streamer={streamer} days={days} initialSub={pendingSub ?? undefined} />
          )}

          {activeTab === 'growth' && (
            <Wachstum
              streamer={streamer}
              days={days}
              initialSub={pendingSub ?? undefined}
              onStreamerSelect={setStreamer}
              onNavigate={handleTabChange}
            />
          )}

          {activeTab === 'planning' && (
            <Planung streamer={streamer} days={days} initialSub={pendingSub ?? undefined} />
          )}

          {activeTab === 'profile' && <PartnerProfile key={streamer || 'own'} streamer={streamer ?? undefined} />}

          {activeTab === 'community' && (
            <Community streamer={streamer} days={days} />
          )}

          {activeTab === 'coaching' && (
            <WasTun streamer={streamer} days={days} initialMode={pendingMode ?? undefined} />
          )}

          {activeTab === 'monetization' && (
            <Monetization streamer={streamer} days={days} />
          )}

          {activeTab === 'session-detail' && selectedSessionId && (
            <SessionDetail
              sessionId={selectedSessionId}
              streamer={streamer || ''}
              onBack={() => {
                setSelectedSessionId(null);
                setActiveTab('streams');
              }}
            />
          )}
        </PlanProvider>
    </DashboardShell>
  );
}

function TitleGeneratorRoute() {
  const t = useT();
  const { data: authStatus } = useAuthStatus();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const ownId = authStatus?.twitchUserId ?? null;
  const streamer = authStatus?.isAdmin ? selectedId ?? ownId : ownId;
  const directory = useQuery({ queryKey: ['title-streamer-directory'], queryFn: fetchTitleStreamers, enabled: Boolean(authStatus?.isAdmin) });
  return (
    <DashboardShell activeRoute="title">
      {authStatus?.isAdmin && <div className="mx-auto max-w-4xl pt-4">
        <label htmlFor="title-streamer" className="text-sm text-text-secondary">{t("Twitch-Kanal")}</label>
        <select id="title-streamer" value={streamer ?? ''} onChange={(event) => setSelectedId(event.target.value)} className="ml-3 rounded-lg border border-border bg-card px-3 py-2 text-white">
          {ownId ? <option value={ownId}>{t("Mein Kanal")}</option> : <option value="" disabled>{t("Kanal auswählen")}</option>}
          {directory.data?.items.filter((item) => item.twitchUserId && item.twitchUserId !== ownId).map((item) => <option key={item.twitchUserId} value={item.twitchUserId!}>{item.login}</option>)}
        </select>
        {directory.isError && <p role="status" className="mt-2 text-sm text-error">{t("Die Kanalliste ist gerade nicht verfügbar.")}</p>}
      </div>}
      <TitleGenerator key={streamer} streamer={streamer} />
    </DashboardShell>
  );
}

function PricingRoute() {
  const { data: authStatus, isLoading: loadingAuth } = useAuthStatus();
  const authenticated = !loadingAuth && authStatus?.authenticated === true;
  return (
    <DashboardShell activeRoute="pricing" showSidebar={authenticated}>
      <Pricing />
    </DashboardShell>
  );
}

export default function App() {
  const location = useSyncExternalStore(subscribeNavigation, navigationSnapshot);
  const path = normalizePathname(window.location.pathname);
  const isInternalHomeRoute = path === PREVIEW_HOME_ROUTE || path === '/twitch/onboarding';
  const isHelpRoute = path === '/twitch/hilfe';
  const isFeedbackRoute = path === '/twitch/feedback';
  const isChallengesRoute = path === '/twitch/challenges';
  const isVerwaltungRoute = path === PREVIEW_VERWALTUNG_ROUTE;
  const isOverlayBuilderRoute = path === PREVIEW_OVERLAY_ROUTE;
  const isTitleRoute = path === PREVIEW_TITLE_ROUTE;
  const isPricingRoute = path === PREVIEW_PRICING_ROUTE;
  const isUplinkRoute = path === PREVIEW_UPLINK_ROUTE;
  const isSocialMediaManagerRoute = path === PREVIEW_SOCIAL_ROUTE || path === '/twitch/social-media' || path.startsWith('/twitch/social-media/');
  const isAnalyticsRoute =
    path === PREVIEW_ANALYTICS_ROUTE ||
    path === '/twitch/analyse' ||
    path === '/dashboard-v2' ||
    path === '/twitch/dashboard-v2';
  const isCategoryRoute =
    path === '/twitch/kategorie' ||
    (isAnalyticsRoute && new URLSearchParams(window.location.search).get('view') === 'category');

  const activeRoute =
    isInternalHomeRoute ? 'home'
    : isChallengesRoute ? 'challenges'
    : isOverlayBuilderRoute ? 'overlay'
    : isVerwaltungRoute ? 'verwaltung'
    : isUplinkRoute ? 'uplink'
    : isTitleRoute ? 'title'
    : isPricingRoute ? 'pricing'
    : isSocialMediaManagerRoute ? 'social'
    : isHelpRoute ? 'hilfe'
    : isFeedbackRoute ? 'feedback'
    : isCategoryRoute ? 'category' : 'analyse';

  const zeigeAssistent =
    path === PREVIEW_HOME_ROUTE &&
    !isPreviewModeEnabled() &&
    !hasDemoRuntimeConfig() &&
    !resolveEffectiveDemoMode({
      pathname: window.location.pathname,
      runtimeConfig: dashboardRuntimeConfig,
    });

  return (
    <QueryClientProvider client={queryClient}>
      {/* Die Sprachwahl liegt ueber allem: sie gilt fuer jede Route dieses
          Bundles, auch fuer die Fehlergrenze. */}
      <LanguageProvider>
        <ErrorBoundary>
        <SessionBootstrap />
        <OnboardingProvider>
        <Suspense fallback={<DashboardShell activeRoute={activeRoute} showSidebar={!isPricingRoute} demoMode={isAnalyticsRoute && resolveEffectiveDemoMode({ pathname: path, runtimeConfig: dashboardRuntimeConfig })}><RouteSkeleton /></DashboardShell>}>
          {isCategoryRoute ? (
            <DashboardShell activeRoute="category"><CategoryCollector /></DashboardShell>
          ) : isSocialMediaManagerRoute ? (
            <DashboardShell activeRoute="social">
              <SocialMediaManager />
            </DashboardShell>
          ) : isHelpRoute ? (
            <DashboardShell activeRoute="hilfe"><EinrichtungCard help /><FeedbackBox area="Hilfe" /></DashboardShell>
          ) : isFeedbackRoute ? (
            <DashboardShell activeRoute="feedback"><FeedbackPage /></DashboardShell>
          ) : isChallengesRoute ? (
            <DashboardShell activeRoute="challenges"><Challenges /></DashboardShell>
          ) : isVerwaltungRoute ? (
            <DashboardShell activeRoute="verwaltung">
              <VerwaltungPage />
            </DashboardShell>
          ) : isOverlayBuilderRoute ? (
            <DashboardShell activeRoute="overlay">
              <OverlayBuilderPage />
            </DashboardShell>
          ) : isTitleRoute ? (
            <TitleGeneratorRoute />
          ) : isPricingRoute ? (
            <PricingRoute />
          ) : isUplinkRoute ? (
            <DashboardShell activeRoute="uplink">
              <UplinkPage />
            </DashboardShell>
          ) : isInternalHomeRoute ? (
            <DashboardShell activeRoute="home">
              <InternalHome />
            </DashboardShell>
          ) : isAnalyticsRoute ? (
            <AnalyticsDashboard key={location} />
          ) : (
            <AnalyticsDashboard key={location} />
          )}
        </Suspense>
        </OnboardingProvider>
        </ErrorBoundary>
        {zeigeAssistent && <DashboardAssistent />}
      </LanguageProvider>
    </QueryClientProvider>
  );
}
