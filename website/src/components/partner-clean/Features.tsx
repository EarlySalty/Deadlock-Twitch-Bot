import { FeatureCard } from "@/components/ui/FeatureCard";
import { SectionHeading } from "@/components/ui/SectionHeading";

interface PartnerBenefit {
  id: string;
  icon: string;
  title: string;
  description: string;
}

const benefits: PartnerBenefit[] = [
  {
    id: "auto-raid",
    icon: "Swords",
    title: "Auto-Raid im Netzwerk",
    description:
      "Endet dein Stream, wandern deine Viewer zum passendsten Live-Partner, fair verteilt und mit Vorrang für neue Partner. Gehen andere offline, landen ihre Raids genauso bei dir. Manuelle Raids bleiben jederzeit möglich.",
  },
  {
    id: "discord-live",
    icon: "Zap",
    title: "Sichtbar ab der ersten Minute",
    description:
      "Du gehst live und stehst automatisch im Community-Discord, auf Wunsch mit Ping-Rolle. Sichtbarkeit ab Sekunde eins, ohne dass du selbst irgendwo posten musst.",
  },
  {
    id: "analytics",
    icon: "BarChart2",
    title: "Deine Zahlen, im Vergleich",
    description:
      "Echtzeit-Dashboard mit 13 Tabs: Zuschauer, Chat, Wachstum, Raids, persönliche Bestwerte und der faire Vergleich mit dem Netzwerk. Nach jedem Stream siehst du, was funktioniert hat.",
  },
  {
    id: "clip-manager",
    icon: "Clapperboard",
    title: "Clips, die sich verbreiten (bald)",
    description:
      "Clips direkt aus dem Chat, mit KI-Titelvorschlag. Als Nächstes kommt der Upload zu YouTube, TikTok und Instagram, damit deine besten Momente von selbst weiterlaufen.",
  },
  {
    id: "community",
    icon: "Users",
    title: "Eine Community im Rücken",
    description:
      "Treue Zuschauer werden automatisch belohnt, Lurker gezielt aktiviert. Dazu ein eigener Streamer-Bereich im Discord: Austausch mit anderen Creatorn und echte Zuschauer statt Algorithmus.",
  },
  {
    id: "moderation",
    icon: "ShieldCheck",
    title: "Ein Schutzschild für alle",
    description:
      "Ein KI-Wächter hält Scam- und Spam-Konten aus deinem Chat, bevor jemand sie sieht. Dazu die globale Ban-Liste des Netzwerks und Timeouts für Fremdwerbung, beides pro Kanal abschaltbar.",
  },
  {
    id: "monitoring",
    icon: "Activity",
    title: "Rund um die Uhr im Blick",
    description:
      "Dein Kanal wird durchgehend beobachtet: Twitch meldet den Sendestart, zusätzlich wird alle 15 Sekunden nachgefragt. So steht dein Live-Post auch dann im Discord, wenn Twitch das Ereignis verschluckt, und deine Stream-Zeiten landen vollständig in der Auswertung.",
  },
];

export function Features() {
  return (
    <section id="features" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge="Partner-Vorteile"
          title="Was als Partner dazugehört"
          subtitle="Sieben Dinge, die dazukommen, sobald dein Kanal Teil vom Netzwerk ist."
        />

        <div className="mt-16 grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {benefits.map((benefit, index) => (
            <FeatureCard
              key={benefit.id}
              icon={benefit.icon}
              title={benefit.title}
              description={benefit.description}
              delay={index * 0.1}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
