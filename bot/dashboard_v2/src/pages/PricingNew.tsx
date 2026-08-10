import { useBillingCatalog, usePremiumTeaser } from '../hooks/useAnalytics';
import { PremiumKarte } from '../components/pricing-v2/PremiumKarte';
import { Rise } from '../motion/Rise';
import { isActivePaidSubscription } from '../types/billing';

/**
 * Die Preisseite nach dem Umbau vom 2026-08-09: eine Karte mit den eigenen
 * Zahlen des Streamers statt acht Plaenen mit Baukasten. Die alte Seite steht
 * unverändert unter `/twitch/old/pricing`.
 *
 * Diese Seite haengt bewusst an keiner Komponente aus `components/pricing/`.
 */

/** Ablaufdatum eines laufenden Trials, sonst null. */
function laufenderTrial(planId: string | null | undefined, ablauf: string | null): Date | null {
  if (planId !== 'analytics_trial' || !ablauf) return null;
  const ende = new Date(ablauf);
  if (Number.isNaN(ende.getTime()) || ende.getTime() <= Date.now()) return null;
  return ende;
}

export default function PricingNew() {
  const { data: katalog } = useBillingCatalog(1);
  const { data: teaser } = usePremiumTeaser();

  const premium = katalog?.plans.find((plan) => plan.id === 'premium');
  const abo = katalog?.current_subscription ?? null;
  const trialEnde = laufenderTrial(abo?.plan_id, abo?.expires_at ?? null);
  const istPremium = isActivePaidSubscription(abo) && abo.plan_id === 'premium';
  const rechnungen = katalog?.payment?.invoice_page_path ?? '/twitch/abbo/rechnungen';

  const monatCents = premium?.monthly_gross_cents ?? 0;
  const jahrCents = premium?.yearly_gross_cents ?? 0;

  return (
    <div className="mx-auto max-w-xl px-4 py-10">
      {istPremium ? (
        <Rise className="rounded-2xl border border-border bg-card p-6 md:p-8">
          <h1 className="text-lg font-semibold text-white">Premium ist aktiv</h1>
          <p className="mt-3 text-white/60">
            Alle Auswertungen sind freigeschaltet. Es gibt hier nichts zu tun.
          </p>
          <a
            href={rechnungen}
            data-press
            className="mt-5 inline-flex items-center rounded-xl border border-border bg-white/5 px-4 py-2.5 text-sm text-white"
          >
            Rechnungen und Abo verwalten
          </a>
        </Rise>
      ) : (
        <Rise>
          <PremiumKarte
            tage={teaser?.tage ?? 0}
            punkte={teaser?.punkte ?? []}
            monatCents={monatCents}
            jahrCents={jahrCents}
            leistungen={premium?.features ?? []}
            steuerhinweis={katalog?.tax_notice ?? null}
            entsperrt={trialEnde !== null}
            einleitung={
              trialEnde
                ? `Dein Testzugang läuft noch bis zum ${trialEnde.toLocaleDateString('de-DE')}. Danach werden die Auswertungen wieder unscharf.`
                : 'Deine Zahlen liegen bereit. Premium macht sie sichtbar.'
            }
            knopfText={trialEnde ? 'Premium behalten' : 'Freischalten'}
          />
          <p className="mt-4 text-center text-xs text-white/30">
            Free bleibt kostenlos, mit dem letzten Stream als Fenster.
          </p>
        </Rise>
      )}
    </div>
  );
}
