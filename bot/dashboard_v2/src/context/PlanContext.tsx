import { createContext, useContext, useState, useEffect, useMemo, type ReactNode } from 'react';
import type { EntitlementId, PlanTier, DashboardView, PlanStatus, TabId, FeatureId } from '../types/billing';
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
  const entitlements: EntitlementId[] = hasFullAccess
    ? ALL_ENTITLEMENTS
    : (plan?.entitlements ?? []);
  const [view, setView] = useState<DashboardView>(
    hasFullAccess || entitlements.includes('analytics.extended') ? 'extended' : 'basic'
  );
  // Sync view when tier changes after mount (e.g. auth loads async)
  useEffect(() => {
    if (hasFullAccess || entitlements.includes('analytics.extended')) {
      setView('extended');
    }
  }, [entitlements, hasFullAccess]);

  const isPreviewMode = view === 'extended' && !hasFullAccess && !entitlements.includes('analytics.extended');

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
  }), [tier, plan, entitlements, view, isDemoMode, isPreviewMode, hasFullAccess]);

  return <PlanContext.Provider value={value}>{children}</PlanContext.Provider>;
}

export function usePlan(): PlanContextType {
  const ctx = useContext(PlanContext);
  if (!ctx) throw new Error('usePlan must be used within a PlanProvider');
  return ctx;
}
