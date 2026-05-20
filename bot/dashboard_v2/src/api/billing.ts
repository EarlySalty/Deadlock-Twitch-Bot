import { fetchApi } from './core';
import type { CatalogPlan } from '../types/billing';

export async function fetchBillingCatalog(cycle: 1 | 12 = 1): Promise<{ plans: CatalogPlan[] }> {
  const raw = await fetchApi<any>('/billing/catalog', { cycle });
  const plans = ((raw.plans ?? []) as any[]).map((p: any) => ({
    ...p,
    price_monthly: (p.price?.effective_monthly_net_cents ?? p.monthly_net_cents ?? 0) / 100,
  }));
  return { plans };
}
