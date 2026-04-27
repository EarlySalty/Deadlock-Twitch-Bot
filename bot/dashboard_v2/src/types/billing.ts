// Plan tier levels
export type PlanTier = 'free' | 'basic' | 'extended';
export type EntitlementId =
  | 'analytics.basic'
  | 'analytics.ai_mini'
  | 'analytics.ai_full'
  | 'analytics.extended'
  | 'chat.lurker_tax'
  | 'chat.promos.disable'
  | 'raid.priority';

// Dashboard view mode (what the user is currently viewing)
export type DashboardView = 'basic' | 'extended';

// Plan status from auth-status API
export interface PlanStatus {
  planId: string | null;
  planName: string | null;
  tier: PlanTier;
  isExtended: boolean;
  expiresAt: string | null;
  source: string | null;
  entitlements: EntitlementId[];
}

// Tab IDs matching the analytics dashboard tabs
// NOTE: These must match TabId from components/layout/TabNavigation.tsx
export type TabId =
  | 'overview'
  | 'social-media'
  | 'schedule'
  | 'category'
  | 'chat'
  | 'growth'
  | 'audience'
  | 'compare'
  | 'viewers'
  | 'coaching'
  | 'monetization'
  | 'experimental'
  | 'ai';

export const ALL_ENTITLEMENTS: EntitlementId[] = [
  'analytics.basic',
  'analytics.ai_mini',
  'analytics.ai_full',
  'analytics.extended',
  'chat.lurker_tax',
  'chat.promos.disable',
  'raid.priority',
];

// Tab visibility configuration per entitlement
export const TAB_ENTITLEMENTS: Partial<Record<TabId, EntitlementId>> = {
  'chat': 'analytics.basic',
  'growth': 'analytics.basic',
  'audience': 'analytics.basic',
  'compare': 'analytics.basic',
  'viewers': 'analytics.extended',
  'coaching': 'analytics.extended',
  'monetization': 'analytics.extended',
  'experimental': 'analytics.extended',
  'ai': 'analytics.ai_mini',
};

// Feature IDs for card-level gating within tabs
export type FeatureId =
  | 'health_scores'
  | 'calendar_heatmap'
  | 'insights_panel'
  | 'stream_timeline_detail'
  | 'chatter_list'
  | 'hype_timeline'
  | 'chat_content_analysis'
  | 'chat_social_graph'
  | 'title_performance'
  | 'raid_retention'
  | 'lurker_analysis'
  | 'audience_sharing'
  | 'viewer_overlap'
  | 'category_timings'
  | 'category_activity_series'
  | 'post_stream_report'
  | 'rankings_extended';

// Feature requirements (cards within tabs that need higher entitlement)
export const FEATURE_ENTITLEMENTS: Record<FeatureId, EntitlementId> = {
  'health_scores': 'analytics.extended',
  'calendar_heatmap': 'analytics.extended',
  'insights_panel': 'analytics.extended',
  'stream_timeline_detail': 'analytics.extended',
  'chatter_list': 'analytics.extended',
  'hype_timeline': 'analytics.extended',
  'chat_content_analysis': 'analytics.extended',
  'chat_social_graph': 'analytics.extended',
  'title_performance': 'analytics.extended',
  'raid_retention': 'analytics.extended',
  'lurker_analysis': 'analytics.extended',
  'audience_sharing': 'analytics.extended',
  'viewer_overlap': 'analytics.extended',
  'category_timings': 'analytics.extended',
  'category_activity_series': 'analytics.extended',
  'post_stream_report': 'analytics.ai_mini',
  'rankings_extended': 'analytics.extended',
};

// Tier hierarchy for comparison
const TIER_ORDER: Record<PlanTier, number> = {
  'free': 0,
  'basic': 1,
  'extended': 2,
};

// Check if a tier meets or exceeds a required tier
export function tierMeetsRequirement(userTier: PlanTier, requiredTier: PlanTier): boolean {
  return TIER_ORDER[userTier] >= TIER_ORDER[requiredTier];
}

// Get display name for tier
export function getTierDisplayName(tier: PlanTier): string {
  switch (tier) {
    case 'free': return 'Free';
    case 'basic': return 'Basic';
    case 'extended': return 'Erweitert';
  }
}

// Billing catalog plan
export interface CatalogPlan {
  id: string;
  name: string;
  tier: PlanTier;
  price_monthly: number;
  entitlements?: EntitlementId[];
  features: string[];
  is_current: boolean;
}

// Trial information derived from plan status
export interface TrialInfo {
  trialEndsAt: string | null;  // ISO date string
  isInTrial: boolean;
  trialDaysRemaining: number;
  onTrialExpiringSoon: boolean;  // true when < 7 days remaining
}
