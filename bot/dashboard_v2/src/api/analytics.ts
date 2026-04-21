import { fetchApi, getBrowserTimezone } from './core';
import type {
  AdsSchedule,
  AudienceInsights,
  AudienceSharing,
  CategoryActivitySeries,
  CategoryComparison,
  CategoryLeaderboard,
  CategoryTimings,
  ChatAnalytics,
  ChatContentAnalysis,
  ChatHypeTimeline,
  ChatSocialGraph,
  CoachingData,
  DashboardOverview,
  ExpGameBreakdown,
  ExpGameTransition,
  ExpGrowthCurve,
  ExpOverview,
  FollowerFunnel,
  HourlyHeatmapData,
  LurkerAnalysis,
  MonthlyStats,
  MonetizationStats,
  RaidAnalytics,
  RaidRetention,
  RankingEntry,
  CalendarHeatmapData,
  SessionEvent,
  StreamReport,
  StreamSession,
  TagAnalysisResponse,
  TagPerformance,
  TagPerformanceExtended,
  TimeRange,
  TitlePerformance,
  TitlePerformanceResponse,
  ViewerDetail,
  ViewerDirectory,
  ViewerFilterType,
  ViewerTimelineProfileResponse,
  ViewerTimelineSessionResponse,
  ViewerOverlap,
  ViewerProfiles,
  ViewerSegments,
  ViewerSortField,
  ViewerTimelinePoint,
  WeekdayStats,
  WatchTimeDistribution,
} from '@/types/analytics';

