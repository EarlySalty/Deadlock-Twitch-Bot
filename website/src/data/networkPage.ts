/**
 * Inhalte der Streamer-Landing V2 (/streamer/v2/).
 *
 * Sprachregelung und Struktur folgen docs/strategie/31 (Positionierung,
 * BrandScript, Verbotsliste) und docs/strategie/32 (Pricing). Bewusst getrennt
 * von data/features.ts, damit die produktive Landing unter /streamer/ ihre
 * eigenen Texte behaelt.
 *
 * Regel aus Kapitel 31: keine Superlative, keine Verknappungs-Countdowns, keine
 * unbelegten Zahlen. Alle Zahlen auf dieser Seite kommen live aus der API oder
 * stehen gar nicht da.
 */

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
    body: "Ein Klick auf Twitch, Freigabe erteilen, fertig. Wir fragen nur die Rechte ab, die das jeweilige Feature wirklich braucht.",
    duration: "ca. 2 Minuten",
  },
  {
    index: "02",
    title: "Netzwerk aktivieren",
    body: "Du wählst, ob dein Stream am Ende automatisch weitergibt und wen du empfangen willst. Beides lässt sich jederzeit abschalten.",
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
  /** Ein Satz Nutzen, mehr nicht. Den Rest erzaehlt das Bild der Karte. */
  body: string;
  /** Kurze Merkmale, die als Chips nebeneinander stehen, keine Saetze. */
  points: string[];
  tone: "primary" | "accent";
}

/** Reihenfolge ist Absicht: Raids zuerst, Moderation ist nicht mehr die Hauptrolle. */
export const valuePillars: ValuePillar[] = [
  {
    id: "raids",
    icon: "Swords",
    kicker: "Der Kern",
    title: "Auto-Raid-Netzwerk",
    body: "Endet dein Stream, geht deine Community nicht offline, sondern zum nächsten deutschen Deadlock-Streamer, der gerade live ist.",
    points: [
      "Partner nach Sprache und Größe",
      "Manueller Raid bleibt",
      "Abschalten mit einem Klick",
    ],
    tone: "primary",
  },
  {
    id: "schutz",
    icon: "ShieldCheck",
    kicker: "Die Grundlage",
    title: "Schutz im Chat",
    body: "Was in einem Partner-Chat als Spam auffällt, ist in allen anderen schon bekannt, bevor es dort ankommt.",
    points: [
      "Gemeinsame Spam-Liste",
      "Wortfilter nach deinen Regeln",
      "In jeder Stufe kostenlos",
    ],
    tone: "accent",
  },
  {
    id: "coaching",
    icon: "Sparkles",
    kicker: "Nach dem Stream",
    title: "Auswertung und Coaching",
    body: "Nach dem Stream siehst du, wann Zuschauer gekommen und gegangen sind, und was daraus für den nächsten Stream folgt.",
    points: [
      "Verlauf und Chat in einer Ansicht",
      "Wochenreport mit nächsten Schritten",
      "Vergleich nur mit dir selbst",
    ],
    tone: "primary",
  },
  {
    id: "clips",
    icon: "Clapperboard",
    kicker: "Zwischen den Streams",
    title: "Clips, die von allein entstehen",
    body: "Der Bot merkt sich die Stellen, an denen im Chat etwas los war, und schneidet daraus fertige Hochkant-Clips.",
    points: [
      "Erkennung über Chat-Ausschläge",
      "Hochkant für TikTok und Shorts",
      "Auto-Posten in Arbeit, Teil von Creator Pro",
    ],
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
    period: "für immer",
    yearly: null,
    anchor: "Vollwertig, ohne Ablaufdatum",
    // Der hervorgehobene Plan ist bewusst der kostenlose, nicht der teuerste.
    featured: true,
    cta: "Kostenlos verbinden",
    ctaHref: "AUTH",
    features: [
      { label: "Auto-Raid-Netzwerk in beide Richtungen", included: true },
      { label: "Kompletter Chat-Schutz", included: true },
      { label: "Go-Live-Post im Community-Discord", included: true },
      { label: "Dashboard mit Grundauswertung", included: true },
      { label: "3 Clips pro Monat, mit Wasserzeichen", included: true },
      { label: "Werbefreier Chat", included: false },
      { label: "Wochenreport und volle Auswertung", included: false },
    ],
  },
  {
    id: "plus",
    name: "Netzwerk Plus",
    price: "4,99 €",
    period: "pro Monat",
    yearly: "39,99 € im Jahr, vier Monate geschenkt",
    anchor: "Der Preis eines Twitch-Subs",
    featured: false,
    cta: "Plus ansehen",
    ctaHref: "ABBO",
    features: [
      { label: "Alles aus Free", included: true },
      { label: "Bevorzugte Platzierung im Raid-Netzwerk", included: true },
      { label: "Werbefreier Chat", included: true },
      { label: "Volle Auswertung und KI-Wochenreport", included: true },
      { label: "Lurker-Erinnerung und eigener Bot-Name", included: true },
      { label: "10 Clips pro Monat, ohne Wasserzeichen", included: true },
      { label: "Automatisches Posten der Clips", included: false },
    ],
    note: "Startet mit dem Netzwerk-Update. Bestehende Abos werden übernommen.",
  },
  {
    id: "pro",
    name: "Creator Pro",
    price: "9,99 €",
    period: "pro Monat",
    yearly: "79,99 € im Jahr, vier Monate geschenkt",
    anchor: "Clip-Werkzeuge kosten am Markt 15 bis 25 $ im Monat",
    featured: false,
    cta: "Pro ansehen",
    ctaHref: "ABBO",
    features: [
      { label: "Alles aus Plus", included: true },
      { label: "Clips ohne Mengenbegrenzung", included: true },
      { label: "Automatisches Posten auf TikTok, Instagram und YouTube", included: true },
      { label: "Untertitel und mehrere Formate", included: true },
      { label: "Vorrang bei Support und neuen Features", included: true },
      { label: "Mehrere Plattformen, sobald verfügbar", included: true },
    ],
    note: "Startet mit dem Netzwerk-Update.",
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
