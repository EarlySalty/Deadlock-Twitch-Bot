import { useState, useEffect, useRef, Component, type ReactNode, type ErrorInfo } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Header } from '@/components/layout/Header';
import { TabNavigation, type TabId } from '@/components/layout/TabNavigation';
import { Overview } from '@/pages/Overview';
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
import { VerwaltungPage } from '@/pages/Verwaltung';
import Pricing from '@/pages/Pricing';
import { AnalyticsTour } from '@/components/onboarding/AnalyticsTour';
import { PlanProvider } from '@/context/PlanContext';
import { TrialBanner } from '@/components/banners/TrialBanner';
import { TrialExpiryModal } from '@/components/modals/TrialExpiryModal';
import { useStreamerList, useAuthStatus } from '@/hooks/useAnalytics';
import type { TimeRange } from '@/types/analytics';
import {
  PREVIEW_ANALYTICS_ROUTE,
  PREVIEW_HOME_ROUTE,
  PREVIEW_PRICING_ROUTE,
  PREVIEW_VERWALTUNG_ROUTE,
} from '@/preview/routes';
import { dashboardRuntimeConfig, resolveEffectiveDemoMode } from '@/runtimeConfig';
import {
  AlertTriangle,
  Sparkles,
  Shield,
  ShieldAlert,
  ShieldCheck,
  Wifi,
} from 'lucide-react';

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
        <div className="min-h-screen bg-bg flex items-center justify-center p-8">
          <div className="panel-card rounded-2xl p-8 max-w-lg text-center">
            <AlertTriangle className="w-12 h-12 text-warning mx-auto mb-4" />
            <h2 className="text-xl font-bold text-white mb-2">Dashboard-Fehler</h2>
            <p className="text-text-secondary mb-4">
              {this.state.error?.message || 'Ein unerwarteter Fehler ist aufgetreten.'}
            </p>
            <button
              onClick={() => this.setState({ hasError: false, error: null })}
              className="px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary-hover transition-colors"
            >
              Erneut versuchen
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}

// Create QueryClient
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 2,
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

