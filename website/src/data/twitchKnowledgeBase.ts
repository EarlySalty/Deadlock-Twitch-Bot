import {
  DISCORD_INVITE_URL,
  TWITCH_DEMO_DASHBOARD_URL,
  TWITCH_FAQ_URL,
  buildTwitchBotAuthUrl,
  buildTwitchDashboardLoginUrl,
} from "@/data/externalLinks";

export interface OnboardingHighlight {
  label: string;
  value: string;
}

export type VisualType = "screenshot" | "animation" | "diagram";

export interface OnboardingStep {
  eyebrow: string;
  title: string;
  description: string;
  visualType: VisualType;
  visualSrc?: string;
  ctaLabel: string;
  ctaHref?: string;
}

export interface ChecklistItem {
  title: string;
  description: string;
  href?: string;
  label?: string;
}

// Visual onboarding steps - simplified 4-step structure
export const ONBOARDING_VISUAL_STEPS: OnboardingStep[] = [
  {
    eyebrow: "1. Verbinden",
    title: "Tritt dem Netzwerk bei",
    description: "Verbinde deinen Twitch-Kanal in Sekunden. Kein extra Konto - einfach mit Twitch einloggen.",
    visualType: "animation",
    ctaLabel: "Bot aktivieren",
    ctaHref: buildTwitchBotAuthUrl(),
  },
  {
    eyebrow: "2. Auto-Raid",
    title: "Automatische Raids bei Deadlock",
    description: "Wenn du offline gehst, leitet der Bot deine Viewer automatisch an aktive Partner weiter.",
    visualType: "diagram",
    ctaLabel: "Mehr erfahren",
    ctaHref: `${TWITCH_FAQ_URL}#raids`,
  },
  {
    eyebrow: "3. Dashboard",
    title: "Dein Stream-Dashboard",
    description: "Viewer-Trends, Raid-Verlauf und Netzwerk-Analytics an einem Ort.",
    visualType: "screenshot",
    visualSrc: "/images/onboarding/dashboard-preview.png",
    ctaLabel: "Demo ansehen",
    ctaHref: TWITCH_DEMO_DASHBOARD_URL,
  },
  {
    eyebrow: "4. Start",
    title: "Bereit zum Streamen",
    description: "Checkliste: Kanal verbunden, Auto-Raid aktiv, Dashboard offen.",
    visualType: "diagram",
    ctaLabel: "Zum Dashboard",
    ctaHref: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
  },
];

export const ONBOARDING_HIGHLIGHTS: OnboardingHighlight[] = [
  { label: "Partnernetzwerk", value: "30+ Deadlock-Streamer" },
  { label: "Auto-Raid", value: "Nur bei Deadlock aktiv" },
  { label: "Start", value: "Kanal verbinden und loslegen" },
];

export const START_CHECKLIST: ChecklistItem[] = [
  {
    title: "Kanal verbinden",
    description:
      "Aktiviere den Bot für deinen Kanal und werde Teil des Deadlock-Partnernetzwerks.",
    href: buildTwitchBotAuthUrl(),
    label: "Bot für deinen Kanal aktivieren",
  },
  {
    title: "Dashboard erkunden",
    description:
      "Schau dir an, was für deinen Kanal sichtbar ist und welche Funktionen du direkt nutzen kannst.",
    href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
    label: "Dashboard öffnen",
  },
  {
    title: "Discord beitreten",
    description:
      "Mehr Sichtbarkeit, automatische Go-Live-Posts und schneller Kontakt zur Community.",
    href: DISCORD_INVITE_URL,
    label: "Discord beitreten",
  },
];
