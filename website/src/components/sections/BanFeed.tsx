import { AnimatePresence } from "framer-motion";
import { Shield, BarChart3, Hash } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { GradientText } from "@/components/ui/GradientText";
import { BanFeedEntry } from "@/components/ui/BanFeedEntry";
import { useBanFeed } from "@/hooks/useBanFeed";

const statCards = [
  { key: "today", label: "Bans heute", icon: Shield },
  { key: "total_30d", label: "Letzte 30 Tage", icon: BarChart3 },
  { key: "channels_protected", label: "Geschützte Kanäle", icon: Hash },
] as const;

export function BanFeed() {
  const { bans, stats } = useBanFeed();

  const statValues: Record<string, number> = {
    today: stats.today,
    total_30d: stats.total_30d,
    channels_protected: stats.channels_protected,
  };

  return (
    <section id="moderation" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-start">

          {/* LEFT — Text + Stats */}
          <ScrollReveal>
            <p className="text-sm text-[var(--color-accent)] font-medium uppercase tracking-wider mb-4">
              Moderation
            </p>

            <h2 className="text-3xl md:text-4xl font-bold text-[var(--color-text-primary)] mb-6">
              <GradientText>Spam-Schutz</GradientText> in Echtzeit
            </h2>

            <p className="text-[var(--color-text-secondary)] text-lg mb-4 leading-relaxed">
              Unser Bot erkennt Spam-Bots automatisch und bannt sie in Echtzeit.
              Pattern-Erkennung, Account-Analyse und Community-Schutz — rund um
              die Uhr.
            </p>

            <p className="text-[var(--color-text-secondary)] text-base mb-8 leading-relaxed">
              Verdächtige Accounts werden anhand bekannter Spam-Muster, Account-Alter
              und Verhaltensanalyse identifiziert, bevor sie Schaden anrichten können.
            </p>

            {/* Stat cards */}
            <div className="space-y-4">
              {statCards.map((card) => {
                const Icon = card.icon;
                return (
                  <div
                    key={card.key}
                    className="panel-card rounded-xl p-4 flex items-center gap-4"
                  >
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
                    <div className="min-w-0 flex-1">
                      <p className="text-xs text-[var(--color-text-secondary)] uppercase tracking-wider">
                        {card.label}
                      </p>
                      <p className="text-2xl font-bold font-display text-[var(--color-text-primary)]">
                        {statValues[card.key].toLocaleString("de-DE")}
                      </p>
                    </div>
                  </div>
                );
              })}
            </div>
          </ScrollReveal>

          {/* RIGHT — Live Feed */}
          <ScrollReveal delay={0.2}>
            <div className="panel-card rounded-2xl p-6">
              {/* Header with live indicator */}
              <div className="flex items-center gap-2 mb-4">
                <span className="relative flex h-2.5 w-2.5">
                  <span className="animate-ping absolute h-full w-full rounded-full bg-green-400 opacity-75" />
                  <span className="relative rounded-full h-2.5 w-2.5 bg-green-500" />
                </span>
                <span className="text-sm font-semibold text-[var(--color-text-primary)]">
                  Live Ban-Feed
                </span>
              </div>

              {/* Feed container */}
              <div className="relative max-h-[400px] overflow-hidden">
                <div className="space-y-1">
                  <AnimatePresence initial={false}>
                    {bans.map((ban) => (
                      <BanFeedEntry
                        key={`${ban.target_login}-${ban.received_at}`}
                        ban={ban}
                      />
                    ))}
                  </AnimatePresence>
                </div>

                {/* Fade-out at bottom */}
                <div className="absolute bottom-0 left-0 right-0 h-16 bg-gradient-to-t from-[var(--color-card)] to-transparent pointer-events-none" />
              </div>
            </div>
          </ScrollReveal>

        </div>
      </div>
    </section>
  );
}
