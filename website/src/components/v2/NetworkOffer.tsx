import { motion } from "framer-motion";
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

/** Zwei Stufen: Free und Premium, die empfohlene rechts. */
export function PricingSection() {
  return (
    <ProtocolSection
      id="preise"
      stamp="06 · Preise"
      headline="Kostenlos bleibt kostenlos."
      intro="Das Netzwerk und der Schutz kosten nichts, weil jeder zusätzliche Kanal das Netzwerk für alle besser macht. Bezahlt wird nur, was Rechenzeit oder Bevorzugung verbraucht."
    >
      <div className="mx-auto grid max-w-4xl gap-6 lg:grid-cols-2">
        {plans.map((plan, i) => (
          <motion.div
            key={plan.id}
            initial={{ opacity: 0, y: 26 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true, margin: "-70px" }}
            transition={{ duration: 0.55, delay: i * 0.09 }}
            className={`panel-card flex flex-col rounded-2xl p-8 ${
              plan.featured ? "v2-plan-featured lg:-mt-4 lg:mb-4" : ""
            }`}
          >
            {plan.featured ? (
              <span className="gradient-accent mb-5 self-start rounded-full px-3 py-1 text-xs font-bold uppercase tracking-wider">
                Empfohlen
              </span>
            ) : null}

            <h3 className="text-lg font-bold text-[var(--color-text-primary)]">
              {plan.name}
            </h3>
            {/* Feste Hoehe, damit der Preis in allen drei Karten auf einer
                Linie steht, auch wenn der Anker zweizeilig umbricht. */}
            <p className="mt-1.5 min-h-[2.6rem] text-sm leading-snug text-[rgba(183,170,145,0.62)]">
              {plan.anchor}
            </p>

            <div className="mt-6 flex items-baseline gap-2">
              <span
                className="text-5xl font-extrabold leading-none bg-clip-text text-transparent"
                style={{ backgroundImage: "var(--gradient-brand)" }}
              >
                {plan.price}
              </span>
              <span className="text-sm text-[var(--color-text-secondary)]">
                {plan.period}
              </span>
            </div>
            {plan.yearly ? (
              <p className="mt-2 text-sm text-[var(--color-accent)]">
                {plan.yearly}
              </p>
            ) : (
              <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
                keine Karte, keine Laufzeit
              </p>
            )}

            <ul className="mt-7 flex-1 space-y-3">
              {plan.features.map((feature) => (
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

            {plan.note ? (
              <p className="mt-6 rounded-lg border border-[var(--color-border)] bg-black/25 px-3.5 py-2.5 text-xs leading-relaxed text-[var(--color-text-secondary)]">
                {plan.note}
              </p>
            ) : null}

            <a
              href={resolveHref(plan.ctaHref)}
              className={`mt-6 inline-flex items-center justify-center gap-2 rounded-xl px-6 py-3 font-semibold no-underline transition-all ${
                plan.featured
                  ? "gradient-accent hover:brightness-110"
                  : "border border-[var(--color-border)] text-[var(--color-text-primary)] hover:border-[var(--color-border-hover)] hover:bg-white/5"
              }`}
            >
              {plan.cta}
            </a>
          </motion.div>
        ))}
      </div>

      {/* Preiswürde statt Entschuldigung: warum überhaupt Geld verlangt wird. */}
      <div className="panel-card mt-8 rounded-2xl p-8">
        <h3 className="text-xl font-bold text-[var(--color-text-primary)]">
          Warum kostet überhaupt etwas Geld?
        </h3>
        <p className="mt-4 max-w-3xl leading-relaxed text-[var(--color-text-secondary)]">
          Server laufen rund um die Uhr, das Schneiden und Hochladen von Clips
          verbraucht Rechenzeit, und die Weiterentwicklung passiert neben dem
          Studium und der Arbeit. Wer zahlt, finanziert genau das mit. Wer nicht
          zahlt, macht das Netzwerk trotzdem größer und ist deshalb genauso
          erwünscht.
        </p>
        <p className="mt-4 max-w-3xl leading-relaxed text-[var(--color-text-secondary)]">
          Wir rabattieren nicht. Günstiger wird es nur gegen eine Gegenleistung,
          etwa eine Empfehlung, die ankommt, oder einen Erfahrungsbericht mit
          echten Zahlen.
        </p>
      </div>
    </ProtocolSection>
  );
}

/** Einwaende offen aufgelistet, mit Beleg statt Beschwichtigung. */
export function ObjectionsSection() {
  return (
    <ProtocolSection
      id="einwaende"
      stamp="07 · Was du dich wahrscheinlich fragst"
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
export function NetworkCta() {
  return (
    <ProtocolSection
      id="start"
      stamp="08 · Der nächste Stream"
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
