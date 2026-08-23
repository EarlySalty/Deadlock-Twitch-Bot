import { motion } from "framer-motion";
import { initials } from "@/components/v2/NetworkLive";
import type { PartnerChannel } from "@/hooks/useNetworkMetrics";
import { ArrowRight, Check, Plus } from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import { objections, plans } from "@/data/networkPage";
import type { Plan } from "@/data/networkPage";
import {
  DISCORD_INVITE_URL,
  TWITCH_ABBO_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_SECURITY_URL,
  buildTwitchBotAuthUrl,
} from "@/data/externalLinks";

function resolveHref(key: string): string {
  switch (key) {
    case "AUTH":
      return buildTwitchBotAuthUrl();
    case "ABBO":
      return TWITCH_ABBO_URL;
    case "SECURITY":
      return TWITCH_SECURITY_URL;
    case "PRIVACY":
      return TWITCH_DATENSCHUTZ_URL;
    default:
      return key;
  }
}

/**
 * Leistungen eines Plans. Was ein Plan nicht kann, wird weggelassen statt mit
 * einem Minus danebengestellt: die Karte soll zeigen, was man bekommt.
 */
function PlanFeatures({ plan, columns }: { plan: Plan; columns?: boolean }) {
  return (
    <ul
      className={`space-y-3 ${columns ? "sm:columns-2 sm:gap-x-8 sm:space-y-0" : ""}`}
    >
      {plan.features
        .filter((feature) => feature.included)
        .map((feature) => (
          <li
            key={feature.label}
            className={`flex gap-3 text-sm text-[var(--color-text-secondary)] ${
              columns ? "sm:mb-3 sm:break-inside-avoid" : ""
            }`}
          >
            <Check
              size={16}
              className="mt-0.5 shrink-0 text-[var(--color-primary)]"
            />
            <span>{feature.label}</span>
          </li>
        ))}
    </ul>
  );
}

/**
 * Der kostenlose Plan steht allein ueber die volle Breite: linke Spalte der
 * Preis, rechte Spalte die Leistungen. Das goldene Licht liegt hier und nur
 * hier, damit der Blick beim Gratisangebot landet und nicht beim teuersten.
 */
function FreePlan({ plan }: { plan: Plan }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 26 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-70px" }}
      transition={{ duration: 0.55 }}
      className="panel-card v2-plan-featured grid gap-10 rounded-2xl p-8 sm:p-10 lg:grid-cols-[minmax(0,0.85fr)_minmax(0,1.15fr)] lg:items-center"
    >
      <div>
        <h3 className="text-lg font-bold text-[var(--color-text-primary)]">
          {plan.name}
        </h3>
        <div className="mt-4 flex flex-wrap items-baseline gap-x-3">
          <span
            className="text-[clamp(3.4rem,7vw,4.6rem)] font-extrabold leading-none bg-clip-text text-transparent"
            style={{ backgroundImage: "var(--gradient-brand)" }}
          >
            {plan.price}
          </span>
          <span className="text-xl font-bold text-[var(--color-text-primary)]">
            {plan.period}
          </span>
        </div>
        <p className="mt-3 text-[var(--color-text-secondary)]">
          {plan.anchor}. Keine Karte, keine Laufzeit.
        </p>
        <a
          href={resolveHref(plan.ctaHref)}
          className="gradient-accent mt-7 inline-flex w-full items-center justify-center gap-2 rounded-xl px-6 py-3.5 font-semibold no-underline transition-all hover:brightness-110 sm:w-auto"
        >
          {plan.cta}
          <ArrowRight size={18} />
        </a>
      </div>

      <PlanFeatures plan={plan} columns />
    </motion.div>
  );
}

