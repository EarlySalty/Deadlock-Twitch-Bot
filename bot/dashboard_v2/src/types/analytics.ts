// Core Analytics Types for Twitch Dashboard

export interface StreamSession {
  id: number;
  date: string;
  startTime: string;
  duration: number; // seconds
  startViewers: number;
  peakViewers: number;
  endViewers: number;
  avgViewers: number;
  retention5m: number;
  retention10m: number;
  retention20m: number;
  dropoffPct: number;
  totalChatterSessions: number;
  firstTimeChatters: number;
  returningChatters: number;
  followersStart: number;
  followersEnd: number;
  title: string;
}

export interface DailyStats {
  date: string;
  hoursWatched: number;
  airtime: number;
  avgViewers: number;
  peakViewers: number;
  followerDelta: number;
  totalChatterSessions: number;
  streamCount: number;
}

export interface MonthlyStats {
  year: number;
  month: number;
  monthLabel: string;
  totalHoursWatched: number;
  totalAirtime: number;
  avgViewers: number;
  peakViewers: number;
  followerDelta: number;
  totalChatterSessions: number;
  streamCount: number;
}

export interface WeekdayStats {
  weekday: number; // 0-6 (Sunday-Saturday)
  weekdayLabel: string;
  streamCount: number;
  avgHours: number;
  avgViewers: number;
  avgPeak: number;
  totalFollowers: number;
}

export interface HourlyHeatmapData {
  weekday: number;
  hour: number;
  streamCount: number;
  avgViewers: number;
  avgPeak: number;
}

export interface MessageTypeStat {
  type: string;
  count: number;
  percentage: number;
}

export interface HourlyActivityStat {
  hour: number;
  count: number;
}

export interface CalendarHeatmapData {
  date: string;
  value: number; // hours watched or stream count
  streamCount: number;
  hoursWatched: number;
}

export interface RawChatStatus {
  available: boolean;
  lastMessageAt: string | null;
  gapStart: string | null;
  suspectedIngestionIssue: boolean;
  backfillState: string;
  note: string | null;
  lastInsertOkAt?: string | null;
  lastInsertErrorAt?: string | null;
}

export interface ViewerWindowMetadata {
  windowPresenceSessions?: number;
  windowPresenceMessages?: number;
  windowRawMessages?: number;
  hasRawMessages?: boolean;
  presenceOnlyInWindow?: boolean;
  messageGapNote?: string | null;
}

export interface SessionEvent {
  channel_updates: Array<{
    at: string;
    title: string | null;
    game: string | null;
    language: string | null;
  }>;
  raids: Array<{
    at: string;
    channel: string;
    viewers: number;
    direction: 'incoming' | 'outgoing';
  }>;
  follows_per_minute: Array<{
    minute: string;
    count: number;
  }>;
}

export interface ChatAnalytics {
  totalMessages: number;
  totalChatterSessions: number;
  /** Backwards compatibility alias from API */
  uniqueChatters?: number;
  totalTrackedViewers?: number;
  firstTimeChatters: number;
  returningChatters: number;
  returningTrackedViewers?: number;
  coreLoyalViewers?: number;
  silentCoreLoyalViewers?: number;
  coreLoyalViewerRate?: number;
  loyaltySessionThreshold?: number;
  messagesPerMinute: number;
  messagesPer100ViewerMinutes?: number | null;
  messagesPer100ViewerMinutesPercentile?: number | null;
  messagesPer100ViewerMinutesMedian?: number | null;
  messagesPer100ViewerMinutesP25?: number | null;
  messagesPer100ViewerMinutesP75?: number | null;
  messagesPer100ViewerMinutesBenchmarkSessions?: number;
  viewerMinutes?: number;
  chatterReturnRate: number;
  chatPenetrationPct?: number | null;
  chatPenetrationReliable?: boolean;
  legacyInteractionActivePerAvgViewer?: number | null;
  // Legacy fields (kept one release for compatibility)
  interactionRateActivePerViewer?: number;
  interactionRateActivePerAvgViewer?: number | null;
  interactionRateReliable?: boolean;
  activeRatio?: number;
  activeChatters?: number;
  lurkerRatio?: number;
  lurkerCount?: number;
  avgMessagesPerChatter?: number;
  timezone?: string;
  dataQuality?: {
    method?: 'no_data' | 'low_coverage' | 'real_samples' | string;
    coverage?: number;
    sampleCount?: number;
    confidence?: 'very_low' | 'low' | 'medium' | 'high';
    sessions?: number;
    sessionsWithChat?: number;
    chatSessionCoverage?: number;
    chattersCoverage?: number;
    chattersApiCoverage?: number;
    passiveViewerSamples?: number;
    viewerSampleCount?: number;
    viewerMinutesSource?: 'real_samples' | 'low_coverage' | string;
  };
  topChatters: ChatterStats[];
  messageTypes: MessageTypeStat[];
  hourlyActivity: HourlyActivityStat[];
  rawChatStatus?: RawChatStatus;
}

