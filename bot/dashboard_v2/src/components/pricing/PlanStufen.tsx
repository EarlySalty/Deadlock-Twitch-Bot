import { useLanguage, useOAuthLink, useT } from '../../context/LanguageContext';
import { translatePageText } from '../../i18n/pageText';
import { useState } from 'react';
import { Check } from 'lucide-react';
import { getPlanCheckoutHref } from '../../preview/routes';
import type { CatalogPlan } from '../../types/billing';

const GOLD = '#C5A059';
const CYAN = '#00D9FF';

const PLUS_PUNKTE = [
  'Voller Verlauf, Vergleiche, Wachstum',
  'KI-Analyse, KI-Chat, Coaching, Wochenreport',
  'Lurker-Erinnerung und Raid-Vorrang',
  'Community-Hinweise abschaltbar',
];

const STUDIO_KURZ: Record<string, string> = {
  studio: 'Drei Plattformen, Einspruch bis zum Termin. 40 Clips.',
  studio_ki: 'Titel, Hashtags, Vollautomatik. 80 Clips.',
  studio_cut: 'KI-Schnitt mit Prüfung. 12 Schnitte im Monat.',
};

const STUDIO_IDS = ['studio', 'studio_ki', 'studio_cut'] as const;

type Marke = 'ja' | 'nein' | string;

const MATRIX: Array<{ gruppe?: string; name?: string; werte?: [Marke, Marke, Marke, Marke, Marke] }> = [
  { gruppe: 'Bot, in jeder Stufe frei' },
  { name: 'Auto-Raid, beide Richtungen', werte: ['ja', 'ja', 'ja', 'ja', 'ja'] },
  { name: 'Chat-Schutz und alle Befehle', werte: ['ja', 'ja', 'ja', 'ja', 'ja'] },
  { name: 'Overlay, Sendeplan, Tagesform', werte: ['ja', 'ja', 'ja', 'ja', 'ja'] },
  { name: 'Go-Live im Community-Discord', werte: ['ja', 'ja', 'ja', 'ja', 'ja'] },
  { name: 'Steam-Bot und Twitch-Werbemanager', werte: ['ja', 'ja', 'ja', 'ja', 'ja'] },
  { gruppe: 'Plus' },
  { name: 'Voller Verlauf, Vergleiche, KI-Auswertung', werte: ['nein', 'ja', 'ja', 'ja', 'ja'] },
  { name: 'Lurker-Erinnerung, Raid-Vorrang', werte: ['nein', 'ja', 'ja', 'ja', 'ja'] },
  { name: 'Community-Hinweise im Chat', werte: ['an', 'Schalter', 'Schalter', 'Schalter', 'Schalter'] },
  { gruppe: 'Studio-Versionen' },
  { name: 'Pipeline, Import, VOD-Archiv', werte: ['nein', 'nein', 'ja', 'ja', 'ja'] },
  { name: 'YouTube, TikTok, Instagram, Vorlagen', werte: ['nein', 'nein', 'ja', 'ja', 'ja'] },
  { name: 'Auto-Pilot: Freigabe oder Einspruch', werte: ['nein', 'nein', 'ja', 'ja', 'ja'] },
  { name: 'Vollautomatik, Titel, Hashtags', werte: ['nein', 'nein', 'nein', 'ja', 'ja'] },
  { name: 'KI-Schnitt, mehrere Videos, Prüfung', werte: ['nein', 'nein', 'nein', 'nein', 'ja'] },
  { name: 'Clips im Monat', werte: ['nein', 'nein', '40', '80', 'ohne Limit'] },
  { name: 'KI-Schnitte im Monat', werte: ['nein', 'nein', 'nein', 'nein', '12'] },
];

