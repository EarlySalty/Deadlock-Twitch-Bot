import { motion } from "framer-motion";
import { ArrowRight, Radio, ShieldCheck, Users } from "lucide-react";
import {
  DISCORD_INVITE_URL,
  buildTwitchBotAuthUrl,
} from "@/data/externalLinks";
import { NetworkRaidDemo } from "@/components/v2/NetworkRaidDemo";
import type { NetworkMetrics } from "@/hooks/useNetworkMetrics";

/**
 * Eine Kennzahl der Beweiszeile. Ohne Wert bleibt der Platz sichtbar leer,
 * es wird nie eine Zahl erfunden.
 */
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

/**
 * Hero der Landing V2.
 *
 * Die rechte Seite beherrscht den Raum: eine einzige Buehne, in der beide
 * Stream-Karten, die Zeitachse und die Statuszeile zusammen sitzen. Der Text
 * links ist bewusst auf Zeile, Satz und zwei Knoepfe eingedampft, damit die
 * Bewegung rechts der Blickfang bleibt.
 *
 * Die Lichtinseln (`v2-ambient`) sind Teil der Komposition, nicht Deko: die
 * goldene liegt hinter der Buehne, die tuerkise hinter der Beweiszeile, sodass
 * beide Farben des Netzwerks im ersten Bild vorkommen.
 */
export function NetworkHero({ metrics }: { metrics: NetworkMetrics }) {
  return (
    <section className="relative overflow-hidden pt-28 pb-10 sm:pt-32 sm:pb-16">
      <div className="relative mx-auto max-w-[84rem] px-6">
        <div className="mx-auto max-w-4xl text-center">
          <motion.span
            initial={{ opacity: 0, y: -10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.5 }}
            className="v2-chip inline-flex items-center gap-2"
          >
            <Radio size={13} />
            Das Partner-Netzwerk der deutschen Deadlock-Community
          </motion.span>

          <motion.h1
            initial={{ opacity: 0, y: 22 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.08 }}
            className="mt-5 text-[clamp(2.3rem,4.8vw,3.5rem)] font-extrabold leading-[1.03] tracking-[-0.02em] text-[var(--color-text-primary)]"
          >
            Werde Partner der deutschen
            <span
              className="block bg-clip-text text-transparent"
              style={{ backgroundImage: "var(--gradient-brand)" }}
            >
              Deadlock-Community.
            </span>
          </motion.h1>

          <motion.p
            initial={{ opacity: 0, y: 22 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.16 }}
            className="mx-auto mt-5 max-w-xl text-lg leading-relaxed text-[var(--color-text-secondary)]"
          >
            Der Bot ist nur der Schlüssel. Ab dem Moment bist du Partner, deine
            Viewer bleiben im Kreislauf.
          </motion.p>
        </div>

        <motion.div
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.3 }}
          className="relative mx-auto mt-7 max-w-[1400px]"
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
            Jetzt Partner werden
            <ArrowRight size={18} />
          </a>
          <a
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex w-full items-center justify-center gap-2 rounded-xl border border-[rgba(255,255,255,0.14)] px-6 py-3.5 font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] sm:w-auto"
          >
            Community-Discord beitreten
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
            label="Streamer im Netzwerk"
            settled={metrics.settled}
          />
          <ProofItem
            icon={<Radio size={17} />}
            value={metrics.liveNow}
            label={
              metrics.categoryKnown ? "gerade live in Deadlock" : "gerade live"
            }
            settled={metrics.settled}
          />
          <ProofItem
            icon={<ShieldCheck size={17} />}
            value={metrics.banStats?.total_30d ?? null}
            label="Spam-Accounts entfernt, 30 Tage"
            settled={metrics.settled}
          />
          <p className="w-full text-center text-xs text-[rgba(183,170,145,0.5)]">
            Die Kennzahlen kommen live aus dem laufenden Betrieb, die Bühne darüber zeigt Clips aus dem Netzwerk.
          </p>
        </motion.div>
      </div>
    </section>
  );
}