/** Plus und Pro darunter: gleich gross, ruhig, ohne Gold und ohne Badge. */
function PaidPlan({ plan, index }: { plan: Plan; index: number }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 22 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-70px" }}
      transition={{ duration: 0.5, delay: index * 0.09 }}
      className="panel-card flex flex-col rounded-2xl p-8"
    >
      <h3 className="text-lg font-bold text-[var(--color-text-primary)]">
        {plan.name}
      </h3>
      <p className="mt-1.5 text-sm leading-snug text-[rgba(183,170,145,0.62)]">
        {plan.anchor}
      </p>

      <div className="mt-6 flex items-baseline gap-2">
        <span className="text-4xl font-extrabold leading-none text-[var(--color-text-primary)]">
          {plan.price}
        </span>
        <span className="text-sm text-[var(--color-text-secondary)]">
          {plan.period}
        </span>
      </div>
      {plan.yearly ? (
        <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
          {plan.yearly}
        </p>
      ) : null}

      <div className="mt-7 flex-1">
        <PlanFeatures plan={plan} />
      </div>

      {plan.note ? (
        <p className="mt-6 rounded-lg border border-[var(--color-border)] bg-black/25 px-3.5 py-2.5 text-xs leading-relaxed text-[var(--color-text-secondary)]">
          {plan.note}
        </p>
      ) : null}

      <a
        href={resolveHref(plan.ctaHref)}
        className="mt-6 inline-flex items-center justify-center gap-2 rounded-xl border border-[var(--color-border)] px-6 py-3 font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-border-hover)] hover:bg-white/5"
      >
        {plan.cta}
      </a>
    </motion.div>
  );
}

/** Der kostenlose Plan zuerst und allein, die bezahlten darunter. */
export function PricingSection() {
  const free = plans.find((plan) => plan.featured) ?? plans[0];
  const paid = plans.filter((plan) => plan !== free);

  return (
    <ProtocolSection
      id="preise"
      ambientSide="left"
      stamp="05 · Preise"
      headline="Kostenlos bleibt kostenlos."
      intro="Jeder zusätzliche Kanal macht das Netzwerk für alle besser, deshalb kostet es nichts. Bezahlt wird nur, was Rechenzeit verbraucht oder dich im Netzwerk bevorzugt."
    >
      <FreePlan plan={free} />

      <div className="mt-6 grid gap-6 lg:grid-cols-2">
        {paid.map((plan, i) => (
          <PaidPlan key={plan.id} plan={plan} index={i} />
        ))}
      </div>

      <p className="mt-6 max-w-3xl leading-relaxed text-[var(--color-text-secondary)]">
        Wer zahlt, finanziert Server und Rechenzeit mit. Wer nicht zahlt, macht
        das Netzwerk trotzdem größer und ist genauso erwünscht.
      </p>
    </ProtocolSection>
  );
}

/** Einwaende offen aufgelistet, mit Beleg statt Beschwichtigung. */
export function ObjectionsSection() {
  return (
    <ProtocolSection
      id="einwaende"
      ambient="none"
      stamp="09 · Was du dich wahrscheinlich fragst"
      headline="Die unangenehmen Fragen zuerst."
      intro="Wenn dir eine Antwort nicht reicht, frag im Discord nach. Dort antworten Menschen, keine Vorlage."
    >
      <div className="space-y-3">
        {objections.map((item, i) => (
          <motion.details
            key={item.question}
            initial={{ opacity: 0, y: 16 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true, margin: "-60px" }}
            transition={{ duration: 0.45, delay: i * 0.06 }}
            className="v2-objection panel-card rounded-xl px-6 py-5"
          >
            <summary className="flex items-center justify-between gap-4">
              <span className="text-lg font-semibold text-[var(--color-text-primary)]">
                {item.question}
              </span>
              <span className="v2-objection-chevron shrink-0 text-[var(--color-primary)]">
                <Plus size={18} />
              </span>
            </summary>
            <div className="mt-4 max-w-3xl">
              <p className="font-medium text-[var(--color-primary-hover)]">
                {item.label}
              </p>
              <p className="mt-2.5 leading-relaxed text-[var(--color-text-secondary)]">
                {item.answer}
              </p>
              {item.proofHref && item.proofLabel ? (
                <a
                  href={resolveHref(item.proofHref)}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="mt-4 inline-flex items-center gap-1.5 text-sm font-semibold text-[var(--color-accent)] no-underline hover:text-[var(--color-accent-hover)]"
                >
                  {item.proofLabel}
                  <ArrowRight size={15} />
                </a>
              ) : null}
            </div>
          </motion.details>
        ))}
      </div>
    </ProtocolSection>
  );
}

