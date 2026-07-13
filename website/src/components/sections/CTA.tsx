import { ExternalLink } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { DiscordLogo } from "@/components/ui/DiscordLogo";
import {
  DISCORD_INVITE_URL,
  TWITCH_FAQ_URL,
  TWITCH_ONBOARDING_URL,
} from "@/data/externalLinks";

export function CTA() {
  return (
    <section id="cta" className="py-24 relative overflow-hidden">
      {/* Background gradient */}
      <div className="absolute inset-0 bg-gradient-to-br from-[var(--color-primary)]/5 via-transparent to-[var(--color-accent)]/5" />

      <div className="max-w-3xl mx-auto px-6 text-center relative z-10">
        <ScrollReveal>
          <h2 className="text-4xl md:text-5xl font-bold text-[var(--color-text-primary)] mb-6 font-display">
            Bereit für{" "}
            <span className="bg-gradient-to-r from-[var(--color-primary)] to-[var(--color-accent)] bg-clip-text text-transparent inline">
              intelligenteres
            </span>{" "}
            Streaming?
          </h2>

          <p className="text-xl text-[var(--color-text-secondary)] mb-10">
            Werde Teil der größten deutschen Deadlock-Community auf Twitch — kostenlos.
          </p>

          <div className="flex gap-4 justify-center flex-wrap">
            <a
              href={TWITCH_ONBOARDING_URL}
              className="gradient-accent rounded-xl px-8 py-4 font-semibold text-lg inline-flex items-center gap-2"
            >
              <ExternalLink size={20} />
              Zum Onboarding
            </a>
            <a
              href={DISCORD_INVITE_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="rounded-xl px-8 py-4 font-semibold text-white text-lg inline-flex items-center gap-2 transition-opacity duration-200 hover:opacity-90"
              style={{ background: "#5865F2" }}
            >
              <DiscordLogo size={22} className="text-white" />
              Discord beitreten
            </a>
            <a
              href={TWITCH_FAQ_URL}
              className="rounded-xl border border-[var(--color-border)] px-8 py-4 font-semibold text-[var(--color-text-primary)] text-lg inline-flex items-center gap-2 transition-colors duration-200 hover:border-[var(--color-border-hover)] hover:bg-white/5"
            >
              Alle Bot-Funktionen
            </a>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}
