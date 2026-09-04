import type { ComponentType } from "react";
import {
  Swords,
  Zap,
  BarChart2,
  Clapperboard,
  Users,
  ShieldCheck,
} from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";

interface PartnerBenefit {
  id: string;
  icon: ComponentType<{ size?: number; className?: string }>;
  title: string;
  description: string;
}

const benefits: PartnerBenefit[] = [
  {
    id: "auto-raid",
    icon: Swords,
    title: "Auto-Raid im Netzwerk",
    description:
      "Endet dein Stream, landen deine Zuschauer beim passenden Live-Partner. Gehen andere offline, landen ihre bei dir.",
  },
  {
    id: "discord-live",
    icon: Zap,
    title: "Sichtbar ab der ersten Minute",
    description:
      "Du gehst live, der Discord weiß es sofort. Verschluckt Twitch das Ereignis, fragt der Bot alle 15 Sekunden selbst nach.",
  },
  {
    id: "analytics",
    icon: BarChart2,
    title: "Deine Zahlen im Blick",
    description:
      "Zuschauer, Chat, Raids, Bestwerte und der Vergleich mit dem Netzwerk. Nach jedem Stream siehst du, was funktioniert hat.",
  },
  {
    id: "clip-manager",
    icon: Clapperboard,
    title: "Clips direkt aus dem Chat",
    description: "!clip im Chat, fertig ist der Clip.",
  },
  {
    id: "community",
    icon: Users,
    title: "Eine Community im Rücken",
    description:
      "Treue Zuschauer werden belohnt, Lurker gezielt angesprochen. Im Streamer-Bereich im Discord tauschst du dich mit den anderen aus.",
  },
  {
    id: "moderation",
    icon: ShieldCheck,
    title: "Ein Schutzschild für alle",
    description:
      "Scam- und Spam-Konten fliegen aus deinem Chat, bevor jemand sie sieht. Dazu die Ban-Liste des ganzen Netzwerks, alles pro Kanal abschaltbar.",
  },
];

export function Features() {
  return (
    <section id="features" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge="Partner-Vorteile"
          title="Was als Partner dazugehört"
          subtitle="Sechs Dinge, die dazukommen, sobald du Partner bist."
        />

        <div className="mt-16 grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {benefits.map((benefit, index) => {
            const Icon = benefit.icon;
            return (
              <ScrollReveal key={benefit.id} delay={index * 0.08}>
                <div className="group h-full rounded-2xl border border-[var(--color-border)] bg-[var(--color-card)] p-7 soft-elevate">
                  <div
                    className="flex h-14 w-14 items-center justify-center rounded-2xl transition-transform duration-200 group-hover:-translate-y-0.5 group-hover:scale-105"
                    style={{
                      background:
                        "linear-gradient(135deg, #f6ddb0 0%, #efd49d 35%, #c8a86b 68%, #a98746 100%)",
                      boxShadow: "0 0 28px -6px rgba(201,168,106,0.65)",
                    }}
                  >
                    <Icon size={26} className="text-[#16100a]" />
                  </div>
                  <h3 className="mt-5 text-lg font-semibold text-[var(--color-text-primary)]">
                    {benefit.title}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-[var(--color-text-secondary)]">
                    {benefit.description}
                  </p>
                </div>
              </ScrollReveal>
            );
          })}
        </div>
      </div>
    </section>
  );
}