export interface ChatterStats {
  login: string;
  totalMessages: number;
  totalSessions: number;
  firstSeen: string;
  lastSeen: string;
  loyaltyScore: number;
}

export interface ViewerOverlap {
  streamerA: string;
  streamerB: string;
  sharedChatters: number;
  totalChattersA: number;
  totalChattersB: number;
  overlapPercentage?: number;
  overlapAtoB?: number;
  overlapBtoA?: number;
  jaccard?: number;
}

export interface CategoryComparison {
  yourStats: {
    avgViewers: number;
    peakViewers: number;
    retention10m: number;
    chatHealth: number;
  };
  categoryAvg: {
    avgViewers: number;
    peakViewers: number;
    retention10m: number;
    chatHealth: number;
  };
  percentiles: {
    avgViewers: number;
    peakViewers?: number;
    retention10m: number;
    chatHealth: number;
  };
  categoryRank?: number;
  categoryTotal?: number;
}

export interface TagPerformance {
  tagName: string;
  usageCount: number;
  avgViewers: number;
  avgRetention10m: number;
  avgFollowerGain: number;
}

export interface GrowthMetrics {
  followerGrowthRate: number;
  viewerGrowthRate: number;
  newViewerRate: number;
  returningViewerRate: number;
  weeklyTrend: TrendPoint[];
}

export interface TrendPoint {
  date: string;
  value: number;
  change: number;
}

export interface HealthScore {
  total: number;
  reach: number;
  retention: number;
  engagement: number;
  growth: number;
  monetization: number;
  network: number;
}

export interface DashboardOverview {
  streamer: string;
  days: number;
  empty?: boolean;
  error?: string;
  scores: HealthScore;
  summary: {
    avgViewers: number;
    peakViewers: number;
    totalHoursWatched: number;
    totalAirtime: number;
    followersDelta: number;
    followersGained?: number;
    followersPerHour: number;
    followersGainedPerHour?: number;
    retention10m: number;
    retentionReliable?: boolean;
    totalChatterSessions: number;
    streamCount: number;
    // Neue Trend-Felder
    avgViewersTrend?: number;      // % Änderung vs. Vorperiode
    peakViewersTrend?: number;
    followersTrend?: number;
    retentionTrend?: number;
  };
  sessions: StreamSession[];
  findings: Insight[];
  actions: ActionItem[];
  correlations: {
    durationVsViewers: number;
    chatVsRetention: number;
  };
  network: {
    sent: number;
    received: number;
    sentViewers: number;
  };
  // Category Ranking
  categoryRank?: number;
  categoryTotal?: number;
  // Neue Audience Insights
  audienceInsights?: AudienceInsights;
}

export interface Insight {
  type: 'pos' | 'neg' | 'warn' | 'info';
  title: string;
  text: string;
}

export interface ActionItem {
  tag: string;
  text: string;
  priority: 'high' | 'medium' | 'low';
}

export interface StreamerInfo {
  login: string;
  displayName: string;
  isPartner: boolean;
  isOnDiscord: boolean;
  lastDeadlockStream: string | null;
}

export interface RankingEntry {
  rank: number;
  login: string;
  value: number;
  trend: 'up' | 'down' | 'same';
  trendValue: number;
}

export interface AudienceBreakdown {
  interactive: number;
  passive: number;
  interactionRate: number;
  estimatedLanguage: string;
  languageConfidence: number;
}

