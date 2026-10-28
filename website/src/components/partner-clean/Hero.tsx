import { landingText } from "@/lib/landingLocale";
import { motion } from "framer-motion";
import { ExternalLink } from "lucide-react";
import { GradientText } from "@/components/ui/GradientText";
import { DiscordLogo } from "@/components/ui/DiscordLogo";
import { RaidDemo } from "@/components/sections/RaidDemo";
import { useEinblendung } from "@/components/ui/ScrollReveal";
import {
  buildTwitchBotAuthUrl,
  DISCORD_INVITE_URL,
} from "@/data/externalLinks";

export function Hero() {
  const titel = useEinblendung<HTMLHeadingElement>();
  const text = useEinblendung<HTMLParagraphElement>();
  const cta = useEinblendung<HTMLDivElement>();
  return (
    <section
      id="hero"
      className="relative min-h-screen flex flex-col justify-center overflow-hidden"
    >
      <div className="max-w-[96rem] mx-auto px-6 pt-32 pb-20 w-full">
        <div className="text-center">
          <motion.h1
            ref={titel.ref}
            initial={false}
            animate={{
              opacity: titel.versteckt ? 0 : 1,
              y: titel.versteckt ? 20 : 0,
            }}
            transition={
              titel.versteckt ? { duration: 0 } : { duration: 0.6, delay: 0.1 }
            }
            className="text-5xl md:text-6xl lg:text-7xl font-bold leading-tight text-[var(--color-text-primary)]"
          >
            {landingText("Kein Stream endet")}
            <br />
            <GradientText>{landingText("im Leeren.")}</GradientText>
          </motion.h1>

          <motion.p
            ref={text.ref}
            initial={false}
            animate={{
              opacity: text.versteckt ? 0 : 1,
              y: text.versteckt ? 20 : 0,
            }}
            transition={
              text.versteckt ? { duration: 0 } : { duration: 0.6, delay: 0.2 }
            }
            className="mt-6 text-xl text-[var(--color-text-secondary)] max-w-2xl mx-auto"
          >
            {landingText(
              "Der Bot ist nur der Schlüssel. Sobald du ihn aktivierst, bist du Partner im deutschen Deadlock-Netzwerk. Gehst du offline, reicht das Netzwerk deine Zuschauer an den nächsten deutschen Deadlock-Stream weiter.",
            )}
          </motion.p>
        </div>

        <div className="mt-16 mx-auto w-full max-w-[1400px]">
          <RaidDemo />
        </div>

        <motion.div
          ref={cta.ref}
          initial={false}
          animate={{
            opacity: cta.versteckt ? 0 : 1,
            y: cta.versteckt ? 20 : 0,
          }}
          transition={
            cta.versteckt ? { duration: 0 } : { duration: 0.6, delay: 0.5 }
          }
          className="mt-10 flex gap-4 justify-center flex-wrap"
        >
          <a
            href={buildTwitchBotAuthUrl()}
            className="gradient-accent rounded-xl px-7 py-3.5 font-semibold inline-flex items-center gap-2 transition-all duration-200 hover:brightness-110 hover:shadow-[0_0_24px_4px_rgba(201,168,106,0.3)]"
          >
            <ExternalLink size={18} />
            {landingText("Jetzt Partner werden")}
          </a>
          <a
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="rounded-xl px-7 py-3.5 font-semibold text-[var(--color-text-primary)] border border-[rgba(255,255,255,0.14)] inline-flex items-center gap-2 transition-all duration-200 hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
          >
            <DiscordLogo size={18} />
            {landingText("Community-Discord beitreten")}
          </a>
        </motion.div>
      </div>
    </section>
  );
}
