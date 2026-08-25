import { createBrowserRouter, Navigate, useParams } from 'react-router';
import { AdminShell } from '@/components/layout/AdminShell';
import { Dashboard } from '@/pages/Dashboard';
import { Affiliates } from '@/pages/billing/Affiliates';
import { Gutschriften } from '@/pages/billing/Gutschriften';
import { Subscriptions } from '@/pages/billing/Subscriptions';
import { BotConfig } from '@/pages/config/BotConfig';
import { ChatConfig } from '@/pages/config/ChatConfig';
import { RaidConfig } from '@/pages/config/RaidConfig';
import { DatabaseStats } from '@/pages/monitoring/DatabaseStats';
import DatabaseQueryPage from '@/pages/monitoring/DatabaseQuery';
import { ErrorLogs } from '@/pages/monitoring/ErrorLogs';
import { EventSubStatusPage } from '@/pages/monitoring/EventSubStatus';
import { SystemOverview } from '@/pages/monitoring/SystemOverview';
import ChatActionsPage from '@/pages/community/ChatActions';
import EngagementPage from '@/pages/community/Engagement';
import GlobalBansPage from '@/pages/community/GlobalBans';
import MarketSharePage from '@/pages/community/MarketShare';
import PartnerSignupBlocksPage from '@/pages/community/PartnerSignupBlocks';
import RaidsActivityPage from '@/pages/community/RaidsActivity';
import ResearchPage from '@/pages/community/Research';
import AnnouncementsPage from '@/pages/content/Announcements';
import ChangelogPage from '@/pages/content/Changelog';
import LegalPage from '@/pages/content/Legal';
import RoadmapPage from '@/pages/content/Roadmap';
import BotControlPage from '@/pages/operations/BotControl';
import ScopesPage from '@/pages/operations/Scopes';
import AuditLogPage from '@/pages/money/AuditLog';
import { StreamerDetailPage } from '@/pages/streamers/StreamerDetail';
import { StreamerList } from '@/pages/streamers/StreamerList';

interface LegacyRedirectProps {
  to: string;
}

function LegacyRedirect({ to }: LegacyRedirectProps) {
  const params = useParams();
  let resolvedTo = to;

  Object.entries(params).forEach(([key, value]) => {
    if (value) {
      resolvedTo = resolvedTo.replace(`:${key}`, encodeURIComponent(value));
    }
  });

  return <Navigate to={resolvedTo} replace />;
}

const router = createBrowserRouter(
  [
    {
      path: '/',
      element: <AdminShell />,
      children: [
        { index: true, element: <Dashboard /> },
        { path: 'operations', element: <Navigate to="/operations/system" replace /> },
        { path: 'operations/system', element: <SystemOverview /> },
        { path: 'operations/scopes', element: <ScopesPage /> },
        { path: 'operations/eventsub', element: <EventSubStatusPage /> },
        { path: 'operations/database', element: <DatabaseStats /> },
        { path: 'operations/query', element: <DatabaseQueryPage /> },
        { path: 'operations/errors', element: <ErrorLogs /> },
        { path: 'operations/bot', element: <BotControlPage /> },
        { path: 'community', element: <Navigate to="/community/streamers" replace /> },
        { path: 'community/streamers', element: <StreamerList /> },
        { path: 'community/streamers/:login', element: <StreamerDetailPage /> },
        { path: 'community/raids', element: <RaidsActivityPage /> },
        { path: 'community/market', element: <MarketSharePage /> },
        { path: 'community/research', element: <ResearchPage /> },
        { path: 'community/engagement', element: <EngagementPage /> },
        { path: 'community/chat', element: <ChatActionsPage /> },
        { path: 'community/global-bans', element: <GlobalBansPage /> },
        { path: 'community/partner-signup-blocks', element: <PartnerSignupBlocksPage /> },
        { path: 'content', element: <Navigate to="/content/announcements" replace /> },
        { path: 'content/announcements', element: <AnnouncementsPage /> },
        { path: 'content/roadmap', element: <RoadmapPage /> },
        { path: 'content/changelog', element: <ChangelogPage /> },
        { path: 'content/legal', element: <LegalPage /> },
        { path: 'config', element: <BotConfig /> },
        { path: 'config/raids', element: <RaidConfig /> },
        { path: 'config/chat', element: <ChatConfig /> },
        { path: 'money', element: <Navigate to="/money/subscriptions" replace /> },
        { path: 'money/subscriptions', element: <Subscriptions /> },
        { path: 'money/affiliates', element: <Affiliates /> },
        { path: 'money/gutschriften', element: <Gutschriften /> },
        { path: 'money/audit', element: <AuditLogPage /> },
        { path: 'streamers', element: <Navigate to="/community/streamers" replace /> },
        { path: 'streamers/:login', element: <LegacyRedirect to="/community/streamers/:login" /> },
        { path: 'monitoring', element: <Navigate to="/operations/system" replace /> },
        { path: 'monitoring/eventsub', element: <Navigate to="/operations/eventsub" replace /> },
        { path: 'monitoring/database', element: <Navigate to="/operations/database" replace /> },
        { path: 'monitoring/errors', element: <Navigate to="/operations/errors" replace /> },
        { path: 'billing', element: <Navigate to="/money/subscriptions" replace /> },
        { path: 'billing/affiliates', element: <Navigate to="/money/affiliates" replace /> },
        { path: 'billing/gutschriften', element: <Navigate to="/money/gutschriften" replace /> },
        { path: '*', element: <Navigate to="/" replace /> },
      ],
    },
  ],
  { basename: '/twitch/admin' },
);

export default router;