// Watch Time Distribution - Wie lange bleiben Viewer?
export interface WatchTimeDistribution {
  under5min: number;      // Schnelle Absprünge (%)
  min5to15: number;       // Kurze Sessions (%)
  min15to30: number;      // Mittlere Sessions (%)
  min30to60: number;      // Längere Sessions (%)
  over60min: number;      // Loyale Zuschauer (%)
  avgWatchTime: number;   // Durchschnittliche Watch Time in Minuten
  medianWatchTime: number; // Median Watch Time in Minuten
  dataQuality?: {
    method: 'no_data' | 'low_coverage' | 'real_samples' | string;
    coverage?: number;
    sample_count?: number;
    viewer_base_count?: number;
    required_min_samples?: number;
    required_min_coverage?: number;
    confidence?: 'very_low' | 'low' | 'medium' | 'high';
    sessions?: number;
  };
  sessionCount?: number;
  previous?: {
    under5min: number;
    min5to15: number;
    min15to30: number;
    min30to60: number;
    over60min: number;
    avgWatchTime: number;
    medianWatchTime: number;
    sessionCount?: number;
  };
  deltas?: {
    under5min: number | null;
    min5to15: number | null;
    min15to30: number | null;
    min30to60: number | null;
    over60min: number | null;
    avgWatchTime: number | null;
  };
}

// Follower Conversion Funnel - Von Viewer zu Follower
export interface FollowerFunnel {
  uniqueViewers: number;        // Einzigartige Viewer im Zeitraum
  returningViewers: number;     // Wiederkehrende Viewer (nicht gefolgt)
  newFollowers: number;         // Gewonnene Follower (nur positive Session-Deltas)
  netFollowerDelta: number;     // Netto-Änderung (kann negativ sein: Follows - Unfollows)
  conversionRate: number;       // newFollowers / uniqueViewers * 100
  avgTimeToFollow: number;      // Durchschnittliche Zeit bis Follow (Minuten)
  followersBySource: {
    organic: number;            // Direkt über Stream
    raids: number;              // Über Raids
    hosts: number;              // Über Hosts
    other: number;              // Sonstige
  };
}

// Erweiterte Tag Performance mit Trends
export interface TagPerformanceExtended extends TagPerformance {
  trend: 'up' | 'down' | 'stable';
  trendValue: number;           // % Änderung
  bestTimeSlot: string;         // z.B. "18:00-22:00"
  avgStreamDuration: number;    // Durchschnittliche Stream-Dauer mit diesem Tag
  categoryRank: number;         // Rang in der Kategorie für diesen Tag
}

// Title Performance - Welche Titel performen besser?
export interface TitlePerformance {
  title: string;
  usageCount: number;
  avgViewers: number;
  avgRetention10m: number;
  avgFollowerGain: number;
  peakViewers: number;
  keywords: string[];           // Extrahierte Keywords
}

// Kombinierte Funnel & Distribution Daten
export interface AudienceInsights {
  watchTimeDistribution: WatchTimeDistribution;
  followerFunnel: FollowerFunnel;
  tagPerformance: TagPerformanceExtended[];
  titlePerformance: TitlePerformance[];
  // Trends im Vergleich zur Vorperiode
  trends: {
    watchTimeChange: number;      // % Änderung avg watch time
    conversionChange: number;     // % Änderung conversion rate
    viewerReturnRate: number;     // % der Viewer die zurückkommen
    viewerReturnChange: number;   // % Änderung return rate
  };
  distinctViewers?: number;
  returnRateMethod?: string;
}

// API Response Types
export interface ApiResponse<T> {
  data: T;
  error?: string;
  empty?: boolean;
}

// Viewer Timeline (from twitch_stats_tracked)
export interface ViewerTimelinePoint {
  timestamp: string;
  avgViewers: number;
  peakViewers: number;
  minViewers: number;
  samples: number;
}

// Category Leaderboard (from twitch_stats_category)
export interface LeaderboardEntry {
  rank: number;
  streamer: string;
  avgViewers: number;
  peakViewers: number;
  isPartner: boolean;
  isYou?: boolean;
}

export interface CategoryLeaderboard {
  leaderboard: LeaderboardEntry[];
  totalStreamers: number;
  yourRank: number | null;
}

// Coaching Types

