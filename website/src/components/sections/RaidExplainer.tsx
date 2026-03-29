import { motion, useInView } from "framer-motion";
import { useRef } from "react";
import { ArrowRight, CheckCircle2, Power, Search, Users } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { GradientText } from "@/components/ui/GradientText";

/* ── data ─────────────────────────────────────────────────────────── */

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
  },
  {
    title: "Partner",
    description:
      "Ein passender Live-Partner aus dem Netzwerk wird priorisiert, damit die Community im richtigen Umfeld bleibt.",
    icon: Search,
  },
  {
    title: "Raid",
    description:
      "Die Viewer werden direkt in den nächsten relevanten Stream weitergeleitet statt am Ende zu verlieren.",
    icon: Users,
  },
];

/* ── animation variants ───────────────────────────────────────────── */

const containerVariants = {
  hidden: {},
  visible: { transition: { staggerChildren: 0.12 } },
};

const rowVariants = {
  hidden: { opacity: 0, x: 20 },
  visible: {
    opacity: 1,
    x: 0,
    transition: { duration: 0.45, ease: "easeOut" as const },
  },
};

/* ── mini SVG illustrations ───────────────────────────────────────── */

/** Monitor that "turns off" (shrinking circle) */
function OfflineSvg() {
  return (
    <svg
      width="60"
      height="40"
      viewBox="0 0 60 40"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className="shrink-0"
    >
      {/* Monitor frame */}
      <rect
        x="8"
        y="4"
        width="44"
        height="28"
        rx="3"
        stroke="rgba(155,179,197,0.35)"
        strokeWidth="1.5"
        fill="none"
      />
      {/* Stand */}
      <line
        x1="30"
        y1="32"
        x2="30"
        y2="37"
        stroke="rgba(155,179,197,0.35)"
        strokeWidth="1.5"
      />
      <line
        x1="22"
        y1="37"
        x2="38"
        y2="37"
        stroke="rgba(155,179,197,0.35)"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
      {/* Shrinking circle (screen turning off) */}
      <motion.circle
        cx="30"
        cy="18"
        fill="var(--color-accent)"
        initial={{ r: 8, opacity: 0.7 }}
        animate={{ r: [8, 2, 8], opacity: [0.7, 0.15, 0.7] }}
        transition={{ duration: 3, repeat: Infinity, ease: "easeInOut" }}
      />
    </svg>
  );
}

/** 3 nodes with pulsing connection lines (mini-network) */
function NetworkSvg() {
  return (
    <svg
      width="60"
      height="40"
      viewBox="0 0 60 40"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className="shrink-0"
    >
      {/* Connection lines */}
      <motion.line
        x1="14"
        y1="20"
        x2="30"
        y2="10"
        stroke="var(--color-accent)"
        strokeWidth="1.2"
        initial={{ opacity: 0.2 }}
        animate={{ opacity: [0.2, 0.7, 0.2] }}
        transition={{ duration: 2, repeat: Infinity, ease: "easeInOut" }}
      />
      <motion.line
        x1="14"
        y1="20"
        x2="30"
        y2="30"
        stroke="var(--color-accent)"
        strokeWidth="1.2"
        initial={{ opacity: 0.2 }}
        animate={{ opacity: [0.2, 0.7, 0.2] }}
        transition={{
          duration: 2,
          repeat: Infinity,
          ease: "easeInOut",
          delay: 0.4,
        }}
      />
      <motion.line
        x1="30"
        y1="10"
        x2="46"
        y2="20"
        stroke="var(--color-accent)"
        strokeWidth="1.2"
        initial={{ opacity: 0.2 }}
        animate={{ opacity: [0.2, 0.7, 0.2] }}
        transition={{
          duration: 2,
          repeat: Infinity,
          ease: "easeInOut",
          delay: 0.8,
        }}
      />
      <motion.line
        x1="30"
        y1="30"
        x2="46"
        y2="20"
        stroke="var(--color-accent)"
        strokeWidth="1.2"
        initial={{ opacity: 0.2 }}
        animate={{ opacity: [0.2, 0.7, 0.2] }}
        transition={{
          duration: 2,
          repeat: Infinity,
          ease: "easeInOut",
          delay: 1.2,
        }}
      />
      {/* Nodes */}
      <circle cx="14" cy="20" r="4" fill="var(--color-primary)" opacity={0.8} />
      <circle cx="30" cy="10" r="3.5" fill="var(--color-accent)" opacity={0.8} />
      <circle cx="30" cy="30" r="3.5" fill="var(--color-accent)" opacity={0.8} />
      <circle cx="46" cy="20" r="4" fill="var(--color-primary)" opacity={0.8} />
    </svg>
  );
}

