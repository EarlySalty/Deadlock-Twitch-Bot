// Plan tier levels
export type PlanTier = 'free' | 'basic' | 'extended';
export type EntitlementId =
  | 'analytics'
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
  | 'onboarding'
  | 'overview'
  | 'streams'
  | 'audience'
  | 'growth'
  | 'planning'
  | 'coaching'
  | 'monetization';

export const ALL_ENTITLEMENTS: EntitlementId[] = [
  'analytics',
  'chat.lurker_tax',
  'chat.promos.disable',
  'raid.priority',
];

// Tab visibility configuration per entitlement.
// Fehlt ein Tab hier, ist er frei zugaenglich. Gemischte Tabs erhalten das
// niedrigste Tier ihrer Inhalte; hoehere Karten/Sub-Tabs gaten feiner.
export const TAB_ENTITLEMENTS: Partial<Record<TabId, EntitlementId>> = {
  'audience': 'analytics',
  'coaching': 'analytics',
  'monetization': 'analytics',
};

// Feature IDs for card-level gating within tabs
export type FeatureId =
  | 'health_scores'
  | 'calendar_heatmap'
  | 'insights_panel'
  | 'hype_timeline'
  | 'chat_content_analysis'
  | 'chat_social_graph'
  | 'title_performance'
  | 'raid_retention'
  | 'lurker_analysis'
  | 'viewer_overlap'
  | 'category_timings'
  | 'post_stream_report'
  | 'rankings_extended';

// Feature requirements (cards within tabs that need higher entitlement)
export const FEATURE_ENTITLEMENTS: Record<FeatureId, EntitlementId> = {
  'health_scores': 'analytics',
  'calendar_heatmap': 'analytics',
  'insights_panel': 'analytics',
  'hype_timeline': 'analytics',
  'chat_content_analysis': 'analytics',
  'chat_social_graph': 'analytics',
  'title_performance': 'analytics',
  'raid_retention': 'analytics',
  'lurker_analysis': 'analytics',
  'viewer_overlap': 'analytics',
  'category_timings': 'analytics',
  'post_stream_report': 'analytics',
  'rankings_extended': 'analytics',
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
