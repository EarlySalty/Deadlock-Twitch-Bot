import { motion } from "framer-motion";
import {
  Clapperboard,
  Check,
  ShieldCheck,
  Sparkles,
  Swords,
} from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import { planSteps, valuePillars } from "@/data/networkPage";
import type { ValuePillar } from "@/data/networkPage";

const PILLAR_ICONS = {
  Swords,
  ShieldCheck,
  Sparkles,
  Clapperboard,
} as const;

/**
 * Der Gegner der Geschichte: die Leere nach dem Stream-Ende. Zwei Karten
 * zeigen denselben Moment einmal ohne und einmal mit Netzwerk. Links
 * verglimmen die Zuschauerpunkte, rechts wandern sie weiter.
 */
export function VoidSection() {
  return (
    <ProtocolSection
      id="leere"
      stamp="01 · Der Moment, um den es geht"
      headline={
        <>
          Du klickst auf „Stream beenden“.{" "}
          <span className="text-[var(--color-text-secondary)]">
            Und dann?
          </span>
        </>
      }
      intro={
        <>
          Für die meisten kleinen Kanäle ist das der teuerste Moment des Abends.
          Die Leute, die gerade noch da waren, sind weg. Beim nächsten Mal
          fängst du wieder von vorne an.
        </>
      }
    >
      <div className="grid gap-6 lg:grid-cols-2">
        {/* Ohne Netzwerk */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true, margin: "-80px" }}
          transition={{ duration: 0.55 }}
          className="rounded-2xl border border-[rgba(255,255,255,0.07)] bg-black/25 p-7"
        >
          <span className="v2-stamp v2-stamp-dim">ohne Netzwerk</span>
          <h3 className="mt-4 text-2xl font-bold text-[var(--color-text-secondary)]">
            Sie versickern.
          </h3>
          <p className="mt-3 text-[var(--color-text-secondary)]">
            Der Kanal geht offline, die Zuschauer verteilen sich irgendwohin.
            Was du an einem Abend aufgebaut hast, ist am nächsten Abend nicht
            mehr da.
          </p>
          <div className="mt-8 flex h-16 items-center gap-2.5">
            {[0, 1, 2, 3, 4, 5, 6, 7].map((i) => (
              <span
                key={i}
                className="v2-void-dot"
                style={{ animationDelay: `${i * 0.16}s` }}
              />
            ))}
          </div>
        </motion.div>

        {/* Mit Netzwerk */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true, margin: "-80px" }}
          transition={{ duration: 0.55, delay: 0.12 }}
          className="panel-card rounded-2xl p-7"
        >
          <span className="v2-stamp">mit Netzwerk</span>
          <h3 className="mt-4 text-2xl font-bold text-[var(--color-text-primary)]">
            Sie werden übergeben.
          </h3>
          <p className="mt-3 text-[var(--color-text-secondary)]">
            Dein Publikum landet bei einem anderen deutschen Deadlock-Stream,
            der gerade läuft. Es bleibt in der Community. Und wenn dort Schluss
            ist, kommt es zu dir oder zum nächsten Partner.
          </p>
          <div className="relative mt-8 h-16">
            <div className="absolute inset-x-0 top-1/2 h-px -translate-y-1/2 bg-gradient-to-r from-[rgba(201,168,106,0.45)] to-[rgba(85,151,143,0.45)]" />
            <div className="relative flex h-full items-center gap-2.5">
              {[0, 1, 2, 3, 4, 5, 6, 7].map((i) => (
                <span
                  key={i}
                  className="v2-relay-dot"
                  style={{
                    animationDelay: `${i * 0.16}s`,
                    ["--v2-relay-distance" as string]: "min(46vw, 230px)",
                  }}
                />
              ))}
            </div>
          </div>
        </motion.div>
      </div>
    </ProtocolSection>
  );
}

/** Miller-Plan: drei Schritte, damit der Weg vor dem Klick klar ist. */
export function PlanSection() {
  return (
    <ProtocolSection
      id="ablauf"
      stamp="02 · So kommst du rein"
      headline="Drei Schritte, dann läuft es ohne dich."
      intro="Kein Setup-Wochenende, keine Konfigurationsdatei. Du verbindest deinen Kanal und entscheidest, was an sein soll."
    >
      <div className="grid gap-5 md:grid-cols-3">
        {planSteps.map((step, i) => (
          <motion.div
            key={step.index}
            initial={{ opacity: 0, y: 22 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true, margin: "-70px" }}
            transition={{ duration: 0.5, delay: i * 0.1 }}
            className="panel-card soft-elevate rounded-2xl p-7"
          >
            <div className="flex items-baseline justify-between">
              <span
                className="text-4xl font-extrabold leading-none bg-clip-text text-transparent"
                style={{ backgroundImage: "var(--gradient-brand)" }}
              >
                {step.index}
              </span>
              <span className="v2-stamp v2-stamp-dim">{step.duration}</span>
            </div>
            <h3 className="mt-5 text-xl font-bold text-[var(--color-text-primary)]">
              {step.title}
            </h3>
            <p className="mt-3 leading-relaxed text-[var(--color-text-secondary)]">
              {step.body}
            </p>
          </motion.div>
        ))}
      </div>
    </ProtocolSection>
  );
}

function PillarCard({ pillar, index }: { pillar: ValuePillar; index: number }) {
  const Icon = PILLAR_ICONS[pillar.icon];
  const accent =
    pillar.tone === "primary"
      ? "var(--color-primary)"
      : "var(--color-accent)";

  return (
    <motion.article
      initial={{ opacity: 0, y: 24 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-70px" }}
      transition={{ duration: 0.55, delay: (index % 2) * 0.1 }}
      className="panel-card soft-elevate rounded-2xl p-8"
    >
      <div className="flex items-center gap-4">
        <span
          className="icon-tile flex h-11 w-11 items-center justify-center rounded-xl"
          style={{ color: accent }}
        >
          <Icon size={20} />
        </span>
        <div>
          <p className="v2-stamp" style={{ color: accent, opacity: 0.85 }}>
            {pillar.kicker}
          </p>
          <h3 className="text-xl font-bold text-[var(--color-text-primary)]">
            {pillar.title}
          </h3>
        </div>
      </div>

      <p className="mt-5 leading-relaxed text-[var(--color-text-secondary)]">
        {pillar.body}
      </p>

      <ul className="mt-6 space-y-2.5">
        {pillar.points.map((point) => (
          <li
            key={point}
            className="flex gap-3 text-sm text-[var(--color-text-secondary)]"
          >
            <Check size={16} className="mt-0.5 shrink-0" style={{ color: accent }} />
            <span>{point}</span>
          </li>
        ))}
      </ul>
    </motion.article>
  );
}

/** Vier Leistungen in der Reihenfolge der Strategie: Raids zuerst. */
export function PillarsSection() {
  return (
    <ProtocolSection
      id="leistungen"
      stamp="03 · Was du bekommst"
      headline="Vier Dinge, die du sonst selbst machen müsstest."
      intro="Der Bot ist der Liefermechanismus. Der eigentliche Wert liegt darin, dass hinter deinem Kanal andere Kanäle stehen."
    >
      <div className="grid gap-6 lg:grid-cols-2">
        {valuePillars.map((pillar, i) => (
          <PillarCard key={pillar.id} pillar={pillar} index={i} />
        ))}
      </div>
    </ProtocolSection>
  );
}