/** Letzter Abschnitt: eine Entscheidung, zwei Wege. */
/**
 * Die Gesichter zum Schluss: echte Partnerkanaele als Avatarreihe ueber den
 * Knoepfen. Kein Beispielbild, kein Platzhalter — ohne geladene Partner
 * bleibt die Reihe schlicht weg.
 */
function PartnerFaces({ partners }: { partners: PartnerChannel[] }) {
  // Die Netzwerk-API liefert derzeit keine Profilbilder. Statt die Reihe
  // wegzulassen, tragen Kanaele ohne Bild ihr Monogramm — echte Logins,
  // nur ohne Foto.
  const shown = partners.slice(0, 9);
  if (shown.length < 3) return null;

  return (
    <div className="mb-9 flex flex-wrap items-center gap-4">
      <div className="flex -space-x-2">
        {shown.map((p) => (
          <a
            key={p.login}
            href={`https://twitch.tv/${p.login}`}
            target="_blank"
            rel="noopener noreferrer"
            title={p.displayName}
            className="relative block h-11 w-11 rounded-full ring-2 ring-[var(--color-bg,#0b0b0b)] transition-transform hover:z-10 hover:-translate-y-1"
          >
            {p.avatarUrl ? (
              <img
                src={p.avatarUrl}
                alt={p.displayName}
                width={44}
                height={44}
                loading="lazy"
                className="h-11 w-11 rounded-full object-cover"
              />
            ) : (
              <span className="flex h-11 w-11 items-center justify-center rounded-full border border-[rgba(239,212,157,0.3)] bg-[rgba(201,168,106,0.12)] text-xs font-bold text-[var(--color-primary-hover)]">
                {initials(p.login)}
              </span>
            )}
            {p.liveDeadlock ? (
              <span className="v2-pulse absolute -bottom-0.5 -right-0.5 h-3 w-3 rounded-full bg-[var(--color-success)] ring-2 ring-[var(--color-bg,#0b0b0b)]" />
            ) : null}
          </a>
        ))}
      </div>
      <span className="text-sm text-[var(--color-text-secondary)]">
        {partners.length} Kanäle sind schon dabei. Grüner Punkt heißt: läuft
        gerade.
      </span>
    </div>
  );
}

export function NetworkCta({ partners }: { partners: PartnerChannel[] }) {
  return (
    <ProtocolSection
      id="start"
      ambient="teal"
      ambientSide="right"
      stamp="10 · Der nächste Stream"
      headline={
        <>
          Dein nächster Stream endet sowieso.
          <br />
          <span
            className="bg-clip-text text-transparent"
            style={{ backgroundImage: "var(--gradient-brand)" }}
          >
            Die Frage ist nur, wohin.
          </span>
        </>
      }
    >
      <PartnerFaces partners={partners} />

      <div className="flex flex-wrap items-center gap-4">
        <a
          href={buildTwitchBotAuthUrl()}
          className="gradient-accent inline-flex w-full items-center justify-center gap-2 rounded-xl px-8 py-4 text-lg font-semibold no-underline transition-all hover:brightness-110 hover:shadow-[0_0_30px_5px_rgba(201,168,106,0.28)] sm:w-auto"
        >
          Jetzt kostenlos verbinden
          <ArrowRight size={19} />
        </a>
        <a
          href={DISCORD_INVITE_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex w-full items-center justify-center gap-2 rounded-xl border border-[rgba(255,255,255,0.14)] px-8 py-4 text-lg font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] sm:w-auto"
        >
          Erst im Discord fragen
        </a>
      </div>
      <p className="mt-6 max-w-2xl text-sm leading-relaxed text-[var(--color-text-secondary)]">
        Verbinden dauert etwa zwei Minuten. Trennen dauert einen Klick. Zwischen
        beidem läuft alles ohne dich.
      </p>
    </ProtocolSection>
  );
}
