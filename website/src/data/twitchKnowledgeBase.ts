import {
  DISCORD_INVITE_URL,
  TWITCH_DEMO_DASHBOARD_URL,
  TWITCH_FAQ_URL,
  buildTwitchBotAuthUrl,
  buildTwitchDashboardLoginUrl,
} from "@/data/externalLinks";
import { tx } from "@/i18n/locale";

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
export const ONBOARDING_VISUAL_STEPS: OnboardingStep[] = tx<OnboardingStep[]>([
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
], [
  {
    eyebrow: "1. Connect",
    title: "Join the network",
    description: "Connect your Twitch channel in seconds. No extra account - just log in with Twitch.",
    visualType: "animation",
    ctaLabel: "Activate the bot",
    ctaHref: buildTwitchBotAuthUrl(),
  },
  {
    eyebrow: "2. Auto-raid",
    title: "Automatic raids for Deadlock",
    description: "When you go offline, the bot automatically sends your viewers to active partners.",
    visualType: "diagram",
    ctaLabel: "Learn more",
    ctaHref: `${TWITCH_FAQ_URL}#raids`,
  },
  {
    eyebrow: "3. Dashboard",
    title: "Your stream dashboard",
    description: "Viewer trends, raid history and network analytics in one place.",
    visualType: "screenshot",
    visualSrc: "/images/onboarding/dashboard-preview.png",
    ctaLabel: "View demo",
    ctaHref: TWITCH_DEMO_DASHBOARD_URL,
  },
  {
    eyebrow: "4. Start",
    title: "Ready to stream",
    description: "Checklist: channel connected, auto-raid active, dashboard open.",
    visualType: "diagram",
    ctaLabel: "Go to dashboard",
    ctaHref: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
  },
]);

export const ONBOARDING_HIGHLIGHTS: OnboardingHighlight[] = tx<OnboardingHighlight[]>([
  { label: "Partnernetzwerk", value: "30+ Deadlock-Streamer" },
  { label: "Auto-Raid", value: "Nur bei Deadlock aktiv" },
  { label: "Start", value: "Kanal verbinden und loslegen" },
], [
  { label: "Partner network", value: "30+ Deadlock streamers" },
  { label: "Auto-raid", value: "Only active for Deadlock" },
  { label: "Start", value: "Connect your channel and go" },
]);

export const START_CHECKLIST: ChecklistItem[] = tx<ChecklistItem[]>([
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
], [
  {
    title: "Connect your channel",
    description:
      "Activate the bot for your channel and join the Deadlock Partner Network.",
    href: buildTwitchBotAuthUrl(),
    label: "Activate the bot for your channel",
  },
  {
    title: "Explore the dashboard",
    description:
      "See what's visible for your channel and which features you can use right away.",
    href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
    label: "Open dashboard",
  },
  {
    title: "Join our Discord",
    description:
      "More visibility, automatic go-live posts and quick contact with the community.",
    href: DISCORD_INVITE_URL,
    label: "Join our Discord",
  },
]);
