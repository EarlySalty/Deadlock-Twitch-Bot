export const HERO_COPY = {
  chip: "Netzwerk deutscher Deadlock-Streamer",
  headlineLead: "Kein Stream endet im",
  headlineAccent: "Leeren.",
  subline:
    "Der Bot ist nur der Schlüssel. Sobald du ihn aktivierst, bist du Partner im deutschen Deadlock-Netzwerk. Gehst du offline, übergibt das Netzwerk deine Zuschauer an den nächsten deutschen Deadlock-Stream, statt sie verschwinden zu lassen.",
  ctaPrimary: "Jetzt Partner werden",
  ctaSecondary: "Community-Discord beitreten",
  proofPartners: "Partner im Netzwerk",
  proofLiveKnown: "gerade live in Deadlock",
  proofLive: "gerade live",
  proofBans: "Spam-Accounts entfernt, 30 Tage",
  proofNote:
    "Die Kennzahlen kommen live aus dem laufenden Betrieb, die Bühne darüber ist ein Beispielablauf.",
} as const;

export const VALUES_COPY = {
  stamp: "Dabei sein heißt",
  headline: "Was als Partner dazugehört",
  intro:
    "Sobald du dabei bist, läuft im Hintergrund mehr als die Übergabe am Stream-Ende: dein Go-Live taucht im Community-Discord auf, dein Chat teilt sich den Schutz mit allen Partnern, und nach dem Stream stehen deine Zahlen bereit.",
} as const;

export interface NetworkValue {
  id: "raids" | "schutz" | "coaching" | "clips";
  kicker: string;
  title: string;
  body: string;
  tone: "primary" | "accent";
}

export const networkValues: NetworkValue[] = [
  {
    id: "raids",
    kicker: "Am Stream-Ende",
    title: "Deine Leute bleiben in der Szene",
    body: "Endest du, führt das Netzwerk deine Zuschauer zum nächsten deutschen Deadlock-Stream, der gerade läuft. Sie versickern nicht, sie landen bei einem Partner.",
    tone: "primary",
  },
  {
    id: "schutz",
    kicker: "Rund um die Uhr",
    title: "Ein Chat-Schutz für alle",
    body: "Follow-Bots, Viewbot-Werbung und Selbstpromo laufen bei jedem Partner gegen dieselbe Liste, bevor sie deinen Chat erreichen.",
    tone: "accent",
  },
  {
    id: "coaching",
    kicker: "Nach dem Stream",
    title: "Deine Zahlen ohne Tabellenarbeit",
    body: "Wann Zuschauer gekommen und gegangen sind, wie der Chat lief und welche Momente getragen haben, steht danach im Dashboard bereit.",
    tone: "primary",
  },
  {
    id: "clips",
    kicker: "Zwischen den Streams",
    title: "Momente werden zu Clips",
    body: "Wer !clip in den Chat schreibt, hält die Szene fest, und die starken Ausschnitte liegen fertig zum Weiterposten bereit.",
    tone: "accent",
  },
];

export const SPAM_COPY = {
  stamp: "Schutz im Netzwerk",
  headline: "Der Spam-Schutz läuft für das ganze Netzwerk",
  intro:
    "Was ein Partner an Werbe- und Bot-Nachrichten meldet, greift bei allen. Der Feed unten kommt aus den Partner-Chats, während du hier liest.",
  feedTitle: "Live aus den Partner-Chats",
  statToday: "heute geräumt",
  stat30d: "in den letzten 30 Tagen",
  statChannels: "geschützte Chats",
  empty: "Der Feed ist gerade nicht abrufbar.",
} as const;

export interface PlanStep {
  index: string;
  title: string;
  body: string;
  duration: string;
}

/** Miller-Plan: drei Schritte, die der Streamer selbst geht. */
export const planSteps: PlanStep[] = [
  {
    index: "01",
    title: "Twitch verbinden",
    body: "Ein Klick auf Twitch, Freigabe erteilen, fertig.",
    duration: "ca. 2 Minuten",
  },
  {
    index: "02",
    title: "Netzwerk aktivieren",
    body: "Du wählst, ob dein Stream am Ende automatisch weitergibt und wen du empfangen willst.",
    duration: "einmalig",
  },
  {
    index: "03",
    title: "Wachsen lassen",
    body: "Ab jetzt läuft es im Hintergrund: Übergabe an Partner, Schutz im Chat, Auswertung nach dem Stream.",
    duration: "läuft von allein",
  },
];

export interface ValuePillar {
  id: string;
  icon: "Swords" | "ShieldCheck" | "Sparkles" | "Clapperboard";
  kicker: string;
  title: string;
  body: string;
  tone: "primary" | "accent";
}

/** Reihenfolge ist Absicht: Raids zuerst, Moderation ist nicht mehr die Hauptrolle. */
export const valuePillars: ValuePillar[] = [
  {
    id: "raids",
    icon: "Swords",
    kicker: "Der Kern",
    title: "Auto-Raid-Netzwerk",
    body: "Wenn dein Stream endet, geht deine Community nicht offline, sondern zum nächsten deutschen Deadlock-Streamer, der gerade live ist.",
    tone: "primary",
  },
  {
    id: "schutz",
    icon: "ShieldCheck",
    kicker: "Die Grundlage",
    title: "Schutz im Chat",
    body: "Follow-Bots, Viewbot-Werbung und Selbstpromo laufen bei uns gegen eine gemeinsame Liste über alle Partner-Kanäle.",
    tone: "accent",
  },
  {
    id: "coaching",
    icon: "Sparkles",
    kicker: "Nach dem Stream",
    title: "Auswertung und Coaching",
    body: "Das Dashboard zeigt, wann Zuschauer gekommen und gegangen sind, wie der Chat lief und was deine besten Momente waren.",
    tone: "primary",
  },
  {
    id: "clips",
    icon: "Clapperboard",
    kicker: "Zwischen den Streams",
    title: "Clips per Befehl im Chat",
    body: "Wer !clip in den Chat schreibt, hält den Moment als Twitch-Clip fest.",
    tone: "accent",
  },
];

