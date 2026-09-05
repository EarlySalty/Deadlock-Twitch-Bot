import { useState, useEffect, useRef, Component, type ReactNode, type ErrorInfo } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Header } from '@/components/layout/Header';
import { TabNavigation, type TabId } from '@/components/layout/TabNavigation';
import { Overview } from '@/pages/Overview';
import { Tagesform } from '@/pages/Tagesform';
import { Sessions } from '@/pages/Sessions';
import { SocialMediaAdminDashboard } from '@/pages/SocialMediaAdmin';
import { Monetization } from '@/pages/Monetization';
import { Publikum } from '@/pages/Publikum';
import { Wachstum } from '@/pages/Wachstum';
import { Planung } from '@/pages/Planung';
import { WasTun } from '@/pages/WasTun';
import { resolveTabParam } from '@/tabAliases';
import { SessionDetail } from '@/pages/SessionDetail';
import { InternalHomeLanding } from '@/pages/InternalHomeLanding';
import { UplinkPage } from '@/pages/Uplink';
import { VerwaltungPage } from '@/pages/Verwaltung';
import { OverlayBuilderPage } from '@/pages/OverlayBuilder';
import Pricing from '@/pages/Pricing';
import { DashboardShell } from '@/components/layout/DashboardShell';
import { AnalyticsTour } from '@/components/onboarding/AnalyticsTour';
import { PlanProvider } from '@/context/PlanContext';
import { LanguageProvider, useT } from '@/context/LanguageContext';
import { DashboardAssistent } from '@/components/assistent/DashboardAssistent';
import { TrialBanner } from '@/components/banners/TrialBanner';
import { TrialExpiryModal } from '@/components/modals/TrialExpiryModal';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import { usePlan } from '@/context/PlanContext';
import type { TimeRange } from '@/types/analytics';
import { parseDaysParam } from '@/utils/zeitraum';
import {
  PREVIEW_ANALYTICS_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_UPLINK_ROUTE,
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
    },
  },
});

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
  const [streamer, setStreamer] = useState<string | null>(null);
  const [days, setDays] = useState<TimeRange>(30);
  const [activeTab, setActiveTab] = useState<TabId | 'session-detail'>('overview');
  const [selectedSessionId, setSelectedSessionId] = useState<number | null>(null);
  const [pendingSub, setPendingSub] = useState<string | null>(null);
  const [pendingMode, setPendingMode] = useState<string | null>(null);
  const hasExplicitTab = useRef(
    Boolean(resolveTabParam(new URLSearchParams(window.location.search).get('tab'))),
  );

  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const { data: authStatus } = useAuthStatus();
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
    if (
      !isDemoShell ||
      dashboardRuntimeConfig.allowedDemoProfiles.length === 0 ||
      dashboardRuntimeConfig.allowedDemoProfiles.includes(wunschStreamer)
    ) {
      setStreamer(wunschStreamer);
      hasAutoSetStreamer.current = true;
      pendingUrlStreamer.current = null;
    }
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
          <AnalyticsTour />
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
  const path = normalizePathname(window.location.pathname);
  const isInternalHomeRoute = path === PREVIEW_HOME_ROUTE;
  const isVerwaltungRoute = path === PREVIEW_VERWALTUNG_ROUTE;
  const isOverlayBuilderRoute = path === PREVIEW_OVERLAY_ROUTE;
  const isPricingRoute = path === PREVIEW_PRICING_ROUTE;
  const isUplinkRoute = path === PREVIEW_UPLINK_ROUTE;
  const isSocialMediaAdminRoute = path === '/social-media-admin';
  const isAnalyticsRoute =
    path === PREVIEW_ANALYTICS_ROUTE ||
    path === '/analyse' ||
    path === '/twitch/onboarding' ||
    path === '/dashboard-v2' ||
    path === '/twitch/dashboard-v2';

  const zeigeAssistent =
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
          {isSocialMediaAdminRoute ? (
            <DashboardShell activeRoute="social">
              <SocialMediaAdminDashboard />
            </DashboardShell>
          ) : isVerwaltungRoute ? (
            <DashboardShell activeRoute="verwaltung">
              <VerwaltungPage />
            </DashboardShell>
          ) : isOverlayBuilderRoute ? (
            <DashboardShell activeRoute="overlay">
              <OverlayBuilderPage />
            </DashboardShell>
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
            <AnalyticsDashboard />
          ) : (
            <AnalyticsDashboard />
          )}
        </ErrorBoundary>
        {zeigeAssistent && <DashboardAssistent />}
      </LanguageProvider>
    </QueryClientProvider>
  );
}
