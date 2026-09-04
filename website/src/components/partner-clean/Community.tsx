import { Trophy, Link, Award, Bell, ArrowRight } from "lucide-react";
import type { ReactNode } from "react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { DiscordLogo } from "@/components/ui/DiscordLogo";
import { DISCORD_INVITE_URL } from "@/data/externalLinks";

interface CommunityCardProps {
  icon: ReactNode;
  title: string;
  description: string;
  delay?: number;
}

function CommunityCard({ icon, title, description, delay = 0 }: CommunityCardProps) {
  return (
    <ScrollReveal delay={delay}>
      <div className="panel-card rounded-xl p-6 flex items-start gap-4 h-full">
        <div className="w-10 h-10 rounded-lg icon-tile flex items-center justify-center shrink-0">
          {icon}
        </div>
        <div>
          <h3 className="text-lg font-semibold text-[var(--color-text-primary)] mb-1">
            {title}
          </h3>
          <p className="text-sm text-[var(--color-text-secondary)]">
            {description}
          </p>
        </div>
      </div>
    </ScrollReveal>
  );
}

export function Community() {
  return (
    <section id="community" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge="Community"
          title="Deine Community, organisiert"
          subtitle="Mach aus neuen Viewern eine aktive Stamm-Community, mit automatischen Belohnungen, Rollen und Live-Signalen."
        />

        <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mt-16">
          <CommunityCard
            icon={<Trophy size={20} />}
            title="Leaderboard"
            description="Automatisches Ranking basierend auf Watch-Time, Aktivität und Treue"
            delay={0}
          />
          <CommunityCard
            icon={<Link size={20} />}
            title="Discord-Integration"
            description="Nahtlose Verbindung zwischen Twitch-Chat und Discord-Server"
            delay={0.1}
          />
          <CommunityCard
            icon={<Award size={20} />}
            title="Rollen-System"
            description="Automatische Rollenvergabe basierend auf Abonnement und Aktivität"
            delay={0.2}
          />
          <CommunityCard
            icon={<Bell size={20} />}
            title="Live-Benachrichtigungen"
            description="Automatische Benachrichtigungen in Discord, wenn du live gehst"
            delay={0.3}
          />
        </div>

        <ScrollReveal delay={0.1}>
          <div className="panel-card mt-10 rounded-2xl p-8 md:p-10 flex flex-col md:flex-row items-center gap-8 justify-between relative overflow-hidden">
            <div className="flex items-center gap-5 text-center md:text-left flex-col md:flex-row">
              <div
                className="w-16 h-16 rounded-2xl flex items-center justify-center shrink-0"
                style={{ background: "#5865F2" }}
              >
                <DiscordLogo size={34} className="text-white" />
              </div>
              <div>
                <h3 className="text-2xl font-bold text-[var(--color-text-primary)] font-display">
                  Komm auf unseren Discord
                </h3>
                <p className="text-[var(--color-text-secondary)] mt-1 max-w-md">
                  Hier läuft die Community zusammen. Mitspieler finden, Hilfe bekommen,
                  Updates verfolgen und direkt mit uns reden.
                </p>
              </div>
            </div>

            <a
              href={DISCORD_INVITE_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="shrink-0 inline-flex items-center gap-2 whitespace-nowrap rounded-xl px-7 py-3.5 font-semibold text-white transition-opacity duration-200 hover:opacity-90"
              style={{ background: "#5865F2" }}
            >
              <DiscordLogo size={20} />
              Discord beitreten
              <ArrowRight size={18} />
            </a>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}
