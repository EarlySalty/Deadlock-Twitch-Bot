import { motion, useInView } from "framer-motion";
import { useRef } from "react";
import { ArrowRight, Power, Search, Users } from "lucide-react";
import { ScrollReveal, useEinblendung } from "@/components/ui/ScrollReveal";
import { GradientText } from "@/components/ui/GradientText";
import { useNetworkStats } from "@/hooks/useNetworkStats";

const flowSchritte = [
  { titel: "Offline", icon: Power },
  { titel: "Partner", icon: Search },
  { titel: "Raid", icon: Users },
];

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
      <rect
        x="8"
        y="4"
        width="44"
        height="28"
        rx="3"
        stroke="rgba(183, 170, 145,0.35)"
        strokeWidth="1.5"
        fill="none"
      />
      <line
        x1="30"
        y1="32"
        x2="30"
        y2="37"
        stroke="rgba(183, 170, 145,0.35)"
        strokeWidth="1.5"
      />
      <line
        x1="22"
        y1="37"
        x2="38"
        y2="37"
        stroke="rgba(183, 170, 145,0.35)"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
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
      <circle cx="14" cy="20" r="4" fill="var(--color-primary)" opacity={0.8} />
      <circle cx="30" cy="10" r="3.5" fill="var(--color-accent)" opacity={0.8} />
      <circle cx="30" cy="30" r="3.5" fill="var(--color-accent)" opacity={0.8} />
      <circle cx="46" cy="20" r="4" fill="var(--color-primary)" opacity={0.8} />
    </svg>
  );
}

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
      <line
        x1="6"
        y1="20"
        x2="54"
        y2="20"
        stroke="rgba(183, 170, 145,0.2)"
        strokeWidth="1"
        strokeDasharray="3 3"
      />
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
      <circle cx="6" cy="20" r="4" fill="var(--color-primary)" opacity={0.5} />
      <circle cx="54" cy="20" r="4" fill="var(--color-accent)" opacity={0.5} />
    </svg>
  );
}

const schrittGrafiken = [OfflineSvg, NetworkSvg, ViewerFlowSvg];

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

export function ZuschauerWanderung() {
  const flow = useEinblendung<HTMLDivElement>();
  const stats = useNetworkStats();

  const kacheln = stats
    ? [
        { wert: stats.activePartners, label: "Aktive Partner" },
        { wert: stats.raidsTotal, label: "Raids gesamt" },
        { wert: stats.raids7d, label: "Raids, letzte 7 Tage" },
        {
          wert: stats.viewersForwardedTotal ?? 0,
          label: "Weitergereichte Zuschauer gesamt",
        },
      ].filter((kachel) => kachel.wert > 0)
    : [];

  return (
    <section id="raid" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
          <ScrollReveal>
            <p className="text-sm text-[var(--color-accent)] font-medium uppercase tracking-wider mb-4">
              Im Netz
            </p>

            <h2 className="text-3xl md:text-4xl font-bold text-[var(--color-text-primary)] mb-6">
              So wandern <GradientText>Zuschauer</GradientText> weiter
            </h2>

            <p className="text-[var(--color-text-secondary)] text-lg leading-relaxed">
              Endet ein Stream, sucht das Netz den passenden Live-Partner und
              reicht die Zuschauer direkt weiter.
            </p>

            {kacheln.length > 0 ? (
              <div className="mt-8 grid grid-cols-2 gap-4">
                {kacheln.map((kachel) => (
                  <div
                    key={kachel.label}
                    className="panel-card rounded-2xl p-5 text-center"
                  >
                    <p className="text-3xl font-bold text-[var(--color-accent)] tabular-nums">
                      {kachel.wert.toLocaleString("de-DE")}
                    </p>
                    <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
                      {kachel.label}
                    </p>
                  </div>
                ))}
              </div>
            ) : null}
          </ScrollReveal>

          <ScrollReveal delay={0.2}>
            <div className="panel-card rounded-2xl p-8">
              <p className="text-lg font-semibold text-[var(--color-text-primary)] mb-6">
                Was am Stream-Ende passiert
              </p>

              <motion.div
                ref={flow.ref}
                variants={containerVariants}
                initial={false}
                animate={flow.versteckt ? "hidden" : "visible"}
                transition={flow.versteckt ? { duration: 0 } : undefined}
                className="space-y-3"
              >
                {flowSchritte.map((schritt, idx) => {
                  const Icon = schritt.icon;
                  const Grafik = schrittGrafiken[idx];

                  return (
                    <motion.div
                      key={schritt.titel}
                      variants={rowVariants}
                      className="bg-[var(--color-card)] rounded-lg p-4 flex items-center gap-4"
                      style={{
                        border: "1px solid rgba(85, 151, 143, 0.55)",
                      }}
                    >
                      <div
                        className="w-11 h-11 rounded-xl shrink-0 flex items-center justify-center"
                        style={{
                          background:
                            "linear-gradient(135deg, rgba(201,168,106,0.26), rgba(85, 151, 143, 0.34))",
                          border: "1px solid rgba(85, 151, 143, 0.45)",
                        }}
                      >
                        <Icon size={18} className="text-[var(--color-accent-hover)]" />
                      </div>

                      <Grafik />

                      <p className="text-sm font-semibold text-[var(--color-text-primary)] min-w-0 flex-1">
                        {schritt.titel}
                      </p>
                    </motion.div>
                  );
                })}

                <motion.div
                  variants={rowVariants}
                  className="relative rounded-xl p-4"
                  style={{
                    border: "1px solid rgba(85, 151, 143, 0.35)",
                    background:
                      "linear-gradient(135deg, rgba(201,168,106,0.1), rgba(85, 151, 143, 0.08))",
                  }}
                >
                  <div className="relative">
                    <AnimatedFlowLine />
                    <div className="relative z-10 flex flex-wrap items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-accent)]">
                      <span className="bg-[rgba(16,11,7,0.85)] px-2 py-0.5 rounded">
                        Offline
                      </span>
                      <ArrowRight size={14} />
                      <span className="bg-[rgba(16,11,7,0.85)] px-2 py-0.5 rounded">
                        Partner
                      </span>
                      <ArrowRight size={14} />
                      <span className="bg-[rgba(16,11,7,0.85)] px-2 py-0.5 rounded">
                        Raid
                      </span>
                    </div>
                  </div>
                </motion.div>
              </motion.div>
            </div>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}