function AnalyticsDashboard() {
  const [streamer, setStreamer] = useState<string | null>(null);
  const [days, setDays] = useState<TimeRange>(30);
  const [activeTab, setActiveTab] = useState<TabId | 'session-detail'>('overview');
  const [selectedSessionId, setSelectedSessionId] = useState<number | null>(null);
  const [pendingSub, setPendingSub] = useState<string | null>(null);
  const [pendingMode, setPendingMode] = useState<string | null>(null);

  const { data: streamers = [], isLoading: loadingStreamers } = useStreamerList();
  const { data: authStatus, isLoading: loadingAuth, isError: authError } = useAuthStatus();
  const isDemoShell = resolveEffectiveDemoMode({
    pathname: window.location.pathname,
    runtimeConfig: dashboardRuntimeConfig,
  });
  const isDemoMode = resolveEffectiveDemoMode({
    pathname: window.location.pathname,
    runtimeConfig: dashboardRuntimeConfig,
  });

  // Tracks if we already auto-set the streamer from auth (fire-once guard)
  const hasAutoSetStreamer = useRef(false);

  // Parse URL params on mount
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const urlStreamer = params.get('streamer');
    const urlDays = params.get('days');

    if (urlStreamer) {
      const normalizedStreamer = urlStreamer.trim().toLowerCase();
      if (
        !isDemoShell ||
        dashboardRuntimeConfig.allowedDemoProfiles.length === 0 ||
        dashboardRuntimeConfig.allowedDemoProfiles.includes(normalizedStreamer)
      ) {
        setStreamer(normalizedStreamer);
        hasAutoSetStreamer.current = true; // URL explicitly set — skip auto-set
      }
    }
    if (urlDays) {
      const d = parseInt(urlDays, 10);
      if (d === 7 || d === 30 || d === 90) setDays(d);
    }
    const resolved = resolveTabParam(params.get('tab'));
    if (resolved) {
      setActiveTab(resolved.tab);
      setPendingSub(params.get('sub') ?? resolved.sub ?? null);
      setPendingMode(params.get('mode') ?? resolved.mode ?? null);
    }
  }, [isDemoShell]);

  // Auto-set streamer to logged-in Twitch user on first auth load
  useEffect(() => {
    const fallbackStreamer =
      authStatus?.twitchLogin ??
      (isDemoShell ? dashboardRuntimeConfig.defaultDemoProfile : null);
    if (!hasAutoSetStreamer.current && fallbackStreamer) {
      setStreamer(fallbackStreamer);
      hasAutoSetStreamer.current = true;
    }
  }, [authStatus, isDemoShell]);

  // Update URL when params change
  useEffect(() => {
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
    setActiveTab(tab);
    setPendingSub(null);
    setPendingMode(null);
  };

  // Auth badge component
  const AuthBadge = () => {
    const badgeBase =
      'flex items-center gap-2 px-3 py-1.5 rounded-full border text-xs font-semibold tracking-wide backdrop-blur-md';

    if (loadingAuth) return null;

    if (isDemoMode) {
      return (
        <div className={`${badgeBase} bg-warning/10 border-warning/30 text-warning`}>
          <Sparkles className="w-4 h-4" />
          <span>Demo-Daten</span>
        </div>
      );
    }

    if (authError || !authStatus?.authenticated) {
      return (
        <div className={`${badgeBase} bg-error/10 border-error/30 text-error`}>
          <ShieldAlert className="w-4 h-4" />
          <span>Nicht authentifiziert</span>
        </div>
      );
    }

    if (authStatus.isLocalhost) {
      return (
        <div className={`${badgeBase} bg-success/10 border-success/30 text-success`}>
          <Wifi className="w-4 h-4" />
          <span>Localhost (Admin)</span>
        </div>
      );
    }

    if (authStatus.isAdmin) {
      return (
        <div className={`${badgeBase} bg-primary/10 border-primary/30 text-primary`}>
          <ShieldCheck className="w-4 h-4" />
          <span>Admin</span>
        </div>
      );
    }

    return (
      <div className={`${badgeBase} bg-accent/10 border-accent/30 text-accent`}>
        <Shield className="w-4 h-4" />
        <span>Partner</span>
      </div>
    );
  };

  return (
    <div className="min-h-screen relative px-3 py-4 md:px-7 md:py-8">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-28 right-[-7rem] h-[25rem] w-[25rem] rounded-full bg-primary/12 blur-3xl" />
        <div className="absolute top-[28%] -left-24 h-[20rem] w-[20rem] rounded-full bg-accent/14 blur-3xl" />
      </div>
      <div className="relative max-w-[1700px] mx-auto">
        {/* Auth Status Badge */}
        <div className="flex justify-end mb-4">
          <AuthBadge />
        </div>

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
              Demo-Daten aus einem statischen Snapshot. Profilwechsel und Analysen laufen ausschließlich über den Demo-Namespace.
            </div>
          )}

          {activeTab !== 'session-detail' && (
            <TabNavigation activeTab={activeTab as TabId} onTabChange={handleTabChange} />
          )}

          {/* Tab Content */}
          {activeTab === 'overview' && (
            <Overview
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

      </div>
    </div>
  );
}

export default function App() {
  const path = normalizePathname(window.location.pathname);
  const isInternalHomeRoute = path === PREVIEW_HOME_ROUTE;
  const isVerwaltungRoute = path === PREVIEW_VERWALTUNG_ROUTE;
  const isPricingRoute = path === PREVIEW_PRICING_ROUTE;
  const isSocialMediaAdminRoute = path === '/social-media-admin';
  const isAnalyticsRoute =
    path === PREVIEW_ANALYTICS_ROUTE ||
    path === '/analyse' ||
    path === '/dashboard-v2' ||
    path === '/twitch/dashboard-v2';

  return (
    <QueryClientProvider client={queryClient}>
      <ErrorBoundary>
        {isSocialMediaAdminRoute ? (
          <SocialMediaAdminDashboard />
        ) : isVerwaltungRoute ? (
          <VerwaltungPage />
        ) : isPricingRoute ? (
          <Pricing />
        ) : isInternalHomeRoute ? (
          <InternalHome />
        ) : isAnalyticsRoute ? (
          <AnalyticsDashboard />
        ) : (
          <AnalyticsDashboard />
        )}
      </ErrorBoundary>
    </QueryClientProvider>
  );
}
