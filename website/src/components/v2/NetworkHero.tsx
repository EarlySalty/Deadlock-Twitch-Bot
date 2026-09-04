import { motion } from "framer-motion";
import { ArrowRight, MessageCircle, Radio, ShieldCheck, Users } from "lucide-react";
import { buildTwitchBotAuthUrl, DISCORD_INVITE_URL } from "@/data/externalLinks";
import { HERO_COPY } from "@/data/networkPage";
import { NetworkRaidDemo } from "@/components/v2/NetworkRaidDemo";
import type { NetworkMetrics } from "@/hooks/useNetworkMetrics";

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
    <div className="flex items-center gap-3">
      <span className="text-[var(--color-primary)]">{icon}</span>
      <span className="flex flex-col leading-tight">
        <strong className="text-xl font-extrabold tabular-nums text-[var(--color-text-primary)]">
          {value !== null
            ? value.toLocaleString("de-DE")
            : settled
              ? "keine Daten"
              : "…"}
        </strong>
        <span className="text-xs text-[var(--color-text-secondary)]">
          {label}
        </span>
      </span>
    </div>
  );
}

export function NetworkHero({ metrics }: { metrics: NetworkMetrics }) {
  return (
    <section id="hero" className="relative overflow-hidden pt-28 pb-10 sm:pt-32 sm:pb-16">
      <div className="relative mx-auto max-w-[84rem] px-6">
        <div className="mx-auto max-w-3xl text-center">
          <motion.span
            initial={{ opacity: 0, y: -10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.5 }}
            className="v2-chip inline-flex items-center gap-2"
          >
            <Radio size={13} />
            {HERO_COPY.chip}
          </motion.span>

          <motion.h1
            initial={{ opacity: 0, y: 22 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.08 }}
            className="mt-6 text-[clamp(2.7rem,6vw,4.6rem)] font-extrabold leading-[0.95] tracking-[-0.02em] text-[var(--color-text-primary)]"
          >
            {HERO_COPY.headlineLead}{" "}
            <span
              className="bg-clip-text text-transparent"
              style={{ backgroundImage: "var(--gradient-brand)" }}
            >
              {HERO_COPY.headlineAccent}
            </span>
          </motion.h1>

          <motion.p
            initial={{ opacity: 0, y: 22 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.16 }}
            className="mx-auto mt-5 max-w-2xl text-lg leading-relaxed text-[var(--color-text-secondary)]"
          >
            {HERO_COPY.subline}
          </motion.p>
        </div>

        <motion.div
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.3 }}
          className="relative mx-auto mt-10 max-w-[1400px]"
        >
          <NetworkRaidDemo partners={metrics.partnerList} />
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 22 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.24 }}
          className="mt-8 flex flex-wrap items-center justify-center gap-4"
        >
          <a
            href={buildTwitchBotAuthUrl()}
            className="gradient-accent inline-flex w-full items-center justify-center gap-2 rounded-xl px-6 py-3.5 font-semibold no-underline transition-all hover:brightness-110 hover:shadow-[0_0_28px_4px_rgba(201,168,106,0.28)] sm:w-auto"
          >
            {HERO_COPY.ctaPrimary}
            <ArrowRight size={18} />
          </a>
          <a
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex w-full items-center justify-center gap-2 rounded-xl border border-[rgba(255,255,255,0.14)] px-6 py-3.5 font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] sm:w-auto"
          >
            <MessageCircle size={18} />
            {HERO_COPY.ctaSecondary}
          </a>
        </motion.div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.6, delay: 0.45 }}
          className="mx-auto mt-10 flex max-w-4xl flex-wrap items-center justify-center gap-x-10 gap-y-5 border-t border-[rgba(239,212,157,0.14)] pt-6"
        >
          <ProofItem
            icon={<Users size={17} />}
            value={metrics.partners}
            label={HERO_COPY.proofPartners}
            settled={metrics.settled}
          />
          <ProofItem
            icon={<Radio size={17} />}
            value={metrics.liveNow}
            label={
              metrics.categoryKnown ? HERO_COPY.proofLiveKnown : HERO_COPY.proofLive
            }
            settled={metrics.settled}
          />
          <ProofItem
            icon={<ShieldCheck size={17} />}
            value={metrics.banStats?.total_30d ?? null}
            label={HERO_COPY.proofBans}
            settled={metrics.settled}
          />
          <p className="w-full text-center text-xs text-[rgba(183,170,145,0.5)]">
            {HERO_COPY.proofNote}
          </p>
        </motion.div>
      </div>
    </section>
  );
}
