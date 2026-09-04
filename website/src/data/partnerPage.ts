export const PARTNER_SEO = {
  title: "Deadlock Partner Netzwerk - Auto-Raid & Streamer Community (Deutsch)",
  description:
    "Werde Partner der deutschen Deadlock Community. Automatische Raids, Discord-Sichtbarkeit und ein Netzwerk, in dem Viewer nicht verloren gehen.",
  keywords:
    "Deutsche Deadlock Community, Deadlock Partner, Deadlock Streamer Netzwerk, Deadlock Discord Deutsch",
} as const;

export const PARTNER_COPY = {
  brand: "Deutsche Deadlock Community",
  badge: "Das Partner-Netzwerk der deutschen Deadlock-Community",
  headline: "Werde Partner der deutschen Deadlock-Community.",
  subline:
    "Der Bot ist nur der Schlüssel. Sobald du ihn aktivierst, bist du Partner, und deine Viewer bleiben im Kreislauf, statt zu verschwinden.",
  ctaPrimary: "Jetzt Partner werden",
  ctaSecondary: "Community-Discord beitreten",
  stageLive: "Gerade im Netzwerk",
  stageRaid: "Wenn einer endet, übernimmt der nächste Partner.",
  proofPartner: "Partner",
  proofLive: "gerade live",
  problemHeadline: "Allein oder im Netzwerk.",
  aloneTitle: "Allein",
  aloneBody: "Dein Stream endet. Die Viewer sind weg.",
  networkTitle: "Im Netzwerk",
  networkBody:
    "Dein Stream endet. Der nächste Partner übernimmt. Die Viewer kommen wieder.",
  meaningHeadline: "Was Partner sein bedeutet",
  meaning: [
    "Deine Viewer bleiben im Deadlock-Netzwerk",
    "Andere Partner raiden zu dir",
    "Dein Stream wird in der Community sichtbar",
    "Chat-Schutz und Zahlen laufen im Hintergrund",
  ],
  rosterHeadline: "Hinter deinem Kanal stehen jetzt andere Kanäle.",
  safetyHeadline: "Nur Mod-Rechte. Jederzeit kündbar.",
  safetyBody:
    "Der Bot braucht nur Mod-Rechte. Streamtitel und Kanal-Einstellungen kann er nicht anfassen. Du kannst den Zugang jederzeit in den Twitch-Einstellungen kündigen. Tokens liegen verschlüsselt.",
  safetyLink: "Sicherheitskonzept lesen",
  closeHeadline:
    "Dein nächster Stream endet sowieso. Die Frage ist, ob du allein endest oder als Partner.",
  closeNote: "Partner werden ist kostenlos. Alles Weitere ist optional.",
  closeSafetyLink: "So gehen wir mit deinem Konto um",
} as const;

export const PARTNER_SECTIONS = [
  "hero",
  "partner",
  "leere",
  "netzwerk",
  "spamschutz",
  "sicherheit",
  "abschluss",
] as const;

export const PARTNER_NAV = [
  { id: "partner", label: "Partner" },
  { id: "netzwerk", label: "Netzwerk" },
  { id: "spamschutz", label: "Schutz" },
  { id: "sicherheit", label: "Sicherheit" },
] as const;

export const PARTNER_FORBIDDEN = [
  "kostenlos verbinden",
  "leistungen",
  "wachstums-netzwerk",
  "kanal-report",
  "was du bekommst",
  "drei schritte",
] as const;

const BASE = import.meta.env.BASE_URL.replace(/\/$/, "");

export const NETWORK_CLIPS = [
  { login: "miracleghost9", displayName: "miracleghost9" },
  { login: "whysolowkey", displayName: "whysolowkey" },
  { login: "kdenos", displayName: "kdenos" },
  { login: "johnnyblazedx", displayName: "johnnyblazedx" },
  { login: "coolysdl", displayName: "coolysdl" },
  { login: "duzzel", displayName: "duzzel" },
] as const;

export function clipSrc(login: string): string {
  return `${BASE}/clips/${login}.mp4`;
}

export function clipPfp(login: string): string {
  return `${BASE}/clips/pfp/${login}.png`;
}

export function twitchUrl(login: string): string {
  return `https://twitch.tv/${login}`;
}