export interface Plan {
  id: string;
  name: string;
  price: string;
  period: string;
  yearly: string | null;
  anchor: string;
  featured: boolean;
  cta: string;
  ctaHref: string;
  features: { label: string; included: boolean }[];
  note?: string;
}

/**
 * Drei Stufen nach docs/strategie/32. Free ist der heutige Ist-Zustand;
 * Plus und Creator Pro bündeln bestehende Einzelpläne neu, deshalb steht bei
 * beiden ausdrücklich, dass sie mit dem Netzwerk-Update starten.
 */
export const plans: Plan[] = [
  {
    id: "free",
    name: "Netzwerk Free",
    price: "0 €",
    period: "dauerhaft",
    yearly: null,
    anchor: "Vollwertig, ohne Ablaufdatum",
    featured: false,
    cta: "Kostenlos verbinden",
    ctaHref: "AUTH",
    features: [
      { label: "Auto-Raid-Netzwerk in beide Richtungen", included: true },
      { label: "Kompletter Chat-Schutz", included: true },
      { label: "Go-Live-Post im Community-Discord", included: true },
      { label: "Dashboard mit Grundauswertung", included: true },
    ],
  },
  {
    id: "plus",
    name: "Netzwerk Plus",
    price: "4,99 €",
    period: "pro Monat",
    yearly: "49,90 € im Jahr, zwei Monate geschenkt",
    anchor: "Der Preis eines Twitch-Subs",
    featured: true,
    cta: "Plus ansehen",
    ctaHref: "ABBO",
    features: [
      { label: "Alles aus Free", included: true },
      { label: "Bevorzugte Platzierung im Raid-Netzwerk", included: true },
      { label: "Werbefreier Chat", included: true },
      { label: "Volle Auswertung und KI-Wochenreport", included: true },
    ],
    note: "Startet mit dem Netzwerk-Update. Bestehende Abos werden übernommen.",
  },
  {
    id: "pro",
    name: "Creator Pro",
    price: "9,99 €",
    period: "pro Monat",
    yearly: "99,90 € im Jahr, zwei Monate geschenkt",
    anchor: "Fuer alles, was nach dem Netzwerk-Update dazukommt",
    featured: false,
    cta: "Pro ansehen",
    ctaHref: "ABBO",
    features: [
      { label: "Alles aus Plus", included: true },
      { label: "Vorrang bei Support und neuen Features", included: true },
    ],
    note: "Noch nicht buchbar. Startet, sobald die ersten eigenen Pro-Funktionen stehen.",
  },
];

export interface Objection {
  question: string;
  label: string;
  answer: string;
  proofLabel?: string;
  proofHref?: string;
}

/**
 * Einwand-Bibliothek nach docs/strategie/16 und 31: erst das Label, das den
 * Einwand als berechtigt anerkennt, dann der überprüfbare Beleg.
 */
export const objections: Objection[] = [
  {
    question: "Klingt nach Scam.",
    label: "Verständlich. Ein fremder Bot, der Rechte auf deinem Kanal will, verdient Misstrauen.",
    answer:
      "Deshalb liegt offen, was wir anfragen und warum: jede Berechtigung einzeln erklärt, der Raid-Zugriff erst dann, wenn du das Feature einschaltest, und Entfernen geht in einem Klick. Wer dahintersteht, steht im Impressum.",
    proofLabel: "Berechtigungen im Detail",
    proofHref: "SECURITY",
  },
  {
    question: "Ich habe schon Nightbot und StreamElements.",
    label: "Dann behalte sie.",
    answer:
      "Wir ersetzen keinen Chat-Bot. Wir sind das Netzwerk dahinter: die Übergabe deiner Zuschauer am Stream-Ende und die gemeinsame Spam-Liste. Beides gibt es bei den großen Bots nicht, weil beides nur mit anderen Deadlock-Kanälen zusammen funktioniert.",
  },
  {
    question: "Ich habe zu wenige Zuschauer für so etwas.",
    label: "Genau dafür ist es gebaut.",
    answer:
      "Das Netzwerk ist auf Kanäle mit 0 bis 100 gleichzeitigen Zuschauern ausgelegt. Wer zwei Zuschauer weitergibt, bekommt auch dann etwas zurück, wenn ein größerer Partner aufhört zu streamen.",
  },
  {
    question: "Was passiert mit meinen Daten?",
    label: "Berechtigte Frage, gerade bei einem Community-Projekt.",
    answer:
      "Gespeichert werden Kanaldaten, Chat-Ereignisse und Auswertungen, die für die Features nötig sind. Keine Weitergabe an Dritte, Löschung auf Anfrage, Server in Deutschland.",
    proofLabel: "Datenschutz",
    proofHref: "PRIVACY",
  },
  {
    question: "Und wenn ich es wieder loswerden will?",
    label: "Dann bist du in einer Minute raus.",
    answer:
      "Im Dashboard trennst du die Verbindung, der Bot verlässt deinen Chat und die Weitergabe endet sofort. Es gibt keine Mindestlaufzeit und keine Rückfrage-Schleife.",
  },
];
