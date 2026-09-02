import { motion } from "framer-motion";
import { initials } from "@/components/v2/NetworkLive";
import type { PartnerChannel } from "@/hooks/useNetworkMetrics";
import { ArrowRight, Check, Minus, Plus } from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import { objections, plans } from "@/data/networkPage";
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

export function PricingSection() {
  const [free, ...extras] = plans;

  return (
    <ProtocolSection
      id="preise"
      ambientSide="left"
      stamp="Optional mehr Tools"
      headline="Kostenlos Partner werden."
      intro="Die Partnerschaft kostet nichts, weil jeder zusätzliche Kanal das Netzwerk für alle besser macht."
    >
      <div className="panel-card v2-plan-featured rounded-2xl p-8 lg:p-10">
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div>
            <h3 className="text-2xl font-bold text-[var(--color-text-primary)]">
              {free.name}
            </h3>
            <p className="mt-2 text-sm leading-snug text-[rgba(183,170,145,0.62)]">
              {free.anchor}
            </p>
            <div className="mt-6 flex items-baseline gap-2">
              <span
                className="text-5xl font-extrabold leading-none bg-clip-text text-transparent"
                style={{ backgroundImage: "var(--gradient-brand)" }}
              >
                {free.price}
              </span>
              <span className="text-sm text-[var(--color-text-secondary)]">
                {free.period}
              </span>
            </div>
            <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
              keine Karte, keine Laufzeit
            </p>
          </div>
          <a
            href={buildTwitchBotAuthUrl()}
            className="gradient-accent inline-flex items-center justify-center gap-2 rounded-xl px-6 py-3 font-semibold no-underline transition-all hover:brightness-110"
          >
            Jetzt Partner werden
            <ArrowRight size={18} />
          </a>
        </div>
        <ul className="mt-8 grid gap-3 sm:grid-cols-2">
          {free.features.map((feature) => (
            <li
              key={feature.label}
              className={`flex gap-3 text-sm ${
                feature.included
                  ? "text-[var(--color-text-secondary)]"
                  : "text-[rgba(183,170,145,0.42)]"
              }`}
            >
              {feature.included ? (
                <Check
                  size={16}
                  className="mt-0.5 shrink-0 text-[var(--color-primary)]"
                />
              ) : (
                <Minus size={16} className="mt-0.5 shrink-0" />
              )}
              <span>{feature.label}</span>
            </li>
          ))}
        </ul>
      </div>

      <p className="v2-stamp mt-10">Optionale Extras</p>
      <div className="mt-4 grid gap-5 sm:grid-cols-2">
        {extras.map((plan) => (
          <div
            key={plan.id}
            className="v2-extra-card flex flex-col rounded-2xl p-6"
          >
            <div className="flex items-baseline justify-between gap-3">
              <h4 className="text-base font-bold text-[var(--color-text-primary)]">
                {plan.name}
              </h4>
              <span className="flex items-baseline gap-1">
                <span className="text-2xl font-extrabold text-[var(--color-text-primary)]">
                  {plan.price}
                </span>
                <span className="text-xs text-[var(--color-text-secondary)]">
                  {plan.period}
                </span>
              </span>
            </div>
            <p className="mt-1 text-sm text-[rgba(183,170,145,0.62)]">
              {plan.anchor}
            </p>
            <ul className="mt-4 flex-1 space-y-2">
              {plan.features.map((feature) => (
                <li
                  key={feature.label}
                  className={`flex gap-2.5 text-sm ${
                    feature.included
                      ? "text-[var(--color-text-secondary)]"
                      : "text-[rgba(183,170,145,0.42)]"
                  }`}
                >
                  {feature.included ? (
                    <Check
                      size={15}
                      className="mt-0.5 shrink-0 text-[var(--color-primary)]"
                    />
                  ) : (
                    <Minus size={15} className="mt-0.5 shrink-0" />
                  )}
                  <span>{feature.label}</span>
                </li>
              ))}
            </ul>
            {plan.note ? (
              <p className="mt-5 rounded-lg border border-[var(--color-border)] bg-black/25 px-3.5 py-2.5 text-xs leading-relaxed text-[var(--color-text-secondary)]">
                {plan.note}
              </p>
            ) : null}
            <a
              href={resolveHref(plan.ctaHref)}
              className="mt-5 inline-flex items-center justify-center gap-2 rounded-xl border border-[var(--color-border)] px-5 py-2.5 text-sm font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-border-hover)] hover:bg-white/5"
            >
              {plan.cta}
            </a>
          </div>
        ))}
      </div>
    </ProtocolSection>
  );
}

/** Einwaende offen aufgelistet, mit Beleg statt Beschwichtigung. */
export function ObjectionsSection() {
  return (
    <ProtocolSection
      id="einwaende"
      ambient="none"
      stamp="Was du dich wahrscheinlich fragst"
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
      stamp="Der nächste Stream"
      headline={
        <>
          Dein nächster Stream endet bei einem{" "}
          <span
            className="bg-clip-text text-transparent"
            style={{ backgroundImage: "var(--gradient-brand)" }}
          >
            Partner.
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
          Jetzt Partner werden
          <ArrowRight size={19} />
        </a>
        <a
          href={DISCORD_INVITE_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex w-full items-center justify-center gap-2 rounded-xl border border-[rgba(255,255,255,0.14)] px-8 py-4 text-lg font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] sm:w-auto"
        >
          Community-Discord beitreten
        </a>
      </div>
      <p className="mt-6 max-w-2xl text-sm leading-relaxed text-[var(--color-text-secondary)]">
        Verbinden dauert etwa zwei Minuten. Trennen dauert einen Klick. Zwischen
        beidem läuft alles ohne dich.
      </p>
    </ProtocolSection>
  );
}
