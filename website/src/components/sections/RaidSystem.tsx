import { motion } from "framer-motion";
import { ArrowRight, CheckCircle2, Power, Search, Users } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { GradientText } from "@/components/ui/GradientText";

const features = [
  {
    title: "Fließende Übergänge",
    description:
      "Aus einer beendeten Session wird direkt der Einstieg in den nächsten passenden Live-Stream.",
  },
  {
    title: "Passende Partner-Ziele",
    description:
      "Bevorzugt werden aktive Partner aus dem Netzwerk, damit Zuschauer im Deadlock-Umfeld bleiben.",
  },
  {
    title: "Mehr Sichtbarkeit für alle",
    description:
      "Viewer werden sinnvoll weitergeleitet, sodass große und kleine Creator gemeinsam von mehr Discoverability profitieren.",
  },
  {
    title: "Volle Kontrolle",
    description:
      "Automatisierung, wenn sie hilft; manuelle Raids bleiben jederzeit Teil eures eigenen Ablaufs.",
  },
];

const flowSteps = [
  {
    title: "Offline",
    description:
      "Sobald ein Deadlock-Stream endet, übernimmt das System automatisch den nächsten Schritt.",
    icon: Power,
    highlight: true,
  },
  {
    title: "Partner",
    description:
      "Ein passender Live-Partner aus dem Netzwerk wird priorisiert, damit die Community im richtigen Umfeld bleibt.",
    icon: Search,
    highlight: true,
  },
  {
    title: "Raid",
    description:
      "Die Viewer werden direkt in den nächsten relevanten Stream weitergeleitet statt am Ende zu verlieren.",
    icon: Users,
    highlight: true,
  },
];

const containerVariants = {
  hidden: {},
  visible: {
    transition: {
      staggerChildren: 0.12,
    },
  },
};

const rowVariants = {
  hidden: { opacity: 0, x: 20 },
  visible: { opacity: 1, x: 0, transition: { duration: 0.45, ease: "easeOut" as const } },
};

export function RaidSystem() {
  return (
    <section id="raid" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">

          {/* LEFT — text */}
          <ScrollReveal>
            <p className="text-sm text-[var(--color-accent)] font-medium uppercase tracking-wider mb-4">
              Auto-Raid System
            </p>

            <h2 className="text-3xl md:text-4xl font-bold text-[var(--color-text-primary)] mb-6">
              <GradientText>Intelligentes</GradientText> Raid-System
            </h2>

            <p className="text-[var(--color-text-secondary)] text-lg mb-8 leading-relaxed">
              Unser Auto-Raid hält die Deadlock-Community in Bewegung. Endet ein
              Partner-Stream nach einer Deadlock-Session, sucht das System
              automatisch nach einem passenden Live-Partner und leitet die
              Community direkt weiter.
            </p>

            <p className="text-[var(--color-text-secondary)] text-base mb-8 leading-relaxed">
              Das sorgt für fließende Übergänge statt harter Stream-Enden: mehr
              Sichtbarkeit, mehr gemeinsame Reichweite und mehr echte
              Verbindungen im Netzwerk. Wer lieber selbst entscheidet, kann
              natürlich weiterhin manuell raiden.
            </p>

            <ul className="space-y-4">
              {features.map((feature) => (
                <li key={feature.title} className="flex items-start gap-3">
                  <CheckCircle2
                    size={20}
                    className="text-[var(--color-accent)] shrink-0 mt-1"
                  />
                  <span className="text-[var(--color-text-secondary)] leading-relaxed">
                    <strong className="text-[var(--color-text-primary)] font-semibold">
                      {feature.title}
                    </strong>{" "}
                    — {feature.description}
                  </span>
                </li>
              ))}
            </ul>
          </ScrollReveal>

          {/* RIGHT — visual mockup */}
          <ScrollReveal delay={0.2}>
            <div className="panel-card rounded-2xl p-8">
              <p className="text-lg font-semibold text-[var(--color-text-primary)] mb-6">
                Flow beim Offline-Gehen
              </p>

              <motion.div
                variants={containerVariants}
                initial="hidden"
                whileInView="visible"
                viewport={{ once: true, margin: "-60px" }}
                className="space-y-3"
              >
                {flowSteps.map((step) => {
                  const Icon = step.icon;

                  return (
                    <motion.div
                      key={step.title}
                      variants={rowVariants}
                      className="bg-[var(--color-card)] rounded-lg p-4 flex items-start gap-4"
                      style={{
                        border: step.highlight
                          ? "1px solid rgba(85, 151, 143, 0.35)"
                          : "1px solid var(--color-border)",
                      }}
                    >
                      <div
                        className="w-11 h-11 rounded-xl shrink-0 flex items-center justify-center"
                        style={{
                          background: step.highlight
                            ? "linear-gradient(135deg, rgba(201,168,106,0.22), rgba(85, 151, 143, 0.2))"
                            : "rgba(183, 170, 145,0.08)",
                          border: step.highlight
                            ? "1px solid rgba(85, 151, 143, 0.24)"
                            : "1px solid rgba(183, 170, 145,0.12)",
                        }}
                      >
                        <Icon
                          size={18}
                          className={
                            step.highlight
                              ? "text-[var(--color-accent)]"
                              : "text-[var(--color-text-secondary)]"
                          }
                        />
                      </div>

                      <div className="min-w-0 flex-1">
                        <p className="text-sm font-semibold text-[var(--color-text-primary)]">
                          {step.title}
                        </p>
                        <p className="text-xs text-[var(--color-text-secondary)] mt-2 leading-relaxed">
                          {step.description}
                        </p>
                      </div>
                    </motion.div>
                  );
                })}

                <motion.div
                  variants={rowVariants}
                  className="rounded-xl p-4"
                  style={{
                    border: "1px solid rgba(85, 151, 143, 0.2)",
                    background:
                      "linear-gradient(135deg, rgba(201,168,106,0.1), rgba(85, 151, 143, 0.08))",
                  }}
                >
                  <div className="flex flex-wrap items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-accent)]">
                    <span>Offline</span>
                    <ArrowRight size={14} />
                    <span>Partner</span>
                    <ArrowRight size={14} />
                    <span>Raid</span>
                  </div>

                  <p className="text-sm text-[var(--color-text-primary)] mt-3 leading-relaxed">
                    So bleibt eure Community in Bewegung und der Stream endet
                    nicht einfach im Leeren.
                  </p>
                </motion.div>
              </motion.div>
            </div>
          </ScrollReveal>

        </div>
      </div>
    </section>
  );
}
