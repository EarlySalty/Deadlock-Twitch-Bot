import { ExternalLink } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { DiscordLogo } from "@/components/ui/DiscordLogo";
import { buildTwitchBotAuthUrl, DISCORD_INVITE_URL } from "@/data/externalLinks";

export function CTA() {
  return (
    <section id="cta" className="py-24 relative overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-br from-[#c8a86b0d] via-transparent to-[#55978f0d]" />

      <div className="max-w-3xl mx-auto px-6 text-center relative z-10">
        <ScrollReveal>
          <h2 className="text-4xl md:text-5xl font-bold text-[var(--color-text-primary)] mb-6 font-display">
            Dein nächster Stream endet sowieso. Die Frage ist, ob du{" "}
            <span className="bg-gradient-to-r from-[var(--color-primary)] to-[var(--color-accent)] bg-clip-text text-transparent inline">
              allein
            </span>{" "}
            endest oder als Partner.
          </h2>

          <p className="text-xl text-[var(--color-text-secondary)] mb-10">
            Werde Teil vom größten deutschen Deadlock-Netzwerk auf Twitch.
          </p>

          <div className="flex gap-4 justify-center flex-wrap">
            <a
              href={buildTwitchBotAuthUrl()}
              className="gradient-accent rounded-xl px-8 py-4 font-semibold text-lg inline-flex items-center gap-2"
            >
              <ExternalLink size={20} />
              Jetzt Partner werden
            </a>
            <a
              href={DISCORD_INVITE_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="rounded-xl px-8 py-4 font-semibold text-white text-lg inline-flex items-center gap-2 transition-opacity duration-200 hover:opacity-90"
              style={{ background: "#5865F2" }}
            >
              <DiscordLogo size={22} className="text-white" />
              Community-Discord beitreten
            </a>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}
