import { fetchApi } from './core';
import type { BillingCatalog, CatalogPlan } from '../types/billing';

/**
 * Form der eigenen Wachstumskurve fuer die Premium-Flaeche. Der Server liefert
 * bewusst keine absoluten Zuschauerzahlen, nur Werte zwischen 0 und 1 — der
 * Weichzeichner auf der Karte ist Gestaltung, keine Zugriffskontrolle.
 */
export interface PremiumTeaser {
  tage: number;
  sitzungen: number;
  punkte: number[];
}

export async function fetchPremiumTeaser(): Promise<PremiumTeaser> {
  const raw = await fetchApi<Partial<PremiumTeaser>>('/premium-teaser');
  return {
    tage: raw.tage ?? 0,
    sitzungen: raw.sitzungen ?? 0,
    punkte: Array.isArray(raw.punkte) ? raw.punkte : [],
  };
}

export async function fetchBillingCatalog(cycle: 1 | 12 = 1): Promise<BillingCatalog> {
  const raw = await fetchApi<any>('/billing/catalog', { cycle });
  const plans = ((raw.plans ?? []) as any[]).map((p: any) => ({
    ...p,
    // Endpreise. Der Katalog liefert seit dem Umbau vom 2026-08-09 nur noch
    // `*_gross_cents`; Umsatzsteuer wird nach § 19 UStG nicht ausgewiesen.
    price_monthly: (p.price?.effective_monthly_gross_cents ?? p.monthly_gross_cents ?? 0) / 100,
  })) as CatalogPlan[];

  const cs = raw.current_subscription ?? null;
  const pay = raw.payment ?? null;

  return {
    plans,
    current_subscription: cs
      ? {
          plan_id: cs.plan_id ?? null,
          plan_name: cs.plan_name ?? null,
          tier: cs.tier ?? 'free',
          is_extended: !!cs.is_extended,
          entitlements: cs.entitlements ?? [],
          expires_at: cs.expires_at ?? null,
          source: cs.source ?? null,
        }
      : null,
    payment: pay
      ? {
          invoice_page_path: pay.invoice_page_path ?? undefined,
          cancel_path: pay.cancel_path ?? undefined,
          checkout_path: pay.checkout_path ?? undefined,
        }
      : null,
    tax_notice: raw.tax_notice ?? null,
  };
}
