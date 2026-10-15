import type { QueryClient } from '@tanstack/react-query';
import { fetchInternalHome } from './api/home';
import type { AuthStatus } from './api/auth';
import { fetchStreamerList } from './api/analytics';
import { pageLoaders } from './pageLoaders';
import { dashboardDestination } from './navigation';
import {
  PREVIEW_ANALYTICS_ROUTE, PREVIEW_HOME_ROUTE, PREVIEW_OVERLAY_ROUTE,
  PREVIEW_PRICING_ROUTE, PREVIEW_SOCIAL_ROUTE, PREVIEW_TITLE_ROUTE,
  PREVIEW_UPLINK_ROUTE, PREVIEW_VERWALTUNG_ROUTE,
} from './preview/routes';

export function prefetchDashboard(href: string, client: QueryClient) {
  const destination = dashboardDestination(href);
  if (!destination) return;
  const path = destination.pathname;
  const loader = path === PREVIEW_HOME_ROUTE ? pageLoaders.InternalHomeLanding
    : path === '/twitch/challenges' ? pageLoaders.Challenges
    : path === PREVIEW_OVERLAY_ROUTE ? pageLoaders.OverlayBuilderPage
    : path === PREVIEW_VERWALTUNG_ROUTE ? pageLoaders.VerwaltungPage
    : path === PREVIEW_UPLINK_ROUTE ? pageLoaders.UplinkPage
    : path === PREVIEW_TITLE_ROUTE ? pageLoaders.TitleGenerator
    : path === PREVIEW_PRICING_ROUTE ? pageLoaders.Pricing
    : path === PREVIEW_SOCIAL_ROUTE || path === '/twitch/social-media' ? pageLoaders.SocialMediaManager
    : path === '/twitch/hilfe' ? undefined
    : path === '/twitch/feedback' ? pageLoaders.FeedbackPage
    : path === PREVIEW_ANALYTICS_ROUTE || path === '/twitch/analyse'
      ? destination.searchParams.get('view') === 'category' ? pageLoaders.CategoryCollector : pageLoaders.Overview
      : undefined;
  if (loader) void loader().catch(() => undefined);
  if (path === PREVIEW_ANALYTICS_ROUTE || path === PREVIEW_HOME_ROUTE) {
    void client.prefetchQuery({ queryKey: ['streamers'], queryFn: fetchStreamerList, staleTime: 10 * 60 * 1000 });
  }
  const auth = client.getQueryData<AuthStatus>(['auth-status']);
  if ([PREVIEW_HOME_ROUTE, PREVIEW_OVERLAY_ROUTE, PREVIEW_VERWALTUNG_ROUTE].includes(path)
    && auth?.authenticated && !auth.isAdmin && !auth.isLocalhost) {
    void client.prefetchQuery({
      queryKey: ['internal-home', null, auth.twitchUserId],
      queryFn: () => fetchInternalHome(null),
      staleTime: 5 * 60 * 1000,
    });
  }
}