export interface CoachingEfficiency {
  viewerHoursPerStreamHour: number;
  categoryAvg: number;
  topPerformers: { streamer: string; ratio: number }[];
  percentile: number;
  totalStreamHours: number;
  totalViewerHours: number;
  growthPer10Hours?: number;
  growthCategoryAvg?: number;
  growthTopPerformers?: { streamer: string; value: number }[];
  growthPercentile?: number;
}

export interface CoachingTitleEntry {
  title: string;
  avgViewers: number;
  peakViewers: number;
  chatters: number;
  usageCount: number;
}

export interface CoachingTitleAnalysis {
  yourTitles: CoachingTitleEntry[];
  categoryTopTitles: { title: string; streamer: string; avgViewers: number }[];
  yourMissingPatterns: string[];
  topPerformerPatterns: string[];
  varietyPct: number;
  uniqueTitleCount: number;
  totalSessionCount: number;
  avgPeerVarietyPct: number;
  peerVariety: { streamer: string; uniqueTitles: number; totalSessions: number; varietyPct: number }[];
}

export interface CoachingSweetSpot {
  weekday: number;
  hour: number;
  categoryViewers: number;
  competitors: number;
  opportunityScore: number;
}

export interface CoachingScheduleOptimizer {
  sweetSpots: CoachingSweetSpot[];
  yourCurrentSlots: { weekday: number; hour: number; count: number }[];
  competitionHeatmap: { weekday: number; hour: number; competitors: number; categoryViewers: number }[];
}

export interface CoachingDurationBucket {
  label: string;
  streamCount: number;
  avgViewers: number;
  avgChatters: number;
  avgRetention5m: number;
  efficiencyRatio: number;
}

export interface CoachingDurationAnalysis {
  buckets: CoachingDurationBucket[];
  optimalLabel: string;
  currentAvgHours: number;
  correlation: number;
}

export interface CoachingCrossCommunity {
  totalUniqueChatters: number;
  chatterSources: { sourceStreamer: string; sharedChatters: number; percentage: number }[];
  isolatedChatters: number;
  isolatedPercentage: number;
  ecosystemSummary: string;
}

export interface CoachingTagOptimization {
  yourTags: { tags: string; avgViewers: number; usageCount: number }[];
  categoryBestTags: { tags: string; avgViewers: number; streamerCount: number }[];
  missingHighPerformers: string[];
  underperformingTags: string[];
}

export interface CoachingRetention {
  your5mRetention: number;
  category5mRetention: number;
  yourViewerCurve: { minute: number; avgViewerPct: number }[];
  topPerformerCurve: { minute: number; avgViewerPct: number }[];
  criticalDropoffMinute: number;
}

export interface CoachingDoubleStream {
  detected: boolean;
  count: number;
  occurrences: { date: string; sessionCount: number; avgViewers: number }[];
  singleDayAvg: number;
  doubleDayAvg: number;
}

export interface CoachingRecommendation {
  priority: 'critical' | 'high' | 'medium' | 'low';
  category: string;
  title: string;
  description: string;
  estimatedImpact: string;
  evidence: string;
  icon: string;
}

export interface CoachingChatConcentration {
  totalChatters: number;
  totalMessages: number;
  msgsPerChatter: number;
  loyaltyBuckets: Record<string, { count: number; pct: number; messages: number }>;
  topChatters: { login: string; messages: number; sessions: number; sharePct: number; cumulativePct: number }[];
  concentrationIndex: number;
  top1Pct: number;
  top3Pct: number;
  ownOneTimerPct: number;
  avgPeerOneTimerPct: number;
}

export interface CoachingRaidPartner {
  login: string;
  sentCount: number;
  sentAvgViewers: number;
  receivedCount: number;
  receivedAvgViewers: number;
  reciprocity: 'mutual' | 'sentOnly' | 'receivedOnly';
  balance: number;
}

export interface CoachingRaidNetwork {
  totalSent: number;
  totalReceived: number;
  totalSentViewers: number;
  totalReceivedViewers: number;
  avgSentViewers: number;
  avgReceivedViewers: number;
  reciprocityRatio: number;
  mutualPartners: number;
  totalPartners: number;
  partners: CoachingRaidPartner[];
}