function eur(cents: number, locale: string): string {
  const sicher = Math.max(0, Math.round(cents));
  return new Intl.NumberFormat(locale, { style: 'currency', currency: 'EUR' }).format(sicher / 100);
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

function zelle(wert: Marke) {
  if (wert === 'ja') {
    return <span className="font-bold" style={{ color: GOLD }}>✓</span>;
  }
  if (wert === 'nein') {
    return <span className="text-white/20">·</span>;
  }
  return <span className="text-sm font-semibold tabular-nums text-white/80">{wert}</span>;
}

interface PlanStufenProps {
  plans: CatalogPlan[];
  cycle: 1 | 12;
}

export default function PlanStufen({ plans, cycle }: PlanStufenProps) {
  const t = useT();
  const { locale } = useLanguage();
  const oauthLink = useOAuthLink();
  const katalog = plans.filter((plan) => plan.id !== 'pro');
  const plus = katalog.find((plan) => plan.id === 'plus');
  const studioPlaene = STUDIO_IDS.flatMap((id) => {
    const plan = katalog.find((eintrag) => eintrag.id === id);
    return plan ? [plan] : [];
  });
  const laufend = studioPlaene.find((plan) => plan.is_current);
  const empfohlen = studioPlaene.find((plan) => plan.recommended) ?? studioPlaene[0];
  const [wahl, setWahl] = useState<string | null>(null);
  const gewaehlt = studioPlaene.find((plan) => plan.id === (wahl ?? laufend?.id ?? empfohlen?.id)) ?? studioPlaene[0];

  if (katalog.length === 0) {
    return null;
  }

  const kopf = STUDIO_IDS.map((id) => katalog.find((plan) => plan.id === id));

  const knopf = (plan: CatalogPlan, farbe: 'plus' | 'studio') => {
    const buchbar = istBuchbar(plan, false);
    const klasse = 'mt-4 block w-full rounded-xl px-4 py-3 text-center text-sm font-semibold';
    if (plan.is_current) {
      return <span className={`${klasse} bg-white/5 text-white/50`}>{t('Deine aktuelle Stufe')}</span>;
    }
    if (!buchbar) {
      return <span className={`${klasse} bg-white/5 text-white/40`}>{t('Bald buchbar')}</span>;
    }
    const href = oauthLink(getPlanCheckoutHref(plan.id, false, cycle));
    const stil = farbe === 'plus' ? 'text-white' : 'text-[#0D0806]';
    const hintergrund = farbe === 'plus' ? 'rgba(0, 217, 255, 0.16)' : GOLD;
    return (
      <a href={href} className={`${klasse} ${stil}`} style={{ backgroundColor: hintergrund }}>
        {t('{p0} buchen', { p0: t(plan.name) })}
      </a>
    );
  };

  return (
    <div>
      <div className="mb-5">
        <p className="text-xl font-semibold text-white">{t('Plus oder Studio')}</p>
        <p className="mt-1 text-sm text-white/55">
          {t('Zwei Produkte. Studio hat drei Versionen. Studio enthält Plus.')}
        </p>
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,0.82fr)_minmax(0,1.18fr)]">
        {plus && (
          <article
            className="flex flex-col rounded-2xl border p-5"
            style={{ borderColor: 'rgba(0, 217, 255, 0.35)', backgroundColor: 'rgba(0, 217, 255, 0.04)' }}
          >
            <p className="text-xs font-extrabold uppercase tracking-[0.14em]" style={{ color: CYAN }}>
              {t(plus.name)}
            </p>
            <p className="mt-2 text-3xl font-bold text-white">
              {eur(betragCents(plus, cycle), locale)}{' '}
              <span className="text-base font-semibold text-white/45">
                {cycle === 12 ? t('/ Jahr') : t('/ Monat')}
              </span>
            </p>
            {cycle === 12 && (
              <p className="text-sm text-white/40">
                {t('entspricht {p0} im Monat, zwei Monate geschenkt', {
                  p0: eur(monatsBetragCents(plus, cycle), locale),
                })}
              </p>
            )}
            <p className="mt-2 text-sm text-white/60">{t('Dein Verlauf und die KI-Auswertung.')}</p>
            <ul className="mt-3 space-y-2">
              {PLUS_PUNKTE.map((punkt) => (
                <li key={punkt} className="flex items-start gap-2 text-sm text-white/70">
                  <Check className="mt-0.5 h-4 w-4 shrink-0" style={{ color: CYAN }} />
                  {t(punkt)}
                </li>
              ))}
            </ul>
            {knopf(plus, 'plus')}
            {plus.hinweis && (
              <p className="mt-3 text-center text-sm text-white/35">
                {translatePageText(t, plus.hinweis, locale)}
              </p>
            )}
          </article>
        )}

        {gewaehlt && (
          <article
            className="flex flex-col rounded-2xl border p-5"
            style={{ borderColor: 'rgba(197, 160, 89, 0.45)', backgroundColor: 'rgba(197, 160, 89, 0.05)' }}
          >
            <p className="text-xs font-extrabold uppercase tracking-[0.14em]" style={{ color: GOLD }}>
              {t('Studio')}
            </p>
            <p className="mt-2 text-sm text-white/60">
              {t('Drei Versionen, eine Buchung. Enthält alles aus Plus.')}
            </p>
            <div className="my-3 flex flex-col gap-2" role="radiogroup" aria-label={t('Studio-Version')}>
              {studioPlaene.map((plan, index) => {
                const an = plan.id === gewaehlt.id;
                const vorher = index > 0 ? studioPlaene[index - 1] : undefined;
                const diff = vorher ? betragCents(plan, cycle) - betragCents(vorher, cycle) : 0;
                return (
                  <label
                    key={plan.id}
                    className="grid cursor-pointer grid-cols-[16px_1fr_auto] items-center gap-2 rounded-xl border px-3 py-2"
                    style={{
                      borderColor: an ? 'rgba(197, 160, 89, 0.7)' : 'rgba(255,255,255,0.1)',
                      backgroundColor: an ? 'rgba(197, 160, 89, 0.1)' : 'rgba(0,0,0,0.18)',
                    }}
                  >
                    <input
                      type="radio"
                      name="studio-version"
                      value={plan.id}
                      checked={an}
                      onChange={() => setWahl(plan.id)}
                      style={{ accentColor: GOLD }}
                    />
                    <span>
                      <b className="block text-sm text-white">{t(plan.name)}</b>
                      <small className="block text-xs text-white/45">{t(STUDIO_KURZ[plan.id] ?? plan.description ?? '')}</small>
                    </span>
                    <span className="text-right text-sm font-bold tabular-nums text-white">
                      {eur(betragCents(plan, cycle), locale)}
                      {diff > 0 && (
                        <span className="mt-0.5 block text-[10px] font-bold" style={{ color: GOLD }}>
                          +{eur(diff, locale)}
                        </span>
                      )}
                    </span>
                  </label>
                );
              })}
            </div>
            {knopf(gewaehlt, 'studio')}
            {gewaehlt.hinweis && (
              <p className="mt-3 text-center text-sm text-white/35">
                {translatePageText(t, gewaehlt.hinweis, locale)}
              </p>
            )}
          </article>
        )}
      </div>

      <p className="my-4 rounded-xl bg-white/[0.03] px-3 py-2.5 text-xs text-white/60">
        <strong className="text-white">{t('Netzwerk Free, 0 €.')}</strong>{' '}
        {t('Kein Abo. Auto-Raid, Chat-Schutz, alle Befehle, Overlay, Sendeplan, Tagesform, Go-Live im Community-Discord, Steam-Bot, Twitch-Werbemanager.')}
        {katalog.find((plan) => plan.id === 'free')?.is_current ? ` ${t('Deine aktuelle Stufe.')}` : ''}
      </p>

      <h2 className="mb-2 text-base font-bold text-white">{t('Vergleich')}</h2>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[560px] border-separate border-spacing-0 text-sm">
          <thead>
            <tr>
              <th rowSpan={2} className="sticky left-0 z-20 bg-[#1b1713] px-2 py-2 text-left font-semibold text-white/45">
                {t('Leistung')}
              </th>
              <th rowSpan={2} className="bg-[#1b1713] px-2 py-2">Free</th>
              <th rowSpan={2} className="bg-[#1b1713] px-2 py-2">Plus</th>
              <th className="bg-[#2a2418] px-2 py-2 text-center font-extrabold uppercase tracking-wider" style={{ color: GOLD }} colSpan={3}>
                {t('Studio')}
              </th>
            </tr>
            <tr>
              {kopf.map((plan, index) => (
                <th
                  key={plan?.id ?? index}
                  className="px-2 py-2 text-center text-xs"
                  style={{ backgroundColor: index === 1 ? '#322a1c' : '#1b1713' }}
                >
                  {plan
                    ? index === 1
                      ? `KI, ${eur(betragCents(plan, cycle), locale)}`
                      : index === 2
                        ? `Schnitt, ${eur(betragCents(plan, cycle), locale)}`
                        : eur(betragCents(plan, cycle), locale)
                    : ''}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {MATRIX.map((zeile) =>
              zeile.gruppe ? (
                <tr key={zeile.gruppe}>
                  <td colSpan={6} className="px-2 pb-1 pt-3 text-left text-[10px] font-extrabold uppercase tracking-[0.14em]" style={{ color: GOLD }}>
                    {t(zeile.gruppe)}
                  </td>
                </tr>
              ) : (
                <tr key={zeile.name}>
                  <td className="sticky left-0 z-10 border-b border-white/10 bg-[#14110e] px-2 py-1.5 text-left text-white/60">
                    {t(zeile.name ?? '')}
                  </td>
                  {zeile.werte?.map((wert, index) => (
                    <td
                      key={`${zeile.name}-${index}`}
                      className="border-b border-white/10 px-2 py-1.5 text-center"
                      style={{ backgroundColor: index === 3 ? 'rgba(197, 160, 89, 0.1)' : undefined }}
                    >
                      {zelle(wert)}
                    </td>
                  ))}
                </tr>
              ),
            )}
          </tbody>
        </table>
      </div>
      <p className="mt-3 max-w-3xl text-xs text-white/40">
        {t('Gebucht wird Plus oder genau eine Studio-Version. Go-Live bleibt in jeder Stufe frei. Community-Hinweise sind standardmäßig an. Jahrespreis gleich zehn Monatspreise.')}
      </p>
      <p className="mt-4 text-center text-sm text-white/30">
        {t('Alle Beträge sind Endpreise. Kleinunternehmer nach Paragraph 19 UStG, kein Umsatzsteuerausweis.')}
      </p>
    </div>
  );
}