export async function fetchOverview(
  streamer: string | null,
  days: TimeRange
): Promise<DashboardOverview> {
  return fetchApi<DashboardOverview>('/overview', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchMonthlyStats(
  streamer: string | null,
  months: number = 12
): Promise<MonthlyStats[]> {
  return fetchApi<MonthlyStats[]>('/monthly-stats', {
    streamer: streamer || '',
    months,
  });
}

export async function fetchWeekdayStats(
  streamer: string | null,
  days: TimeRange
): Promise<WeekdayStats[]> {
  return fetchApi<WeekdayStats[]>('/weekly-stats', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchHourlyHeatmap(
  streamer: string | null,
  days: TimeRange
): Promise<HourlyHeatmapData[]> {
  return fetchApi<HourlyHeatmapData[]>('/hourly-heatmap', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchCalendarHeatmap(
  streamer: string | null,
  days: number = 365
): Promise<CalendarHeatmapData[]> {
  return fetchApi<CalendarHeatmapData[]>('/calendar-heatmap', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchChatAnalytics(
  streamer: string | null,
  days: TimeRange
): Promise<ChatAnalytics> {
  return fetchApi<ChatAnalytics>('/chat-analytics', {
    streamer: streamer || '',
    days,
    timezone: getBrowserTimezone(),
  });
}

export async function fetchViewerOverlap(
  streamer: string | null,
  limit: number = 20
): Promise<ViewerOverlap[]> {
  return fetchApi<ViewerOverlap[]>('/viewer-overlap', {
    streamer: streamer || '',
    limit,
  });
}

export async function fetchTagAnalysis(
  days: TimeRange,
  limit: number = 30
): Promise<TagPerformance[]> {
  return fetchApi<TagPerformance[]>('/tag-analysis', {
    days,
    limit,
  });
}

export async function fetchRankings(
  metric: 'viewers' | 'growth' | 'retention' | 'chat',
  days: TimeRange,
  limit: number = 20,
  excludeExternal = true
): Promise<RankingEntry[]> {
  return fetchApi<RankingEntry[]>('/rankings', {
    metric,
    days,
    limit,
    ...(excludeExternal && { exclude_external: '1' }),
  });
}

export async function fetchSessionDetail(
  sessionId: number
): Promise<StreamSession & { timeline: { minute: number; viewers: number }[]; chatters: { login: string; messages: number }[] }> {
  return fetchApi(`/session/${sessionId}`);
}

export async function fetchSessionEvents(
  sessionId: number
): Promise<SessionEvent> {
  return fetchApi<SessionEvent>(`/session/${sessionId}/events`);
}

export async function fetchStreamerList(): Promise<{ login: string; isPartner: boolean }[]> {
  return fetchApi<{ login: string; isPartner: boolean }[]>('/streamers');
}

export async function fetchCategoryComparison(
  streamer: string | null,
  days: TimeRange,
  excludeExternal = true
): Promise<CategoryComparison> {
  return fetchApi('/category-comparison', {
    streamer: streamer || '',
    days,
    ...(excludeExternal && { exclude_external: '1' }),
  });
}

export async function fetchWatchTimeDistribution(
  streamer: string | null,
  days: TimeRange
): Promise<WatchTimeDistribution> {
  return fetchApi<WatchTimeDistribution>('/watch-time-distribution', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchFollowerFunnel(
  streamer: string | null,
  days: TimeRange
): Promise<FollowerFunnel> {
  return fetchApi<FollowerFunnel>('/follower-funnel', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchTagAnalysisExtended(
  streamer: string | null,
  days: TimeRange,
  limit: number = 20
): Promise<TagAnalysisResponse> {
  const raw = await fetchApi<TagAnalysisResponse | TagPerformanceExtended[]>('/tag-analysis-extended', {
    streamer: streamer || '',
    days,
    limit,
  });
  if (Array.isArray(raw)) {
    return { tags: raw, peerBenchmark: null };
  }
  return raw;
}

export async function fetchTitlePerformance(
  streamer: string | null,
  days: TimeRange,
  limit: number = 20
): Promise<TitlePerformanceResponse> {
  const raw = await fetchApi<TitlePerformanceResponse | TitlePerformance[]>('/title-performance', {
    streamer: streamer || '',
    days,
    limit,
  });
  if (Array.isArray(raw)) {
    return { titles: raw, peerBenchmark: null };
  }
  return raw;
}

export async function fetchAudienceInsights(
  streamer: string | null,
  days: TimeRange
): Promise<AudienceInsights> {
  return fetchApi<AudienceInsights>('/audience-insights', {
    streamer: streamer || '',
    days,
  });
}

export interface AudienceDemographicsResponse {
  viewerTypes: { label: string; percentage: number }[];
  activityPattern: 'weekend-heavy' | 'weekday-focused' | 'balanced';
  primaryLanguage: string;
  languageConfidence: number;
  peakActivityHours: number[];
  peakHoursMethod?: string;
  chatPenetrationPct?: number | null;
  chatPenetrationReliable?: boolean;
  messagesPer100ViewerMinutes?: number | null;
  viewerMinutes?: number;
  legacyInteractionActivePerAvgViewer?: number | null;
  interactiveRate: number;
  interactionRateActivePerViewer?: number;
  interactionRateActivePerAvgViewer?: number | null;
  interactionRateReliable?: boolean;
  loyaltyScore: number;
  timezone?: string;
  dataQuality?: {
    confidence: 'very_low' | 'low' | 'medium' | 'high';
    sessions?: number;
    method?: 'no_data' | 'low_coverage' | 'real_samples' | string;
    peakMethod?: 'no_data' | 'low_coverage' | 'real_samples' | string;
    coverage?: number;
    sampleCount?: number;
    peakSessionCount?: number;
    peakSessionsWithActivity?: number;
    interactiveSampleCount?: number;
    interactionCoverage?: number;
    chattersCoverage?: number;
    chattersApiCoverage?: number;
    passiveViewerSamples?: number;
    sessionsWithChat?: number;
    chatSessionCoverage?: number;
    viewerSampleCount?: number;
    viewerMinutesSource?: 'real_samples' | 'low_coverage' | string;
  };
}

export async function fetchAudienceDemographics(
  streamer: string | null,
  days: TimeRange
): Promise<AudienceDemographicsResponse> {
  return fetchApi<AudienceDemographicsResponse>('/audience-demographics', {
    streamer: streamer || '',
    days,
    timezone: getBrowserTimezone(),
  });
}

export async function fetchViewerTimeline(
  streamer: string | null,
  days: number
): Promise<ViewerTimelinePoint[]> {
  return fetchApi<ViewerTimelinePoint[]>('/viewer-timeline', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchViewerPresenceTimeline(
  streamer: string | null,
  sessionId: number,
  options: {
    minPresentMin?: number;
    segment?: string;
    search?: string;
    limit?: number;
  } = {}
): Promise<ViewerTimelineSessionResponse> {
  if (!streamer) {
    throw new Error('Streamer required');
  }

  return fetchApi<ViewerTimelineSessionResponse>(
    `/${encodeURIComponent(streamer)}/viewer-timeline`,
    {
      streamer,
      session_id: sessionId,
      min_present_min: options.minPresentMin ?? 0,
      ...(options.segment && options.segment !== 'all' && { segment: options.segment }),
      ...(options.search && { search: options.search }),
      limit: options.limit ?? 200,
    }
  );
}

export async function fetchViewerTimelineProfile(
  streamer: string | null,
  login: string
): Promise<ViewerTimelineProfileResponse> {
  if (!streamer) {
    throw new Error('Streamer required');
  }

  return fetchApi<ViewerTimelineProfileResponse>(
    `/${encodeURIComponent(streamer)}/viewer-timeline/profile`,
    {
      streamer,
      login,
    }
  );
}

export async function fetchCoaching(
  streamer: string,
  days: TimeRange
): Promise<CoachingData> {
  return fetchApi<CoachingData>('/coaching', {
    streamer,
    days,
  });
}

export async function fetchCategoryTimings(
  days: TimeRange,
  source: 'category' | 'tracked' = 'category'
): Promise<CategoryTimings> {
  return fetchApi<CategoryTimings>('/category-timings', { days, source });
}

export async function fetchCategoryActivitySeries(
  days: TimeRange
): Promise<CategoryActivitySeries> {
  return fetchApi<CategoryActivitySeries>('/category-activity-series', { days });
}

export async function fetchMonetization(
  streamer: string | null,
  days: TimeRange
): Promise<MonetizationStats> {
  return fetchApi<MonetizationStats>('/monetization', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchAdsSchedule(
  streamer: string
): Promise<AdsSchedule> {
  return fetchApi<AdsSchedule>('/ads-schedule', {
    streamer,
  });
}

export async function fetchLurkerAnalysis(
  streamer: string | null,
  days: TimeRange
): Promise<LurkerAnalysis> {
  return fetchApi<LurkerAnalysis>('/lurker-analysis', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchRaidRetention(
  streamer: string | null,
  days: TimeRange
): Promise<RaidRetention> {
  return fetchApi<RaidRetention>('/raid-retention', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchRaidAnalytics(
  streamer: string | null,
  days: TimeRange
): Promise<RaidAnalytics> {
  return fetchApi<RaidAnalytics>('/raid-analytics', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchViewerProfiles(
  streamer: string | null,
  days: TimeRange
): Promise<ViewerProfiles> {
  return fetchApi<ViewerProfiles>('/viewer-profiles', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchAudienceSharing(
  streamer: string | null,
  days: TimeRange
): Promise<AudienceSharing> {
  return fetchApi<AudienceSharing>('/audience-sharing', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchViewerDirectory(
  streamer: string | null,
  days: TimeRange,
  sort: ViewerSortField = 'sessions',
  order: 'asc' | 'desc' = 'desc',
  filter: ViewerFilterType = 'all',
  search: string = '',
  page: number = 1,
  perPage: number = 50
): Promise<ViewerDirectory> {
  return fetchApi<ViewerDirectory>('/viewer-directory', {
    streamer: streamer || '',
    days,
    sort,
    order,
    filter,
    ...(search && { search }),
    page,
    per_page: perPage,
  });
}

export async function fetchViewerDetail(
  streamer: string | null,
  login: string,
  days: TimeRange
): Promise<ViewerDetail> {
  return fetchApi<ViewerDetail>('/viewer-detail', {
    streamer: streamer || '',
    login,
    days,
  });
}

export async function fetchViewerSegments(
  streamer: string | null,
  days: TimeRange
): Promise<ViewerSegments> {
  return fetchApi<ViewerSegments>('/viewer-segments', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchChatHypeTimeline(
  streamer: string | null,
  sessionId?: number
): Promise<ChatHypeTimeline> {
  return fetchApi<ChatHypeTimeline>('/chat-hype-timeline', {
    streamer: streamer || '',
    ...(sessionId != null && { session_id: sessionId }),
  });
}

export async function fetchChatContentAnalysis(
  streamer: string | null,
  days: number
): Promise<ChatContentAnalysis> {
  return fetchApi<ChatContentAnalysis>('/chat-content-analysis', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchChatSocialGraph(
  streamer: string | null,
  days: number
): Promise<ChatSocialGraph> {
  return fetchApi<ChatSocialGraph>('/chat-social-graph', {
    streamer: streamer || '',
    days,
  });
}

export async function fetchCategoryLeaderboard(
  streamer: string | null,
  days: number,
  limit: number = 25,
  sort: 'avg' | 'peak' = 'avg',
  excludeExternal = false,
  tier?: string | null
): Promise<CategoryLeaderboard> {
  return fetchApi<CategoryLeaderboard>('/category-leaderboard', {
    streamer: streamer || '',
    days,
    limit,
    sort,
    ...(excludeExternal && { exclude_external: '1' }),
    ...(tier && { tier }),
  });
}

export async function fetchExpOverview(
  streamer: string,
  days: number
): Promise<ExpOverview> {
  return fetchApi<ExpOverview>('/exp/overview', { streamer, days });
}

export async function fetchExpGameBreakdown(
  streamer: string,
  days: number
): Promise<ExpGameBreakdown[]> {
  return fetchApi<ExpGameBreakdown[]>('/exp/game-breakdown', { streamer, days });
}

export async function fetchExpGameTransitions(
  streamer: string,
  days: number
): Promise<ExpGameTransition[]> {
  return fetchApi<ExpGameTransition[]>('/exp/game-transitions', { streamer, days });
}

export async function fetchExpGrowthCurves(
  streamer: string,
  days: number
): Promise<ExpGrowthCurve[]> {
  return fetchApi<ExpGrowthCurve[]>('/exp/growth-curves', { streamer, days });
}

export interface RoadmapItem {
  id: number;
  title: string;
  description: string | null;
  status: 'planned' | 'in_progress' | 'done';
  priority: number;
  created_at: string | null;
  updated_at: string | null;
}

export interface RoadmapData {
  planned: RoadmapItem[];
  in_progress: RoadmapItem[];
  done: RoadmapItem[];
}

export async function fetchRoadmap(): Promise<RoadmapData> {
  return fetchApi<RoadmapData>('/roadmap');
}

export async function fetchStreamReport(
  streamer: string | null,
  sessionId?: number
): Promise<StreamReport> {
  const params: Record<string, string | number> = { streamer: streamer || '' };
  if (sessionId != null) params.session_id = sessionId;
  return fetchApi<StreamReport>('/stream-report', params);
}