/** Dots flowing left → right (viewer flow) */
function ViewerFlowSvg() {
  const dotCount = 4;
  return (
    <svg
      width="60"
      height="40"
      viewBox="0 0 60 40"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className="shrink-0"
    >
      {/* Track line */}
      <line
        x1="6"
        y1="20"
        x2="54"
        y2="20"
        stroke="rgba(155,179,197,0.2)"
        strokeWidth="1"
        strokeDasharray="3 3"
      />
      {/* Flowing dots */}
      {Array.from({ length: dotCount }).map((_, i) => (
        <motion.circle
          key={i}
          cy="20"
          r="3"
          fill="var(--color-accent)"
          initial={{ cx: 6, opacity: 0 }}
          animate={{ cx: [6, 54], opacity: [0, 0.9, 0.9, 0] }}
          transition={{
            duration: 2.5,
            repeat: Infinity,
            ease: "easeInOut",
            delay: i * 0.6,
          }}
        />
      ))}
      {/* Source icon (left) */}
      <circle cx="6" cy="20" r="4" fill="var(--color-primary)" opacity={0.5} />
      {/* Target icon (right) */}
      <circle cx="54" cy="20" r="4" fill="var(--color-accent)" opacity={0.5} />
    </svg>
  );
}

const stepIllustrations = [OfflineSvg, NetworkSvg, ViewerFlowSvg];

/* ── animated flow-line for summary ───────────────────────────────── */

function AnimatedFlowLine() {
  const ref = useRef<SVGSVGElement>(null);
  const inView = useInView(ref, { once: true, margin: "-40px" });

  return (
    <svg
      ref={ref}
      width="100%"
      height="4"
      viewBox="0 0 300 4"
      preserveAspectRatio="none"
      className="absolute top-1/2 left-0 right-0 -translate-y-1/2 pointer-events-none"
      style={{ zIndex: 0 }}
    >
      <motion.line
        x1="0"
        y1="2"
        x2="300"
        y2="2"
        stroke="var(--color-accent)"
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray="300"
        strokeDashoffset={inView ? 0 : 300}
        initial={{ strokeDashoffset: 300 }}
        animate={inView ? { strokeDashoffset: 0 } : undefined}
        transition={{ duration: 1.2, ease: "easeInOut" }}
        opacity={0.35}
      />
    </svg>
  );
}

/* ── component ────────────────────────────────────────────────────── */

export function RaidExplainer() {
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
                {flowSteps.map((step, idx) => {
                  const Icon = step.icon;
                  const Illustration = stepIllustrations[idx];

                  return (
                    <motion.div
                      key={step.title}
                      variants={rowVariants}
                      className="bg-[var(--color-bg)]/50 rounded-lg p-4 flex items-start gap-4"
                      style={{
                        border: "1px solid rgba(16,183,173,0.35)",
                      }}
                    >
                      {/* Icon */}
                      <div
                        className="w-11 h-11 rounded-xl shrink-0 flex items-center justify-center"
                        style={{
                          background:
                            "linear-gradient(135deg, rgba(255,122,24,0.22), rgba(16,183,173,0.2))",
                          border: "1px solid rgba(16,183,173,0.24)",
                        }}
                      >
                        <Icon size={18} className="text-[var(--color-accent)]" />
                      </div>

                      {/* SVG illustration */}
                      <Illustration />

                      {/* Text */}
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

                {/* Summary block with animated connecting line */}
                <motion.div
                  variants={rowVariants}
                  className="relative rounded-xl p-4"
                  style={{
                    border: "1px solid rgba(16,183,173,0.2)",
                    background:
                      "linear-gradient(135deg, rgba(255,122,24,0.1), rgba(16,183,173,0.08))",
                  }}
                >
                  {/* Animated line behind the steps */}
                  <div className="relative">
                    <AnimatedFlowLine />
                    <div className="relative z-10 flex flex-wrap items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-accent)]">
                      <span className="bg-[var(--color-bg)]/80 px-2 py-0.5 rounded">
                        Offline
                      </span>
                      <ArrowRight size={14} />
                      <span className="bg-[var(--color-bg)]/80 px-2 py-0.5 rounded">
                        Partner
                      </span>
                      <ArrowRight size={14} />
                      <span className="bg-[var(--color-bg)]/80 px-2 py-0.5 rounded">
                        Raid
                      </span>
                    </div>
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