export interface CoachingPeerEntry {
  login: string;
  sessions: number;
  avgViewers: number;
  maxPeak: number;
  avgHours: number;
  avgChatters: number;
  retention5m: number;
  totalHours: number;
  followsGained: number;
  uniqueTitles: number;
  titleVariety: number;
}

export interface CoachingPeerComparison {
  ownData: CoachingPeerEntry | null;
  ownRank: number;
  totalStreamers: number;
  similarPeers: CoachingPeerEntry[];
  aspirationalPeers: CoachingPeerEntry[];
  metricsRanked: Record<string, { rank: number; total: number; value: number }>;
  gapToNext: { login: string; avgViewersDiff: number; chatDiff: number; retentionDiff: number } | null;
}

export interface CoachingCompetitionHourly {
  hour: number;
  activeStreamers: number;
  avgViewers: number;
  avgPeak: number;
  opportunityScore: number;
  yourData: { count: number; avgViewers: number; avgPeak: number; avgChatters: number } | null;
}

export interface CoachingCompetitionWeekly {
  weekday: number;
  weekdayLabel: string;
  activeStreamers: number;
  avgViewers: number;
  yourData: { count: number; avgViewers: number; avgPeak: number } | null;
}

export interface CoachingCompetitionDensity {
  hourly: CoachingCompetitionHourly[];
  weekly: CoachingCompetitionWeekly[];
  sweetSpots: CoachingCompetitionHourly[];
}

export interface CoachingData {
  streamer: string;
  days: number;
  empty?: boolean;
  efficiency: CoachingEfficiency;
  titleAnalysis: CoachingTitleAnalysis;
  scheduleOptimizer: CoachingScheduleOptimizer;
  durationAnalysis: CoachingDurationAnalysis;
  crossCommunity: CoachingCrossCommunity;
  tagOptimization: CoachingTagOptimization;
  retentionCoaching: CoachingRetention;
  doubleStreamDetection: CoachingDoubleStream;
  chatConcentration: CoachingChatConcentration;
  raidNetwork: CoachingRaidNetwork;
  peerComparison: CoachingPeerComparison;
  competitionDensity: CoachingCompetitionDensity;
  recommendations: CoachingRecommendation[];
  aiSummary: string | null;
}

export interface AdsSchedule {
  current: {
    next_ad_at: string | null;
    last_ad_at: string | null;
    duration: number | null;
    preroll_free_time: number | null;
    snooze_count: number | null;
    snooze_refresh_at: string | null;
    snapshot_at: string;
  } | null;
  history: Array<{
    snapshot_at: string;
    next_ad_at: string | null;
    duration: number | null;
    preroll_free_time: number | null;
  }>;
}

export type TimeRange = 7 | 30 | 90 | 365;
export type { TabId } from '@/components/layout/TabNavigation';

// --- Lurker Analysis ---
export interface LurkerEntry {
  login: string;
  lurkSessions: number;
  firstSeen: string | null;
  lastSeen: string | null;
}

export interface LurkerAnalysis {
  dataAvailable: boolean;
  message?: string;
  regularLurkers: LurkerEntry[];
  lurkerStats: {
    ratio: number;
    avgSessions: number;
    totalLurkers: number;
    totalViewers: number;
  };
  conversionStats: {
    rate: number;
    eligible: number;
    converted: number;
  };
}

// --- Viewer Profiles ---
export interface ViewerProfiles {
  dataAvailable: boolean;
  message?: string;
  profiles: {
    exclusive: number;
    loyalMulti: number;
    casual: number;
    explorer: number;
    passive: number;
    total: number;
  };
  exclusivityDistribution: Array<{
    streamerCount: number;
    viewerCount: number;
  }>;
}

// --- Audience Sharing ---
export interface AudienceSharingEntry {
  streamer: string;
  sharedViewers: number;
  inflow: number;
  outflow: number;
  jaccardSimilarity: number;
}

export interface AudienceSharing {
  dataAvailable: boolean;
  message?: string;
  current: AudienceSharingEntry[];
  timeline: Array<{
    month: string;
    streamer: string;
    sharedViewers: number;
  }>;
  totalUniqueViewers: number;
  dataQuality: {
    months: number;
    minSharedFilter: number;
  };
}

