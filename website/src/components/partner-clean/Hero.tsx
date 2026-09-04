import { motion } from "framer-motion";
import { ExternalLink } from "lucide-react";
import { GradientText } from "@/components/ui/GradientText";
import { DiscordLogo } from "@/components/ui/DiscordLogo";
import { RaidDemo } from "@/components/sections/RaidDemo";
import { buildTwitchBotAuthUrl, DISCORD_INVITE_URL } from "@/data/externalLinks";

export function Hero() {
  return (
    <section
      id="hero"
      className="relative min-h-screen flex flex-col justify-center overflow-hidden"
    >
      <div className="max-w-[96rem] mx-auto px-6 pt-32 pb-20 w-full">
        <div className="text-center">
          <motion.div
            initial={{ opacity: 0, y: -12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.5 }}
            className="inline-flex items-center rounded-full px-4 py-1.5 bg-[var(--color-card)] border border-[var(--color-border)] text-sm text-[var(--color-accent)]"
          >
            Größtes Deadlock-Raid-Netzwerk auf Twitch
          </motion.div>

          <motion.h1
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.1 }}
            className="mt-6 text-5xl md:text-6xl lg:text-7xl font-bold leading-tight text-[var(--color-text-primary)]"
          >
            Kein Stream endet
            <br />
            <GradientText>im Leeren.</GradientText>
          </motion.h1>

          <motion.p
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6, delay: 0.2 }}
            className="mt-6 text-xl text-[var(--color-text-secondary)] max-w-2xl mx-auto"
          >
            Der Bot ist nur der Schlüssel. Sobald du ihn aktivierst, bist du
            Partner im deutschen Deadlock-Netzwerk. Gehst du offline, reicht das
            Netzwerk deine Zuschauer an den nächsten deutschen Deadlock-Stream
            weiter.
          </motion.p>
        </div>

        <div className="mt-16 mx-auto w-full max-w-[1400px]">
          <RaidDemo />
        </div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.5 }}
          className="mt-10 flex gap-4 justify-center flex-wrap"
        >
          <a
            href={buildTwitchBotAuthUrl()}
            className="gradient-accent rounded-xl px-7 py-3.5 font-semibold inline-flex items-center gap-2 transition-all duration-200 hover:brightness-110 hover:shadow-[0_0_24px_4px_rgba(201,168,106,0.3)]"
          >
            <ExternalLink size={18} />
            Jetzt Partner werden
          </a>
          <a
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="rounded-xl px-7 py-3.5 font-semibold text-[var(--color-text-primary)] border border-[rgba(255,255,255,0.14)] inline-flex items-center gap-2 transition-all duration-200 hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
          >
            <DiscordLogo size={18} />
            Community-Discord beitreten
          </a>
        </motion.div>
      </div>
    </section>
  );
}
