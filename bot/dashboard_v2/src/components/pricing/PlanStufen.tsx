import { motion } from 'framer-motion';
import { Check } from 'lucide-react';
import { getPlanCheckoutHref } from '../../preview/routes';
import type { CatalogPlan } from '../../types/billing';
import { useT } from '../../context/LanguageContext';
import { activeLocale, tr } from '../../i18n/dictionary';

const AKZENT: Record<string, string> = {
  free: '#C5A059',
  plus: '#00D9FF',
  pro: '#FF5A3C',
};

const AKZENT_FALLBACK = '#C5A059';

function eur(cents: number, stellen = 2): string {
  const sicher = Math.max(0, Math.round(cents));
  return new Intl.NumberFormat(activeLocale(), {
    style: 'currency',
    currency: 'EUR',
    minimumFractionDigits: stellen,
    maximumFractionDigits: stellen,
  }).format(sicher / 100);
}

function betragCents(plan: CatalogPlan, cycle: 1 | 12): number {
  if (typeof plan.price?.total_gross_cents === 'number') {
    return plan.price.total_gross_cents;
  }
  const monat = plan.monthly_gross_cents ?? Math.round((plan.price_monthly ?? 0) * 100);
  const jahr = plan.yearly_gross_cents ?? monat * 10;
  return cycle === 12 ? jahr : monat;
}

function monatsBetragCents(plan: CatalogPlan, cycle: 1 | 12): number {
  if (typeof plan.price?.effective_monthly_gross_cents === 'number') {
    return plan.price.effective_monthly_gross_cents;
  }
  return Math.round(betragCents(plan, cycle) / cycle);
}

function istBuchbar(plan: CatalogPlan, gratis: boolean): boolean {
  return plan.buchbar !== false && (gratis || plan.checkout_available !== false);
}

function ueberschriftFuer(anzahl: number): string {
  switch (anzahl) {
    case 1:
      return tr('Eine Stufe, ein Bot');
    case 2:
      return tr('Zwei Stufen, ein Bot');
    case 3:
      return tr('Drei Stufen, ein Bot');
    case 4:
      return tr('Vier Stufen, ein Bot');
    default:
      return tr('{count} Stufen, ein Bot', { count: anzahl });
  }
}

function unterzeileFuer(anzahl: number): string {
  if (anzahl <= 1) {
    return tr('Free bleibt dauerhaft vollwertig.');
  }
  if (anzahl === 2) {
    return tr('Free bleibt dauerhaft vollwertig. Plus zeigt dir deine Entwicklung statt nur den letzten Stream.');
  }
  return tr('Free bleibt dauerhaft vollwertig. Die bezahlten Stufen zeigen dir deine Entwicklung statt nur den letzten Stream.');
}

interface PlanStufenProps {
  plans: CatalogPlan[];
  cycle: 1 | 12;
}

export default function PlanStufen({ plans, cycle }: PlanStufenProps) {
  const t = useT();
  if (plans.length === 0) {
    return null;
  }

  const karten = plans.filter((plan) => plan.id !== 'pro');
  const pro = plans.find((plan) => plan.id === 'pro');
  const proGehoertNachOben = pro ? istBuchbar(pro, false) || pro.is_current : false;
  const obenAngezeigt = proGehoertNachOben && pro ? [...karten, pro] : karten;

  return (
    <div>
      <div className="mb-7">
        <p className="text-xl font-semibold text-white mb-1.5">
          {ueberschriftFuer(obenAngezeigt.length)}
        </p>
        <p className="text-sm text-white/45">{unterzeileFuer(obenAngezeigt.length)}</p>
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
                  {t(plan.description)}
                </p>
              )}

              <div className="mt-7 mb-1 flex items-baseline gap-2">
                <span className="text-4xl font-bold text-white tracking-tight">
                  {gratis ? eur(0, 0) : eur(cents)}
                </span>
                <span className="text-white/45 text-base">
                  {gratis ? t('für immer') : cycle === 12 ? t('/ Jahr') : t('/ Monat')}
                </span>
              </div>
              {!gratis && cycle === 12 && (
                <p className="text-white/35 text-sm">
                  {t('entspricht {price} im Monat, zwei Monate geschenkt', { price: eur(monatlich) })}
                </p>
              )}
              {!gratis && cycle === 1 && (
                <p className="text-white/35 text-sm">{t('monatlich kündbar')}</p>
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
                    {t(feature)}
                  </li>
                ))}
              </ul>

              {plan.is_current ? (
                <span className="px-6 py-3.5 rounded-xl text-sm font-semibold text-center bg-white/5 text-white/50">
                  {t('Deine aktuelle Stufe')}
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
                  {gratis ? t('Kostenlos nutzen') : t('{plan} buchen', { plan: plan.name })}
                </a>
              ) : (
                <span className="px-6 py-3.5 rounded-xl text-sm font-semibold text-center bg-white/5 text-white/40">
                  {t('Bald buchbar')}
                </span>
              )}

              {plan.hinweis && (
                <p className="text-white/35 text-sm mt-3 text-center">{t(plan.hinweis)}</p>
              )}
            </motion.div>
          );
        })}
      </div>

      {pro && !proGehoertNachOben && (
        <div className="mt-6 rounded-2xl border border-white/10 bg-white/[0.02] px-8 py-6 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
          <div className="max-w-2xl">
            <p className="text-white/80 font-medium">{t('{plan} ist noch nicht buchbar.', { plan: pro.name })}</p>
            <p className="text-white/40 text-sm mt-1.5 leading-relaxed">
              {t('Die Stufe bekommt Inhalt, sobald die Werkzeuge dafür stehen.')}
            </p>
          </div>
          {typeof pro.monthly_gross_cents === 'number' && (
            <span className="text-white/35 text-sm whitespace-nowrap">
              {t('später {price} im Monat', { price: eur(pro.monthly_gross_cents) })}
            </span>
          )}
        </div>
      )}

      <p className="text-white/30 text-sm mt-6 text-center">
        {t('Alle Beträge sind Endpreise. Kleinunternehmer nach Paragraph 19 UStG, kein Umsatzsteuerausweis.')}
      </p>
    </div>
  );
}
