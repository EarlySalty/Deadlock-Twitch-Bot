export interface Feature {
  id: string;
  icon: string;
  title: string;
  description: string;
}

export const features: Feature[] = [
  {
    id: "auto-raid",
    icon: "Swords",
    title: "Auto-Raid",
    description:
      "Endet dein Stream, raidet der Bot automatisch den passendsten Live-Partner, fair verteilt und mit Vorrang für neue Partner. Und wenn andere offline gehen, landen ihre Raids genauso bei dir. Manuelle Raids bleiben jederzeit möglich.",
  },
  {
    id: "discord-live",
    icon: "Zap",
    title: "Live-Post im Discord",
    description:
      "Du gehst live, der Bot postet dich im Community-Discord, automatisch erkannt und auf Wunsch mit Ping-Rolle. Sichtbarkeit ab der ersten Minute, ohne dass du selbst irgendwo posten musst.",
  },
  {
    id: "analytics",
    icon: "BarChart2",
    title: "Analytics",
    description:
      "Echtzeit-Dashboard mit 13 Tabs: Zuschauer, Chat, Wachstum, Raids, persönliche Bestwerte und der faire Vergleich mit dem Netzwerk. Nach jedem Stream siehst du, was funktioniert hat, statt nur ein Bauchgefühl zu haben.",
  },
  {
    id: "clip-manager",
    icon: "Clapperboard",
    title: "Clip Manager (Coming Soon)",
    description:
      "Clips direkt aus dem Chat erstellen, die KI schlägt Titel vor. Als Nächstes kommt der Multi-Plattform-Upload zu YouTube, TikTok und Instagram, damit deine besten Momente sich von selbst verbreiten.",
  },
  {
    id: "community",
    icon: "Users",
    title: "Community",
    description:
      "Treue Zuschauer werden automatisch belohnt, Lurker gezielt aktiviert. Dazu ein eigener Streamer-Bereich im Community-Discord: Austausch mit anderen Creatorn und echte Zuschauer statt Algorithmus.",
  },
  {
    id: "moderation",
    icon: "ShieldCheck",
    title: "Moderation",
    description:
      "Ein KI-Wächter erkennt Scam- und Spam-Konten und bannt sie, bevor dein Chat sie überhaupt sieht. Dazu die globale Ban-Liste des Netzwerks und Timeouts für Fremdwerbung, beides pro Kanal abschaltbar.",
  },
];
