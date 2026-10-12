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
import { tx } from "@/i18n/locale";

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
    title: tx("Auto-Raid im Netzwerk", "Network auto-raid"),
    description: tx(
      "Endet dein Stream, landen deine Zuschauer beim passenden Live-Partner. Gehen andere offline, landen ihre bei dir.",
      "When your stream ends, your viewers land with a fitting live partner. When others go offline, theirs land with you.",
    ),
  },
  {
    id: "discord-live",
    icon: Zap,
    title: tx("Sichtbar ab der ersten Minute", "Visible from minute one"),
    description: tx(
      "Du gehst live, der Discord weiß es sofort. Verschluckt Twitch das Ereignis, fragt der Bot alle 15 Sekunden selbst nach.",
      "You go live, the Discord knows instantly. If Twitch drops the event, the bot checks for itself every 15 seconds.",
    ),
  },
  {
    id: "analytics",
    icon: BarChart2,
    title: tx("Deine Zahlen im Blick", "Your numbers at a glance"),
    description: tx(
      "Zuschauer, Chat, Raids, Bestwerte und der Vergleich mit dem Netzwerk. Nach jedem Stream siehst du, was funktioniert hat.",
      "Viewers, chat, raids, personal bests and how you compare to the network. After every stream you see what worked.",
    ),
  },
  {
    id: "clip-manager",
    icon: Clapperboard,
    title: tx("Clips direkt aus dem Chat", "Clips straight from chat"),
    description: tx("!clip im Chat, fertig ist der Clip.", "Type !clip in chat and the clip is done."),
  },
  {
    id: "community",
    icon: Users,
    title: tx("Eine Community im Rücken", "A community behind you"),
    description: tx(
      "Treue Zuschauer werden belohnt, Lurker gezielt angesprochen. Im Streamer-Bereich im Discord tauschst du dich mit den anderen aus.",
      "Loyal viewers get rewarded, lurkers get a targeted nudge. In the streamer area of the Discord you swap notes with the others.",
    ),
  },
  {
    id: "moderation",
    icon: ShieldCheck,
    title: tx("Ein Schutzschild für alle", "One shield for everyone"),
    description: tx(
      "Scam- und Spam-Konten fliegen aus deinem Chat, bevor jemand sie sieht. Dazu die Ban-Liste des ganzen Netzwerks, alles pro Kanal abschaltbar.",
      "Scam and spam accounts get kicked from your chat before anyone sees them. Plus the ban list of the whole network, all switchable per channel.",
    ),
  },
];

export function Features() {
  return (
    <section id="features" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge={tx("Partner-Vorteile", "Partner perks")}
          title={tx("Was als Partner dazugehört", "What you get as a partner")}
          subtitle={tx("Sechs Dinge, die dazukommen, sobald du Partner bist.", "Six things you get the moment you're a partner.")}
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
