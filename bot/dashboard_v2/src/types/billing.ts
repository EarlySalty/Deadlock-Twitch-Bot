// Plan tier levels
export type PlanTier = 'free' | 'basic' | 'extended';
export type EntitlementId =
  | 'analytics'
  | 'chat.lurker_tax'
  | 'chat.promos.disable'
  | 'raid.priority'
  // Nur Creator Pro: automatisches Posten auf TikTok, Instagram und YouTube.
  | 'social.auto_post';

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
  'social.auto_post',
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
// Preis-Tableau einer Stufe fuer den abgefragten Zyklus (Katalog-Feld `price`).
// Alle Betraege sind Endpreise in Cent (Paragraph 19 UStG).
export interface CatalogPlanPrice {
  cycle_months: number;
  cycle_label: string;
  subtotal_gross_cents: number;
  discount_percent: number;
  discount_cents: number;
  total_gross_cents: number;
  effective_monthly_gross_cents: number;
}

export interface CatalogPlan {
  id: string;
  name: string;
  tier: PlanTier;
  // Rechnerischer Monatspreis in Euro. Kein Katalog-Feld, sondern in
  // `api/billing.ts` aus den Cent-Betraegen abgeleitet.
  price_monthly: number;
  description?: string;
  badge?: string;
  recommended?: boolean;
  monthly_gross_cents?: number;
  yearly_gross_cents?: number;
  price?: CatalogPlanPrice;
  checkout_available?: boolean;
  // `false` heisst: die Stufe steht im Katalog, ist aber nicht kaeuflich,
  // weil ihre Funktionen noch nicht existieren. Kommt aus dem Katalog und
  // zieht `checkout_available` serverseitig mit auf `false`.
  buchbar?: boolean;
  stripe_price_id?: string | null;
  entitlements?: EntitlementId[];
  features: string[];
  // `true` nur, wenn diese Stufe dauerhaft und bezahlt laeuft. Ein Trial oder
  // ein Alt-Tarif zu anderem Preis zaehlt nicht: dort bleibt der Kaufknopf.
  is_current: boolean;
  // Ruhiger Zusatz unter dem Knopf, wenn die Stufe zwar gerade genutzt, aber
  // nicht dauerhaft bezahlt wird (Testphase, alter Tarif).
  hinweis?: string | null;
}

// Trial information derived from plan status
export interface TrialInfo {
  trialEndsAt: string | null;  // ISO date string
  isInTrial: boolean;
  trialDaysRemaining: number;
  onTrialExpiringSoon: boolean;  // true when < 7 days remaining
}

// Aktuell aufgeloestes Abo des eingeloggten Nutzers (Katalog-Feld
// `current_subscription`). Fuer Free-Nutzer ist plan_id === 'raid_free'.
export interface CurrentSubscription {
  plan_id: string | null;
  plan_name: string | null;
  tier: PlanTier;
  is_extended: boolean;
  entitlements?: EntitlementId[];
  expires_at: string | null;
  source: string | null;
}

// Vom Backend gelieferte Management-Pfade (Katalog-Feld `payment`). Bewusst
// server-seitig, damit Frontend keine Billing-URLs hartkodiert.
export interface BillingPaymentPaths {
  invoice_page_path?: string;
  cancel_path?: string;
  checkout_path?: string;
}

// Vollstaendige Antwort von GET /twitch/api/v2/billing/catalog.
export interface BillingCatalog {
  plans: CatalogPlan[];
  current_subscription: CurrentSubscription | null;
  payment: BillingPaymentPaths | null;
}

// Free-Default vs. echtes bezahltes Abo. Nur bei einem bezahlten Plan zeigen wir
// die Abo-Verwaltung (Rechnungen/Portal) an.
export function isActivePaidSubscription(
  sub: CurrentSubscription | null | undefined,
): sub is CurrentSubscription {
  return (
    !!sub &&
    !!sub.plan_id &&
    sub.plan_id !== 'raid_free' &&
    sub.tier !== 'free'
  );
}
