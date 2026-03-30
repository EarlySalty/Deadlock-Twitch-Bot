import { fetchApi } from './core';
import type { CatalogPlan } from '../types/billing';

export async function fetchBillingCatalog(): Promise<{ plans: CatalogPlan[] }> {
  return fetchApi<{ plans: CatalogPlan[] }>('/billing/catalog');
}
