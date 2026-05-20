// React Query hooks for analytics data

import { useQuery } from '@tanstack/react-query';
import {
  fetchOverview,
  fetchMonthlyStats,
  fetchWeekdayStats,
  fetchHourlyHeatmap,
  fetchCalendarHeatmap,
  fetchChatAnalytics,
  fetchViewerOverlap,
  fetchTagAnalysis,
  fetchTagAnalysisExtended,
  fetchTitlePerformance,
  fetchRankings,
  fetchSessionDetail,
  fetchSessionEvents,
  fetchStreamerList,
  fetchCategoryComparison,
  fetchWatchTimeDistribution,
  fetchFollowerFunnel,
  fetchAudienceInsights,
  fetchAudienceDemographics,
  fetchViewerTimeline,
  fetchViewerPresenceTimeline,
  fetchViewerTimelineProfile,
  fetchCategoryLeaderboard,
  fetchCoaching,
  fetchMonetization,
  fetchCategoryTimings,
  fetchCategoryActivitySeries,
  fetchLurkerAnalysis,
  fetchRaidRetention,
  fetchRaidAnalytics,
  fetchViewerProfiles,
  fetchAudienceSharing,
  fetchViewerDirectory,
  fetchViewerDetail,
  fetchViewerSegments,
  fetchChatHypeTimeline,
  fetchChatContentAnalysis,
  fetchChatSocialGraph,
  fetchExpOverview,
  fetchExpGameBreakdown,
  fetchExpGameTransitions,
  fetchExpGrowthCurves,
  fetchRoadmap,
  fetchAdsSchedule,
  fetchStreamReport,
  fetchStreamReportAB,
} from '@/api/analytics';
import { fetchAuthStatus } from '@/api/auth';
import { fetchBillingCatalog } from '@/api/billing';
import {
  fetchAdminAffiliates,
  fetchAdminAffiliateStats,
  fetchAdminAffiliateDetail,
} from '@/api/admin';
import { fetchAffiliatePortal } from '@/api/affiliate';
import type { StreamReport, StreamReportABResponse, StreamReportVariant, TimeRange, ViewerSortField, ViewerFilterType } from '@/types/analytics';

// Stale time: 5 minutes
const STALE_TIME = 5 * 60 * 1000;

export function useOverview(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['overview', streamer, days],
    queryFn: () => fetchOverview(streamer, days),
    staleTime: STALE_TIME,
    enabled: true,
  });
}

export function useMonthlyStats(streamer: string | null, months = 12) {
  return useQuery({
    queryKey: ['monthly-stats', streamer, months],
    queryFn: () => fetchMonthlyStats(streamer, months),
    staleTime: STALE_TIME,
  });
}

export function useWeekdayStats(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['weekday-stats', streamer, days],
    queryFn: () => fetchWeekdayStats(streamer, days),
    staleTime: STALE_TIME,
  });
}

export function useHourlyHeatmap(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['hourly-heatmap', streamer, days],
    queryFn: () => fetchHourlyHeatmap(streamer, days),
    staleTime: STALE_TIME,
  });
}

export function useCalendarHeatmap(streamer: string | null, days = 365) {
  return useQuery({
    queryKey: ['calendar-heatmap', streamer, days],
    queryFn: () => fetchCalendarHeatmap(streamer, days),
    staleTime: STALE_TIME,
  });
}

export function useChatAnalytics(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['chat-analytics', streamer, days],
    queryFn: () => fetchChatAnalytics(streamer, days),
    staleTime: STALE_TIME,
  });
}

