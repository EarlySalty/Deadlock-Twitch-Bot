import { motion } from "framer-motion";
import { ArrowRight, Radio, ShieldCheck, Users } from "lucide-react";
import { buildTwitchBotAuthUrl } from "@/data/externalLinks";
import type { NetworkMetrics } from "@/hooks/useNetworkMetrics";

/** Eine Kennzahl der Beweiszeile. Ohne Wert bleibt der Platz sichtbar leer. */
function ProofItem({
  icon,
  value,
  label,
  settled,
}: {
  icon: React.ReactNode;
  value: number | null;
  label: string;
  settled: boolean;
}) {
  return (
    <div className="flex items-center gap-2.5">
      <span className="text-[var(--color-primary)]">{icon}</span>
      <span className="text-sm text-[var(--color-text-secondary)]">
        <strong className="font-semibold text-[var(--color-text-primary)]">
          {value !== null ? value.toLocaleString("de-DE") : settled ? "keine Daten" : "…"}
        </strong>{" "}
        {label}
      </span>
    </div>
  );
}

/**
 * Der Beispielablauf rechts im Hero. Zeigt in vier Zeilen, was am Stream-Ende
 * passiert. Die Uhrzeiten sind als Beispiel gekennzeichnet, damit sie niemand
 * fuer gemessene Werte haelt.
 */
function HandoverCard() {
  const rows = [
    { time: "23:47:00", text: "Dein Stream endet", tone: "muted" as const },
    { time: "23:47:01", text: "Netzwerk sucht einen passenden Partner", tone: "muted" as const },
    { time: "23:47:03", text: "Deine Zuschauer landen im nächsten Deadlock-Stream", tone: "gold" as const },
    { time: "morgen", text: "Ein anderer Stream endet, du bekommst Zuschauer zurück", tone: "teal" as const },
  ];

  return (
    <div className="panel-card rounded-2xl p-6 sm:p-7">
      <div className="flex items-center justify-between">
        <span className="v2-stamp">Beispielablauf</span>
        <span className="flex items-center gap-2">
          <span className="v2-pulse h-2 w-2 rounded-full bg-[var(--color-success)]" />
          <span className="text-xs text-[var(--color-text-secondary)]">Netzwerk aktiv</span>
        </span>
      </div>

      <div className="mt-6 space-y-4">
        {rows.map((row, i) => (
          <motion.div
            key={row.time}
            initial={{ opacity: 0, x: -10 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.5, delay: 0.5 + i * 0.18 }}
            className="flex gap-4"
          >
            <span className="v2-stamp v2-stamp-dim w-16 shrink-0 pt-0.5">{row.time}</span>
            <span
              className={`text-sm leading-relaxed ${
                row.tone === "gold"
                  ? "text-[var(--color-primary-hover)]"
                  : row.tone === "teal"
                    ? "text-[var(--color-accent-hover)]"
                    : "text-[var(--color-text-secondary)]"
              }`}
            >
              {row.text}
            </span>
          </motion.div>
        ))}
      </div>

      {/* Die Uebergabe als Bild: Punkte wandern vom linken zum rechten Kanal. */}
      <div className="mt-7 rounded-xl border border-[var(--color-border)] bg-black/25 p-4">
        <div className="flex items-center justify-between text-xs text-[var(--color-text-secondary)]">
          <span>dein Kanal</span>
          <span>Partnerkanal</span>
        </div>
        <div className="relative mt-3 h-6">
          <div className="absolute inset-x-0 top-1/2 h-px -translate-y-1/2 bg-gradient-to-r from-[rgba(201,168,106,0.5)] via-[rgba(201,168,106,0.22)] to-[rgba(85,151,143,0.5)]" />
          <div className="relative flex h-full items-center gap-2">
            {[0, 1, 2, 3, 4].map((i) => (
              <span
                key={i}
                className="v2-relay-dot"
                style={{
                  animationDelay: `${i * 0.22}s`,
                  ["--v2-relay-distance" as string]: "min(58vw, 320px)",
                }}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

export function NetworkHero({ metrics }: { metrics: NetworkMetrics }) {
  return (
    <section className="relative overflow-hidden pt-36 pb-20 sm:pt-44 sm:pb-28">
      <div className="mx-auto grid max-w-[84rem] items-center gap-14 px-6 lg:grid-cols-[1.05fr_0.95fr]">
        <div>
          <motion.span
            initial={{ opacity: 0, y: -10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.5 }}
            className="inline-flex items-center gap-2 rounded-full border border-[var(--color-border)] bg-[var(--color-card)] px-4 py-1.5 text-sm text-[var(--color-accent)]"
          >
            <Radio size={14} />
            Für deutschsprachige Deadlock-Streamer
          </motion.span>

          <motion.h1
            initial={{ opacity: 0, y: 22 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.08 }}
            className="mt-7 text-5xl font-extrabold leading-[0.98] tracking-tight text-[var(--color-text-primary)] sm:text-6xl lg:text-7xl"
          >
            Kein Stream
            <br />
            endet im{" "}
            <span
              className="bg-clip-text text-transparent"
              style={{ backgroundImage: "var(--gradient-brand)" }}
            >
              Leeren.
            </span>
          </motion.h1>

          <motion.p
            initial={{ opacity: 0, y: 22 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.16 }}
            className="mt-7 max-w-xl text-lg leading-relaxed text-[var(--color-text-secondary)] sm:text-xl"
          >
            Das kostenlose Wachstums-Netzwerk für deutschsprachige
            Deadlock-Streamer: Auto-Raids beim Stream-Ende, Schutz im Chat,
            Auswertung nach dem Stream und Clips, die von allein entstehen.
          </motion.p>

          <motion.div
            initial={{ opacity: 0, y: 22 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.24 }}
            className="mt-9 flex flex-wrap items-center gap-4"
          >
            <a
              href={buildTwitchBotAuthUrl()}
              className="gradient-accent inline-flex w-full items-center justify-center gap-2 rounded-xl px-7 py-3.5 font-semibold no-underline transition-all hover:brightness-110 hover:shadow-[0_0_28px_4px_rgba(201,168,106,0.28)] sm:w-auto"
            >
              Jetzt kostenlos verbinden
              <ArrowRight size={18} />
            </a>
            <a
              href="#report"
              className="inline-flex w-full items-center justify-center gap-2 rounded-xl border border-[rgba(255,255,255,0.14)] px-7 py-3.5 font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] sm:w-auto"
            >
              Kanal-Report holen
            </a>
          </motion.div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.6, delay: 0.4 }}
            className="mt-10 flex flex-wrap items-center gap-x-7 gap-y-3"
          >
            <ProofItem
              icon={<Users size={16} />}
              value={metrics.partners}
              label="Streamer im Netzwerk"
              settled={metrics.settled}
            />
            <ProofItem
              icon={<Radio size={16} />}
              value={metrics.liveNow}
              label="gerade live"
              settled={metrics.settled}
            />
            <ProofItem
              icon={<ShieldCheck size={16} />}
              value={metrics.banStats?.total_30d ?? null}
              label="Spam-Accounts entfernt, 30 Tage"
              settled={metrics.settled}
            />
          </motion.div>
          <p className="mt-3 text-xs text-[var(--color-subtle,rgba(183,170,145,0.55))]">
            Alle Zahlen auf dieser Seite kommen live aus dem laufenden Betrieb.
          </p>
        </div>

        <motion.div
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.3 }}
        >
          <HandoverCard />
        </motion.div>
      </div>
    </section>
  );
}