// --- Raid Retention ---
export interface RaidRetentionEntry {
  raidId: number;
  toBroadcaster: string;
  viewersSent: number;
  executedAt: string;
  chattersAt5m: number | null;
  chattersAt15m: number | null;
  chattersAt30m: number | null;
  retention30mPct: number;
  newChatters: number | null;
  chatterConversionPct: number;
  knownFromRaider: number | null;
}

export interface RaidRetention {
  dataAvailable: boolean;
  message?: string;
  summary: {
    avgRetentionPct: number;
    avgConversionPct: number;
    totalNewChatters: number;
    raidCount: number;
  };
  raids: RaidRetentionEntry[];
}

// --- Incoming Raid Analytics ---
export interface RaidImpact {
  viewers_before: number | null;
  viewers_peak_after: number | null;
  boost_pct: number | null;
  retention_5m_pct: number | null;
  retention_15m_pct: number | null;
  retention_30m_pct: number | null;
  follows_after_raid: number;
}

export interface IncomingRaid {
  from_channel: string;
  detected_at: string;
  viewers_sent: number;
  classification: string;
  unraid_seen: boolean;
  impact: RaidImpact;
}

export interface IncomingSummary {
  total_raids_received: number;
  avg_viewers_received: number;
  avg_boost_pct: number | null;
  avg_retention_15m: number | null;
  best_raider: string | null;
  raid_balance: { sent: number; received: number };
}

export interface RaidAnalytics {
  per_source: Array<{
    from_channel: string;
    raids_received: number;
    avg_viewers_sent: number;
    avg_new_chatters: number;
    avg_retention_30m: number | null;
    follows_attributed: number;
    conversion_rate: number | null;
    known_audience_overlap: number | null;
  }>;
  follow_attribution: {
    total_follows: number;
    raid_follows: number;
    organic_follows: number;
    raid_conversion_rate: number | null;
  } | null;
  retention_curves: Array<{
    raid_id: number;
    from: string;
    viewers_sent: number;
    new_chatters: number;
    retention_curve: {
      plus5m: number;
      plus15m: number;
      plus30m: number;
    };
  }>;
  incoming_raids: IncomingRaid[];
  incoming_summary: IncomingSummary | null;
  window_days: number;
  dataQuality: {
    botFilterApplied: boolean;
    retentionCurveSampleSize: number;
    perSourceUsesFullWindow: boolean;
    raidMetricBatchSize: number;
  };
}

// Category Timings (Median-basiert)
export interface TimingSlot {
  median: number | null;
  p25: number | null;
  p75: number | null;
  streamer_count: number;
  sample_count: number;
}

export interface HourlyTimingSlot extends TimingSlot {
  hour: number;
}

export interface WeeklyTimingSlot extends TimingSlot {
  weekday: number;
  label: string;
}

export interface CategoryTimings {
  hourly: HourlyTimingSlot[];
  weekly: WeeklyTimingSlot[];
  total_streamers: number;
  window_days: number;
  method: string;
}

export interface CategoryActivitySeriesRow {
  label: string;
  categoryAvg: number | null;
  trackedAvg: number | null;
  categoryPeak: number | null;
  trackedPeak: number | null;
  categorySamples: number;
  trackedSamples: number;
}

export interface CategoryActivityHourlyRow extends CategoryActivitySeriesRow {
  hour: number;
}

export interface CategoryActivityWeeklyRow extends CategoryActivitySeriesRow {
  weekday: number;
}

export interface CategoryActivitySeries {
  hourly: CategoryActivityHourlyRow[];
  weekly: CategoryActivityWeeklyRow[];
  windowDays: number;
  source: string;
}

// Monetization & Hype Train
export interface WorstAd {
  started_at: string;
  duration_s: number;
  drop_pct: number;
  is_automatic: boolean;
  min_into_stream?: number;
  recovery_min?: number | null;
}

export interface AdBucketData {
  avg_drop: number | null;
  count: number;
}

export interface RecoveryBucketData {
  avg_recovery_min: number | null;
  count: number;
}

export interface AutoVsManual {
  auto_avg_drop: number | null;
  manual_avg_drop: number | null;
  auto_count: number;
  manual_count: number;
}

