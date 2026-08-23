import { motion } from 'framer-motion';
import { Check } from 'lucide-react';
import { getPlanCheckoutHref } from '../../preview/routes';
import type { CatalogPlan } from '../../types/billing';

/* Die drei Stufen des Katalogs, so wie das Backend sie liefert: free, plus, pro.
   Der frühere Baukasten setzte Feature-Kacheln zu Plan-IDs zusammen, die es seit
   dem Umbau nicht mehr gibt; jede Auswahl landete deshalb bei "Kostenlos" und
   verlinkte am Checkout vorbei. Hier wird nur noch angezeigt, was im Katalog
   steht. Kein Empfohlen-Badge, keine Minus-Kreuze: was eine Stufe kann, steht
   als Liste da, was sie nicht kann, steht in der Stufe darüber. */

const AKZENT: Record<string, string> = {
  free: '#C5A059',
  plus: '#00D9FF',
  pro: '#FF5A3C',
};

const AKZENT_FALLBACK = '#C5A059';

/** Cent-Betrag als deutscher Endpreis, z. B. `499` → `4,99 €`. */
function eur(cents: number): string {
  const sicher = Math.max(0, Math.round(cents));
  return `${Math.floor(sicher / 100)},${String(sicher % 100).padStart(2, '0')} €`;
}

/** Fälliger Endpreis der Stufe im gewählten Zyklus, in Cent. */
function betragCents(plan: CatalogPlan, cycle: 1 | 12): number {
  if (typeof plan.price?.total_gross_cents === 'number') {
    return plan.price.total_gross_cents;
  }
  const monat = plan.monthly_gross_cents ?? Math.round((plan.price_monthly ?? 0) * 100);
  const jahr = plan.yearly_gross_cents ?? monat * 10;
  return cycle === 12 ? jahr : monat;
}

/** Rechnerischer Monatspreis im Jahreszyklus, in Cent. */
function monatsBetragCents(plan: CatalogPlan, cycle: 1 | 12): number {
  if (typeof plan.price?.effective_monthly_gross_cents === 'number') {
    return plan.price.effective_monthly_gross_cents;
  }
  return Math.round(betragCents(plan, cycle) / cycle);
}

interface PlanStufenProps {
  plans: CatalogPlan[];
  cycle: 1 | 12;
}

export default function PlanStufen({ plans, cycle }: PlanStufenProps) {
  if (plans.length === 0) {
    return null;
  }

  return (
    <div>
      <div className="mb-5">
        <p className="text-base font-semibold text-white mb-1">Drei Stufen, ein Bot</p>
        <p className="text-sm text-white/40">
          Free bleibt vollwertig. Plus zeigt dir deine Entwicklung, Pro nimmt dir die Clip-Arbeit ab.
        </p>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        {plans.map((plan, index) => {
          const akzent = AKZENT[plan.id] ?? AKZENT_FALLBACK;
          const cents = betragCents(plan, cycle);
          const gratis = cents === 0;
          const monatlich = monatsBetragCents(plan, cycle);
          /* Bezahlte Stufen nur anbieten, wenn das Backend sie auch verkaufen
             kann: ohne hinterlegte Stripe-Preis-ID meldet der Katalog
             checkout_available=false und der Checkout schickt jeden Klick mit
             missing_stripe_price_id zurueck. Lieber ehrlich "bald buchbar"
             anzeigen als einen Kauf versprechen, der abprallt. Free braucht
             keinen Stripe-Preis und bleibt immer anklickbar. */
          const buchbar = gratis || plan.checkout_available !== false;

          return (
            <motion.div
              key={plan.id}
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.3, delay: 0.05 * index }}
              data-tour-id={`tour-pricing-${plan.id}`}
              className="rounded-2xl border p-6 flex flex-col"
              style={{
                borderColor: akzent + (plan.is_current ? '80' : '35'),
                backgroundColor: akzent + '0B',
              }}
            >
              <p className="font-semibold text-white text-lg">{plan.name}</p>
              {plan.description && (
                <p className="text-white/40 text-xs leading-snug mt-1">{plan.description}</p>
              )}

              <div className="mt-4 mb-1 flex items-baseline gap-1.5">
                <span className="text-2xl font-bold text-white">
                  {gratis ? '0 €' : eur(cents)}
                </span>
                <span className="text-white/40 text-sm">
                  {gratis ? 'für immer' : cycle === 12 ? '/ Jahr' : '/ Monat'}
                </span>
              </div>
              {!gratis && cycle === 12 && (
                <p className="text-white/30 text-xs">
                  entspricht {eur(monatlich)} im Monat, zwei Monate geschenkt
                </p>
              )}
              {!gratis && cycle === 1 && (
                <p className="text-white/30 text-xs">monatlich kündbar</p>
              )}

              <ul className="space-y-1.5 mt-5 mb-6 flex-1">
                {plan.features.map((feature) => (
                  <li key={feature} className="flex items-start gap-2 text-xs text-white/60">
                    <Check className="w-3 h-3 mt-0.5 flex-shrink-0" style={{ color: akzent }} />
                    {feature}
                  </li>
                ))}
              </ul>

              {plan.is_current ? (
                <span className="px-6 py-3 rounded-xl text-sm font-semibold text-center bg-white/5 text-white/50">
                  Deine aktuelle Stufe
                </span>
              ) : buchbar ? (
                <a
                  href={getPlanCheckoutHref(gratis ? null : plan.id, gratis, cycle)}
                  className={`px-6 py-3 rounded-xl text-sm font-semibold text-center transition-[background-color,border-color,color,box-shadow,transform,translate,scale] duration-200 ${
                    gratis
                      ? 'bg-white/10 hover:bg-white/15 text-white'
                      : 'bg-white/15 hover:bg-white/20 text-white'
                  }`}
                >
                  {gratis ? 'Kostenlos nutzen' : `${plan.name} buchen`}
                </a>
              ) : (
                <span className="px-6 py-3 rounded-xl text-sm font-semibold text-center bg-white/5 text-white/40">
                  Bald buchbar
                </span>
              )}
            </motion.div>
          );
        })}
      </div>

      <p className="text-white/30 text-xs mt-4 text-center">
        Alle Beträge sind Endpreise. Kleinunternehmer nach Paragraph 19 UStG, kein
        Umsatzsteuerausweis.
      </p>
    </div>
  );
}
