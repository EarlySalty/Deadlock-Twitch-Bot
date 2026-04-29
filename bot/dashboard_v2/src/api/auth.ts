import { fetchApi } from './core';
import type { EntitlementId, PlanTier } from '../types/billing';

export interface AuthStatus {
  authenticated: boolean;
  level: 'localhost' | 'admin' | 'partner' | 'none';
  demoMode: boolean;
  isAdmin: boolean;
  isLocalhost: boolean;
  canViewAllStreamers: boolean;
  twitchLogin?: string | null;
  displayName?: string | null;
  partnerStatus?: 'active' | 'archived' | 'departnered' | 'non_partner' | 'token_error' | 'blocked' | null;
  technicalPauseReason?: string | null;
  operationalState?: string | null;
  canAccessAnalyticsDashboard?: boolean;
  tokenErrorGraceExpiresAt?: string | null;
  csrfToken?: string | null;
  csrf_token?: string | null;
  access?: {
    landing: boolean;
    analytics: boolean;
  };
  permissions: {
    viewAllStreamers: boolean;
    viewComparison: boolean;
    viewChatAnalytics: boolean;
    viewOverlap: boolean;
  };
  plan?: {
    planId: string | null;
    planName: string | null;
    tier: PlanTier;
    isExtended: boolean;
    expiresAt: string | null;
    source: string | null;
    entitlements: EntitlementId[];
  } | null;
}

export async function fetchAuthStatus(): Promise<AuthStatus> {
  return fetchApi<AuthStatus>('/auth-status');
}
