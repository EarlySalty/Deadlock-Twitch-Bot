import { motion } from 'framer-motion';
import { Check } from 'lucide-react';
import { getPlanCheckoutHref } from '../../preview/routes';
import type { CatalogPlan } from '../../types/billing';

/* Die Stufen des Katalogs, so wie das Backend sie liefert: free, plus, pro.
   Der frühere Baukasten setzte Feature-Kacheln zu Plan-IDs zusammen, die es seit
   dem Umbau nicht mehr gibt; jede Auswahl landete deshalb bei "Kostenlos" und
   verlinkte am Checkout vorbei. Hier wird nur noch angezeigt, was im Katalog
   steht. Kein Empfohlen-Badge, keine Minus-Kreuze: was eine Stufe kann, steht
   als Liste da, was sie nicht kann, steht in der Stufe darüber.

   Creator Pro bekommt bewusst KEINE eigene Säule. Solange das Clip-Werkzeug
   fehlt, bleibt von Pro nur "alles aus Plus" plus Support-Vorrang übrig, und
   eine Karte mit zwei Zeilen neben zwei vollen Karten wirkt wie ein Angebot,
   das man nicht kaufen will. Es steht deshalb als ruhige Zeile darunter, bis
   es Substanz hat. Sobald der Katalog Pro auf `buchbar` stellt und die
   Feature-Liste trägt, gehört es zurück nach oben. */

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

/* Bezahlte Stufen nur anbieten, wenn das Backend sie auch verkaufen kann: ohne
   hinterlegte Stripe-Preis-ID meldet der Katalog checkout_available=false und
   der Checkout schickt jeden Klick mit missing_stripe_price_id zurück. Lieber
   ehrlich "bald buchbar" anzeigen als einen Kauf versprechen, der abprallt.
   Free braucht keinen Stripe-Preis und bleibt immer anklickbar. `buchbar`
   schlägt alles: die Stufe existiert als Ausblick, ihre Funktionen noch nicht. */
function istBuchbar(plan: CatalogPlan, gratis: boolean): boolean {
  return plan.buchbar !== false && (gratis || plan.checkout_available !== false);
}

interface PlanStufenProps {
  plans: CatalogPlan[];
  cycle: 1 | 12;
}

export default function PlanStufen({ plans, cycle }: PlanStufenProps) {
  if (plans.length === 0) {
    return null;
  }

  const karten = plans.filter((plan) => plan.id !== 'pro');
  const pro = plans.find((plan) => plan.id === 'pro');
  const proGehoertNachOben = pro ? istBuchbar(pro, false) : false;
  const obenAngezeigt = proGehoertNachOben && pro ? [...karten, pro] : karten;

  return (
    <div>
      <div className="mb-7">
        <p className="text-xl font-semibold text-white mb-1.5">Zwei Stufen, ein Bot</p>
        <p className="text-sm text-white/45">
          Free bleibt dauerhaft vollwertig. Plus zeigt dir deine Entwicklung statt nur den
          letzten Stream.
        </p>
      </div>

      <div
        className={`grid grid-cols-1 gap-6 ${
          obenAngezeigt.length > 2 ? 'lg:grid-cols-3' : 'lg:grid-cols-2'
        }`}
      >
        {obenAngezeigt.map((plan, index) => {
          const akzent = AKZENT[plan.id] ?? AKZENT_FALLBACK;
          const cents = betragCents(plan, cycle);
          const gratis = cents === 0;
          const monatlich = monatsBetragCents(plan, cycle);
          const buchbar = istBuchbar(plan, gratis);

          return (
            <motion.div
              key={plan.id}
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.3, delay: 0.05 * index }}
              data-tour-id={`tour-pricing-${plan.id}`}
              className="rounded-2xl border p-8 flex flex-col"
              style={{
                borderColor: akzent + (plan.is_current ? '80' : '35'),
                backgroundColor: akzent + '0B',
              }}
            >
              <p className="font-semibold text-white text-xl">{plan.name}</p>
              {plan.description && (
                <p className="text-white/45 text-sm leading-relaxed mt-2 max-w-md">
                  {plan.description}
                </p>
              )}

              <div className="mt-7 mb-1 flex items-baseline gap-2">
                <span className="text-4xl font-bold text-white tracking-tight">
                  {gratis ? '0 €' : eur(cents)}
                </span>
                <span className="text-white/45 text-base">
                  {gratis ? 'für immer' : cycle === 12 ? '/ Jahr' : '/ Monat'}
                </span>
              </div>
              {!gratis && cycle === 12 && (
                <p className="text-white/35 text-sm">
                  entspricht {eur(monatlich)} im Monat, zwei Monate geschenkt
                </p>
              )}
              {!gratis && cycle === 1 && (
                <p className="text-white/35 text-sm">monatlich kündbar</p>
              )}

              <ul className="space-y-3 mt-8 mb-8 flex-1">
                {plan.features.map((feature) => (
                  <li
                    key={feature}
                    className="flex items-start gap-3 text-sm text-white/70 leading-relaxed"
                  >
                    <Check
                      className="w-4 h-4 mt-0.5 flex-shrink-0"
                      style={{ color: akzent }}
                    />
                    {feature}
                  </li>
                ))}
              </ul>

              {plan.is_current ? (
                <span className="px-6 py-3.5 rounded-xl text-sm font-semibold text-center bg-white/5 text-white/50">
                  Deine aktuelle Stufe
                </span>
              ) : buchbar ? (
                <a
                  href={getPlanCheckoutHref(gratis ? null : plan.id, gratis, cycle)}
                  className={`px-6 py-3.5 rounded-xl text-sm font-semibold text-center transition-[background-color,border-color,color,box-shadow,transform,translate,scale] duration-200 ${
                    gratis
                      ? 'bg-white/10 hover:bg-white/15 text-white'
                      : 'bg-white/15 hover:bg-white/20 text-white'
                  }`}
                >
                  {gratis ? 'Kostenlos nutzen' : `${plan.name} buchen`}
                </a>
              ) : (
                <span className="px-6 py-3.5 rounded-xl text-sm font-semibold text-center bg-white/5 text-white/40">
                  Bald buchbar
                </span>
              )}

              {/* Wer im Test oder auf einem alten Tarif steht, nutzt die Stufe
                  schon, hat sie aber nicht dauerhaft gekauft. Der Knopf bleibt
                  deshalb stehen, der Hinweis sagt nur, warum. */}
              {plan.hinweis && (
                <p className="text-white/35 text-sm mt-3 text-center">{plan.hinweis}</p>
              )}
            </motion.div>
          );
        })}
      </div>

      {pro && !proGehoertNachOben && (
        <div className="mt-6 rounded-2xl border border-white/10 bg-white/[0.02] px-8 py-6 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
          <div className="max-w-2xl">
            <p className="text-white/80 font-medium">
              {pro.name} kommt, sobald das Clip-Werkzeug steht.
            </p>
            <p className="text-white/40 text-sm mt-1.5 leading-relaxed">
              Geplant sind Clips ohne Mengenbegrenzung, automatisches Posten und Untertitel.
              Solange davon nichts läuft, gibt es dafür auch nichts zu bezahlen.
            </p>
          </div>
          <span className="text-white/35 text-sm whitespace-nowrap">
            später {eur(pro.monthly_gross_cents ?? 999)} im Monat
          </span>
        </div>
      )}

      <p className="text-white/30 text-sm mt-6 text-center">
        Alle Beträge sind Endpreise. Kleinunternehmer nach Paragraph 19 UStG, kein
        Umsatzsteuerausweis.
      </p>
    </div>
  );
}
