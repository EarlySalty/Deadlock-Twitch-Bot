import { createContext, useContext, useState, useEffect, useMemo, type ReactNode } from 'react';
import type { EntitlementId, PlanTier, DashboardView, PlanStatus, TabId, FeatureId, TrialInfo } from '../types/billing';
import { ALL_ENTITLEMENTS, TAB_ENTITLEMENTS, FEATURE_ENTITLEMENTS } from '../types/billing';

interface PlanContextType {
  tier: PlanTier;
  plan: PlanStatus | null;
  entitlements: EntitlementId[];
  view: DashboardView;
  setView: (view: DashboardView) => void;
  isDemoMode: boolean;
  isPreviewMode: boolean;
  hasEntitlement: (entitlement: EntitlementId) => boolean;
  canAccessTab: (tabId: TabId) => boolean;
  isTabLocked: (tabId: TabId) => boolean;
  isFeatureLocked: (featureId: FeatureId) => boolean;
  hasFullAccess: boolean;
  isAdmin: boolean;
  trialInfo: TrialInfo | null;
}

const PlanContext = createContext<PlanContextType | null>(null);

interface PlanProviderProps {
  children: ReactNode;
  plan: PlanStatus | null;
  isAdmin: boolean;
  isLocalhost: boolean;
  isDemoMode: boolean;
}

export function PlanProvider({ children, plan, isAdmin, isLocalhost, isDemoMode }: PlanProviderProps) {
  const hasFullAccess = isAdmin || isLocalhost || isDemoMode;
  const tier: PlanTier = hasFullAccess ? 'extended' : (plan?.tier ?? 'free');
  const entitlements = useMemo<EntitlementId[]>(
    () => (hasFullAccess ? ALL_ENTITLEMENTS : (plan?.entitlements ?? [])),
    [hasFullAccess, plan?.entitlements],
  );
  const hasExtendedAnalytics = hasFullAccess || entitlements.includes('analytics.extended');
  const [view, setView] = useState<DashboardView>(
    hasExtendedAnalytics ? 'extended' : 'basic'
  );
  // Sync view when tier changes after mount (e.g. auth loads async)
  useEffect(() => {
    if (hasExtendedAnalytics) {
      setView('extended');
    }
  }, [hasExtendedAnalytics]);

  const isPreviewMode = view === 'extended' && !hasExtendedAnalytics;

  // Compute trial info from plan snapshot
  const trialInfo = useMemo<TrialInfo | null>(() => {
    // Try to get trial_end_at from plan metadata or source field
    const trialEndsAt = (plan as { trial_end_at?: string } | null)?.trial_end_at ?? null;

    if (!trialEndsAt) return null;

    const now = new Date();
    const trialEnd = new Date(trialEndsAt);
    const diffMs = trialEnd.getTime() - now.getTime();
    const diffDays = Math.ceil(diffMs / (1000 * 60 * 60 * 24));

    if (diffDays < 0) return null; // trial already expired

    return {
      trialEndsAt,
      isInTrial: true,
      trialDaysRemaining: diffDays,
      onTrialExpiringSoon: diffDays < 7,
    };
  }, [plan]);

  const value = useMemo<PlanContextType>(() => ({
    tier,
    plan,
    entitlements,
    view,
    setView,
    isDemoMode,
    isPreviewMode,
    hasEntitlement: (entitlement: EntitlementId) => {
      if (hasFullAccess) return true;
      return entitlements.includes(entitlement);
    },
    canAccessTab: (tabId: TabId) => {
      const requiredEntitlement = TAB_ENTITLEMENTS[tabId];
      if (!requiredEntitlement) return true;
      if (hasFullAccess) return true;
      return entitlements.includes(requiredEntitlement);
    },
    isTabLocked: (tabId: TabId) => {
      const requiredEntitlement = TAB_ENTITLEMENTS[tabId];
      if (!requiredEntitlement) return false;
      if (hasFullAccess) return false;
      return !entitlements.includes(requiredEntitlement);
    },
    isFeatureLocked: (featureId: FeatureId) => {
      if (hasFullAccess) return false;
      const requiredEntitlement = FEATURE_ENTITLEMENTS[featureId];
      return !entitlements.includes(requiredEntitlement);
    },
    hasFullAccess,
    isAdmin,
    trialInfo,
  }), [tier, plan, entitlements, view, isDemoMode, isPreviewMode, hasFullAccess, isAdmin, trialInfo]);

  return <PlanContext.Provider value={value}>{children}</PlanContext.Provider>;
}

export function usePlan(): PlanContextType {
  const ctx = useContext(PlanContext);
  if (!ctx) throw new Error('usePlan must be used within a PlanProvider');
  return ctx;
}