export interface MonetizationStats {
  ads: {
    total: number;
    auto: number;
    manual: number;
    sessions_with_ads: number;
    avg_duration_s: number;
    avg_viewer_drop_pct: number | null;
    worst_ads: WorstAd[];
    duration_impact?: Record<string, AdBucketData>;
    position_impact?: Record<string, AdBucketData>;
    auto_vs_manual?: AutoVsManual;
    best_ad_time?: string | null;
    avg_recovery_min?: number | null;
    recovery_by_duration?: Record<string, RecoveryBucketData>;
    recommendations?: string[];
  };
  hype_train: {
    total: number;
    avg_level: number;
    max_level: number;
    avg_duration_s: number;
  };
  bits: {
    total: number;
    cheer_events: number;
  };
  subs: {
    total_events: number;
    gifted: number;
  };
  window_days: number;
}

// ── Viewer Analytics Types ──

export interface ViewerEntry {
  login: string;
  totalSessions: number;
  totalMessages: number;
  firstSeen: string;
  lastSeen: string;
  daysSinceLastSeen: number;
  otherChannels: number;
  topOtherChannels: string[];
  category: 'dedicated' | 'regular' | 'casual' | 'lurker' | 'new';
  avgMessagesPerSession: number;
  isLurker: boolean;
}

export interface ViewerEntry extends ViewerWindowMetadata {}

export interface ViewerDirectorySummary {
  totalViewers: number;
  activeViewers: number;
  lurkers: number;
  exclusiveViewers: number;
  sharedViewers: number;
  avgSessionsPerViewer: number;
  avgOtherChannels: number;
}

export interface ViewerDirectory {
  viewers: ViewerEntry[];
  total: number;
  page: number;
  perPage: number;
  days?: number;
  summary: ViewerDirectorySummary;
  rawChatStatus?: RawChatStatus;
}

export type ViewerSortField = 'sessions' | 'messages' | 'last_seen' | 'other_channels' | 'first_seen';
export type ViewerFilterType = 'all' | 'active' | 'lurker' | 'exclusive' | 'shared' | 'new' | 'churned';

export interface ViewerDetailOverview {
  totalSessions: number;
  totalMessages: number;
  firstSeen: string;
  lastSeen: string;
  category: string;
  isLurker: boolean;
}

export interface ViewerDetailOverview extends ViewerWindowMetadata {}

export interface ViewerActivityDay {
  date: string;
  sessions: number;
  messages: number;
}

export interface ViewerCrossChannel {
  streamer: string;
  sessions: number;
  messages: number;
  firstSeen: string;
  lastSeen: string;
  overlap: 'before' | 'after' | 'unknown';
}

export interface ViewerChatPatterns {
  peakHours: number[];
  avgMessagesPerSession: number;
  mostActiveDay: string;
  messagesTrend: 'increasing' | 'decreasing' | 'stable' | 'insufficient_data';
}

export interface ViewerDetail {
  login: string;
  days?: number;
  overview: ViewerDetailOverview;
  activityTimeline: ViewerActivityDay[];
  crossChannelPresence: ViewerCrossChannel[];
  chatPatterns: ViewerChatPatterns;
  personality?: ViewerPersonality;
  rawChatStatus?: RawChatStatus;
}

export interface SegmentData {
  count: number;
  pct: number;
  avgMessages: number;
  avgSessions: number;
}

export interface AtRiskViewer {
  login: string;
  sessions: number;
  messages: number;
  daysSinceLastSeen: number;
  category: string;
  recentlySeenAt?: string[];
}

export interface ChurnRisk {
  atRisk: number;
  recentlyChurned: number;
  atRiskViewers: AtRiskViewer[];
}

export interface SharedChannel {
  streamer: string;
  sharedCount: number;
  direction: 'bidirectional' | 'outgoing' | 'incoming';
}

export interface CrossChannelStats {
  exclusiveViewersPct: number;
  avgOtherChannels: number;
  topSharedChannels: SharedChannel[];
}

export interface ViewerSegments {
  days: number;
  segments: Record<string, SegmentData>;
  churnRisk: ChurnRisk;
  crossChannelStats: CrossChannelStats;
}

// ── Chat Deep Analysis Types ──