export function useViewerOverlap(streamer: string | null, limit = 20) {
  return useQuery({
    queryKey: ['viewer-overlap', streamer, limit],
    queryFn: () => fetchViewerOverlap(streamer, limit),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

export function useTagAnalysis(days: TimeRange, limit = 30) {
  return useQuery({
    queryKey: ['tag-analysis', days, limit],
    queryFn: () => fetchTagAnalysis(days, limit),
    staleTime: STALE_TIME,
  });
}

export function useRankings(
  metric: 'viewers' | 'growth' | 'retention' | 'chat',
  days: TimeRange,
  limit = 20,
  excludeExternal = true
) {
  return useQuery({
    queryKey: ['rankings', metric, days, limit, excludeExternal],
    queryFn: () => fetchRankings(metric, days, limit, excludeExternal),
    staleTime: STALE_TIME,
  });
}

export function useSessionDetail(sessionId: number | null) {
  return useQuery({
    queryKey: ['session', sessionId],
    queryFn: () => fetchSessionDetail(sessionId!),
    staleTime: STALE_TIME,
    enabled: !!sessionId,
  });
}

export function useSessionEvents(sessionId: number | null) {
  return useQuery({
    queryKey: ['session-events', sessionId],
    queryFn: () => fetchSessionEvents(sessionId!),
    staleTime: STALE_TIME,
    enabled: !!sessionId,
  });
}

export function useStreamerList() {
  return useQuery({
    queryKey: ['streamers'],
    queryFn: fetchStreamerList,
    staleTime: 10 * 60 * 1000, // 10 minutes
  });
}

export function useCategoryComparison(
  streamer: string | null,
  days: TimeRange,
  excludeExternal = true
) {
  return useQuery({
    queryKey: ['category-comparison', streamer, days, excludeExternal],
    queryFn: () => fetchCategoryComparison(streamer, days, excludeExternal),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

export function useAuthStatus() {
  return useQuery({
    queryKey: ['auth-status'],
    queryFn: fetchAuthStatus,
    staleTime: 60 * 1000, // 1 minute
    retry: false,
  });
}

// Watch Time Distribution Hook
export function useWatchTimeDistribution(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['watch-time-distribution', streamer, days],
    queryFn: () => fetchWatchTimeDistribution(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Follower Funnel Hook
export function useFollowerFunnel(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['follower-funnel', streamer, days],
    queryFn: () => fetchFollowerFunnel(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Extended Tag Analysis Hook
export function useTagAnalysisExtended(streamer: string | null, days: TimeRange, limit = 20) {
  return useQuery({
    queryKey: ['tag-analysis-extended', streamer, days, limit],
    queryFn: () => fetchTagAnalysisExtended(streamer, days, limit),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Title Performance Hook
export function useTitlePerformance(streamer: string | null, days: TimeRange, limit = 20) {
  return useQuery({
    queryKey: ['title-performance', streamer, days, limit],
    queryFn: () => fetchTitlePerformance(streamer, days, limit),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Combined Audience Insights Hook
export function useAudienceInsights(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['audience-insights', streamer, days],
    queryFn: () => fetchAudienceInsights(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Audience Demographics Hook
export function useAudienceDemographics(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['audience-demographics', streamer, days],
    queryFn: () => fetchAudienceDemographics(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Viewer Timeline Hook (stats_tracked bucketed data)
export function useViewerCountTimeline(streamer: string | null, days: number) {
  return useQuery({
    queryKey: ['viewer-timeline', streamer, days],
    queryFn: () => fetchViewerTimeline(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

export function useViewerTimeline(
  streamer: string | null,
  sessionId: number | null,
  options: {
    minPresentMin?: number;
    segment?: string;
    search?: string;
    limit?: number;
  } = {}
) {
  return useQuery({
    queryKey: ['viewer-presence-timeline', streamer, sessionId, options],
    queryFn: () => fetchViewerPresenceTimeline(streamer, sessionId!, options),
    staleTime: STALE_TIME,
    enabled: !!streamer && !!sessionId,
  });
}

export function useViewerTimelineProfile(streamer: string | null, login: string | null) {
  return useQuery({
    queryKey: ['viewer-timeline-profile', streamer, login],
    queryFn: () => fetchViewerTimelineProfile(streamer, login!),
    staleTime: STALE_TIME,
    enabled: !!streamer && !!login,
  });
}

// Coaching Hook
export function useCoaching(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['coaching', streamer, days],
    queryFn: () => fetchCoaching(streamer!, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Category Timings Hook
export function useCategoryTimings(days: TimeRange, source: 'category' | 'tracked' = 'category') {
  return useQuery({
    queryKey: ['category-timings', days, source],
    queryFn: () => fetchCategoryTimings(days, source),
    staleTime: STALE_TIME,
  });
}

export function useCategoryActivitySeries(days: TimeRange) {
  return useQuery({
    queryKey: ['category-activity-series', days],
    queryFn: () => fetchCategoryActivitySeries(days),
    staleTime: STALE_TIME,
  });
}

// Ads Schedule Hook
export function useAdsSchedule(streamer: string | null) {
  return useQuery({
    queryKey: ['ads-schedule', streamer],
    queryFn: () => fetchAdsSchedule(streamer!),
    enabled: !!streamer,
    staleTime: STALE_TIME,
  });
}

// Monetization Hook
export function useMonetization(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['monetization', streamer, days],
    queryFn: () => fetchMonetization(streamer, days),
    staleTime: STALE_TIME,
  });
}

// Lurker Analysis Hook
export function useLurkerAnalysis(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['lurker-analysis', streamer, days],
    queryFn: () => fetchLurkerAnalysis(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Raid Retention Hook
export function useRaidRetention(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['raid-retention', streamer, days],
    queryFn: () => fetchRaidRetention(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Raid Analytics Hook (includes incoming raids)
export function useRaidAnalytics(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['raid-analytics', streamer, days],
    queryFn: () => fetchRaidAnalytics(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Viewer Profiles Hook
export function useViewerProfiles(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['viewer-profiles', streamer, days],
    queryFn: () => fetchViewerProfiles(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Audience Sharing Hook
export function useAudienceSharing(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['audience-sharing', streamer, days],
    queryFn: () => fetchAudienceSharing(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Viewer Directory Hook
export function useViewerDirectory(
  streamer: string | null,
  days: TimeRange,
  sort: ViewerSortField = 'sessions',
  order: 'asc' | 'desc' = 'desc',
  filter: ViewerFilterType = 'all',
  search: string = '',
  page: number = 1,
  perPage: number = 50
) {
  return useQuery({
    queryKey: ['viewer-directory', streamer, days, sort, order, filter, search, page, perPage],
    queryFn: () => fetchViewerDirectory(streamer, days, sort, order, filter, search, page, perPage),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Viewer Detail Hook
export function useViewerDetail(streamer: string | null, login: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['viewer-detail', streamer, login, days],
    queryFn: () => fetchViewerDetail(streamer, login!, days),
    staleTime: STALE_TIME,
    enabled: !!streamer && !!login,
  });
}

// Viewer Segments Hook
export function useViewerSegments(streamer: string | null, days: TimeRange) {
  return useQuery({
    queryKey: ['viewer-segments', streamer, days],
    queryFn: () => fetchViewerSegments(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Chat Hype Timeline Hook
export function useChatHypeTimeline(streamer: string | null, sessionId?: number) {
  return useQuery({
    queryKey: ['chat-hype-timeline', streamer, sessionId],
    queryFn: () => fetchChatHypeTimeline(streamer, sessionId),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Chat Content Analysis Hook
export function useChatContentAnalysis(streamer: string | null, days: number) {
  return useQuery({
    queryKey: ['chat-content-analysis', streamer, days],
    queryFn: () => fetchChatContentAnalysis(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Chat Social Graph Hook
export function useChatSocialGraph(streamer: string | null, days: number) {
  return useQuery({
    queryKey: ['chat-social-graph', streamer, days],
    queryFn: () => fetchChatSocialGraph(streamer, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

// Category Leaderboard Hook
export function useCategoryLeaderboard(
  streamer: string | null,
  days: number,
  limit = 25,
  sort: 'avg' | 'peak' = 'avg',
  excludeExternal = false
) {
  return useQuery({
    queryKey: ['category-leaderboard', streamer, days, limit, sort, excludeExternal],
    queryFn: () => fetchCategoryLeaderboard(streamer, days, limit, sort, excludeExternal),
    staleTime: STALE_TIME,
  });
}

// ========= Experimental (Labor) Hooks =========

export function useExpOverview(streamer: string | null, days: number) {
  return useQuery({
    queryKey: ['exp-overview', streamer, days],
    queryFn: () => fetchExpOverview(streamer!, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

export function useExpGameBreakdown(streamer: string | null, days: number) {
  return useQuery({
    queryKey: ['exp-game-breakdown', streamer, days],
    queryFn: () => fetchExpGameBreakdown(streamer!, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

export function useExpGameTransitions(streamer: string | null, days: number) {
  return useQuery({
    queryKey: ['exp-game-transitions', streamer, days],
    queryFn: () => fetchExpGameTransitions(streamer!, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

export function useExpGrowthCurves(streamer: string | null, days: number) {
  return useQuery({
    queryKey: ['exp-growth-curves', streamer, days],
    queryFn: () => fetchExpGrowthCurves(streamer!, days),
    staleTime: STALE_TIME,
    enabled: !!streamer,
  });
}

export function useRoadmap() {
  return useQuery({
    queryKey: ['roadmap'],
    queryFn: fetchRoadmap,
    staleTime: 2 * 60 * 1000, // 2 minutes
  });
}

// ── Billing ────────────────────────────────────────
export function useBillingCatalog(cycle: 1 | 12 = 1) {
  return useQuery({
    queryKey: ['billing-catalog', cycle],
    queryFn: () => fetchBillingCatalog(cycle),
    staleTime: STALE_TIME,
  });
}

// ── Admin Affiliates ───────────────────────────────
export function useAdminAffiliates() {
  return useQuery({
    queryKey: ['admin-affiliates'],
    queryFn: fetchAdminAffiliates,
  });
}

export function useAdminAffiliateStats() {
  return useQuery({
    queryKey: ['admin-affiliate-stats'],
    queryFn: fetchAdminAffiliateStats,
  });
}

export function useAdminAffiliateDetail(login: string | null) {
  return useQuery({
    queryKey: ['admin-affiliate-detail', login],
    queryFn: () => fetchAdminAffiliateDetail(login!),
    enabled: !!login,
  });
}

// ── Affiliate Portal ───────────────────────────────
export function useAffiliatePortal() {
  return useQuery({
    queryKey: ['affiliate-portal'],
    queryFn: fetchAffiliatePortal,
    retry: false,
  });
}

export function useStreamReport(
  streamer: string | null,
  sessionId?: number,
  variant: StreamReportVariant = 'compact'
) {
  return useQuery<StreamReport>({
    queryKey: ['stream-report', streamer, sessionId ?? null, variant],
    queryFn: () => fetchStreamReport(streamer, sessionId, variant),
    staleTime: 2 * 60 * 1000,
    enabled: !!streamer,
    retry: false,
  });
}

export function useStreamReportAB(streamer: string | null, sessionId?: number) {
  return useQuery<StreamReportABResponse>({
    queryKey: ['stream-report-ab', streamer, sessionId ?? null],
    queryFn: () => fetchStreamReportAB(streamer, sessionId),
    staleTime: 2 * 60 * 1000,
    enabled: !!streamer,
    retry: false,
  });
}
