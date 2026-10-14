import { landingText } from "@/lib/landingLocale";
export interface AffiliateFeature {
  icon: string;
  title: string;
  description: string;
}

export const affiliateFeatures: AffiliateFeature[] = [
  {
    icon: "Percent",
    title: landingText("30% Provision"),
    description: landingText(
      "Auf jede Zahlung deiner geworbenen Streamer — dauerhaft, ohne Obergrenze. Solange der Streamer zahlt, verdienst du mit.",
    ),
  },
  {
    icon: "UserPlus",
    title: landingText("Streamer beanspruchen"),
    description: landingText(
      "Finde Deadlock-Streamer und beanspruche sie im Portal, bevor jemand anders es tut. First come, first served.",
    ),
  },
  {
    icon: "CreditCard",
    title: "Stripe Connect",
    description: landingText(
      "Verbinde dein Stripe-Konto einmalig und bekomme jede Provision automatisch auf dein Bankkonto — kein manuelles Anfordern.",
    ),
  },
  {
    icon: "Infinity",
    title: landingText("Kein Ablaufdatum"),
    description: landingText(
      "Einmal geworben, dauerhaft provisionsberechtigt. Keine zeitliche Begrenzung, kein Verfall.",
    ),
  },
  {
    icon: "LayoutDashboard",
    title: landingText("Echtzeit-Dashboard"),
    description: landingText(
      "Sieh in Echtzeit was du verdienst: Beanspruchte Streamer, Provisionen, Auszahlungen und History — alles im Portal.",
    ),
  },
  {
    icon: "LogIn",
    title: landingText("Einfache Registrierung"),
    description: landingText(
      "Melde dich mit deinem Twitch-Account an, verbinde Stripe — in unter 5 Minuten startklar. Keine Adressdaten nötig.",
    ),
  },
];