export interface HypeTimelineEntry {
  minute: number;
  messages: number;
  chatters: number;
  viewers: number;
  isSpike: boolean;
}

export interface HypeSpike {
  minute: number;
  messages: number;
  multiplier: number;
}

export interface HypeCorrelation {
  chatViewerR: number;
  interpretation: string;
  chatLeadsViewers: boolean;
  lagMinutes: number;
}

export interface HypeRecentSession {
  id: number;
  date: string;
  title: string;
  avgMPM: number;
  peakMPM: number;
}

export interface ChatHypeTimeline {
  sessionId: number;
  sessionTitle: string;
  startedAt: string;
  duration: number;
  avgMPM: number;
  peakMPM: number;
  timeline: HypeTimelineEntry[];
  spikes: HypeSpike[];
  correlation: HypeCorrelation;
  recentSessions: HypeRecentSession[];
  rawChatStatus?: RawChatStatus;
}

export interface HeroMention {
  hero: string;
  count: number;
  pct: number;
}

export interface SentimentBucket {
  bucket: string;
  positive: number;
  negative: number;
  score: number;
}

export interface OverallSentiment {
  score: number;
  label: string;
  trend: string;
  totalAnalyzed: number;
  positiveCount: number;
  negativeCount: number;
}

export interface BackseatData {
  count: number;
  pct: number;
  examples: string[];
}

export interface EngagementDepth {
  reaction: number;
  reactionPct: number;
  short: number;
  shortPct: number;
  discussion: number;
  discussionPct: number;
  total: number;
  avgWordCount: number;
}

export interface ChatContentAnalysis {
  heroMentions: HeroMention[];
  topicBreakdown: Record<string, number>;
  sentimentTimeline: SentimentBucket[];
  overallSentiment: OverallSentiment;
  backseat: BackseatData;
  engagementDepth: EngagementDepth;
  rawChatStatus?: RawChatStatus;
}

export interface SocialHub {
  login: string;
  mentionsSent: number;
  mentionsReceived: number;
  score: number;
}

export interface SocialPair {
  from: string;
  to: string;
  count: number;
}

export interface MentionDistribution {
  mentionedOnce: number;
  mentioned2to5: number;
  mentioned5plus: number;
}

export interface ChatSocialGraph {
  totalMentions: number;
  uniqueMentioners: number;
  uniqueMentioned: number;
  hubs: SocialHub[];
  topPairs: SocialPair[];
  mentionDistribution: MentionDistribution;
  rawChatStatus?: RawChatStatus;
}

export interface ViewerPersonality {
  primary: string;
  distribution: Record<string, number>;
}

// ========= Experimental (Labor) Analytics =========

export interface ExpOverview {
  totalSessions: number;
  gamesPlayed: number;
  avgViewers: number;
  bestGame: string;
  bestGameAvgViewers: number;
}

export interface ExpGameBreakdown {
  game: string;
  sessions: number;
  avgViewers: number;
  peakViewers: number;
  avgDurationMin: number;
  avgFollowerDelta: number;
}

export interface ExpGameTransition {
  fromGame: string;
  toGame: string;
  count: number;
  avgViewersBefore: number;
  avgViewersAfter: number;
  viewerDelta: number;
}

export interface ExpGrowthCurve {
  game: string;
  minuteFromStart: number;
  avgViewers: number;
  sampleCount: number;
}

// ========= KI Analyse (AI Analysis) =========

export interface AIAnalysisPoint {
  number: number;
  priority: 'kritisch' | 'hoch' | 'mittel';
  title: string;
  analysis: string;
  action: string;
  expectedImpact: string;
}

export interface AIAnalysisResult {
  id?: number | null;
  streamer: string;
  days: number;
  gameFilter?: 'deadlock' | 'all';
  generatedAt: string;
  points: AIAnalysisPoint[];
  dataSnapshot: {
    streamCount: number;
    totalHours: number;
    avgViewers: number;
    peakViewers: number;
    followersGained: number;
    avgRetention10m: number;
    avgDropoffPct: number;
    avgChatters: number;
  };
}

export interface AIHistoryEntry extends AIAnalysisResult {
  id: number;
  model: string;
  kritischCount: number;
  hochCount: number;
  mittelCount: number;
}
