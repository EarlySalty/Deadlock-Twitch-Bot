import {
  DISCORD_INVITE_URL,
  TWITCH_AFFILIATE_URL,
  TWITCH_AGB_URL,
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_DEMO_DASHBOARD_URL,
  TWITCH_FAQ_URL,
  TWITCH_IMPRESSUM_URL,
  TWITCH_ONBOARDING_URL,
  buildTwitchBotAuthUrl,
  buildTwitchDashboardLoginUrl,
} from "@/data/externalLinks";

export interface LinkRef {
  label: string;
  href: string;
}

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

export interface FaqItem {
  question: string;
  answer: string;
  details: string[];
  access: string;
  tags: string[];
  routes?: LinkRef[];
}

export interface FaqSection {
  id: string;
  badge: string;
  title: string;
  description: string;
  items: FaqItem[];
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

export const FAQ_SECTIONS: FaqSection[] = [];

FAQ_SECTIONS.push(
  {
    id: "einstieg",
    badge: "Einstieg",
    title: "Erste Schritte & Zugang",
    description:
      "Alles was du wissen musst, um loszulegen — von der Anmeldung bis zum ersten Blick ins Dashboard.",
    items: [
      {
        question: "Was ist DDC?",
        answer:
          "DDC ist eine kostenlose Plattform für Deadlock-Streamer auf Twitch. Du bekommst ein Analytics-Dashboard mit Echtzeit-Daten, ein automatisches Raid-Netzwerk, Discord-Automation und KI-gestütztes Coaching — alles an einem Ort.",
        details: [
          "Über 30 Deadlock-Streamer nutzen das Netzwerk bereits.",
          "Das Dashboard hat 13 Tabs mit detaillierten Auswertungen zu deinem Stream.",
          "Du brauchst nur deinen Twitch-Account — keine zusätzliche Registrierung.",
        ],
        access: "Alle",
        tags: ["ddc", "plattform", "deadlock", "streamer", "überblick"],
        routes: [
          { label: "Onboarding", href: TWITCH_ONBOARDING_URL },
          { label: "Demo Dashboard", href: TWITCH_DEMO_DASHBOARD_URL },
        ],
      },
      {
        question: "Wie starte ich?",
        answer:
          "Klick auf \"Bot für deinen Kanal aktivieren\" und verbinde deinen Twitch-Kanal mit dem Deadlock-Partnernetzwerk. Danach landest du direkt im Dashboard.",
        details: [
          "Der gesamte Anmeldeprozess dauert weniger als 30 Sekunden.",
          "Du brauchst nur deinen bestehenden Twitch-Account.",
          "Auto-Raids sind für Deadlock gedacht und greifen nicht bei anderen Spielen.",
        ],
        access: "Alle",
        tags: ["start", "anmeldung", "login", "registrierung", "einloggen"],
        routes: [
          { label: "Bot für deinen Kanal aktivieren", href: buildTwitchBotAuthUrl() },
        ],
      },
      {
        question: "Kostet DDC etwas?",
        answer:
          "Nein, du kannst kostenlos loslegen. Alle Kern-Features wie Analytics, Auto-Raid und Discord-Automation sind ohne Abo nutzbar.",
        details: [
          "Es gibt kein Pflicht-Abo, um die Plattform zu nutzen.",
          "Premium-Features kannst du später im Dashboard entdecken, falls du möchtest.",
          "Du wirst nie ungefragt in ein kostenpflichtiges Abo gesteckt.",
        ],
        access: "Alle",
        tags: ["kosten", "kostenlos", "gratis", "preis", "abo", "bezahlen"],
      },
      {
        question: "Warum sollte ich dem Discord beitreten?",
        answer:
          "Der Discord ist deine beste Quelle für kostenlose Reichweite. Sobald du live gehst, erscheint automatisch ein Go-Live-Post — alle Community-Mitglieder sehen, dass du streamst.",
        details: [
          "Go-Live-Posts werden automatisch gepostet, wenn du live gehst — ohne dass du etwas tun musst.",
          "Es gibt einen eigenen Streamer-Bereich mit exklusiven Infos und direktem Austausch.",
          "Pro-Tipp: Zock mit der Community! Wer mit anderen Deadlock-Spielern unterwegs ist, baut sich organisch eine treue Zuschauerschaft auf.",
        ],
        access: "Alle",
        tags: ["discord", "community", "go-live", "reichweite", "netzwerk"],
        routes: [
          { label: "Discord beitreten", href: DISCORD_INVITE_URL },
        ],
      },
      {
        question: "Gibt es eine Demo?",
        answer:
          "Ja! Es gibt ein öffentliches Demo-Dashboard, in dem du dir alle Features anschauen kannst, bevor du dich anmeldest. Kein Login nötig.",
        details: [
          "Die Demo zeigt echte Dashboard-Funktionen mit Beispieldaten.",
          "Du siehst alle 13 Tabs und kannst die Oberfläche frei erkunden.",
          "Ideal, um dir ein Bild zu machen, bevor du deinen eigenen Account verbindest.",
        ],
        access: "Alle",
        tags: ["demo", "vorschau", "testen", "preview", "ausprobieren"],
        routes: [
          { label: "Demo Dashboard", href: TWITCH_DEMO_DASHBOARD_URL },
        ],
      },
    ],
  },
  {
    id: "analytics",
    badge: "Analytics",
    title: "Dashboard & Analytics",
    description:
      "Alles über dein persönliches Analytics-Dashboard — von Viewer-Trends bis KI-Empfehlungen.",
    items: [
      {
        question: "Was zeigt mir das Dashboard?",
        answer:
          "Dein Dashboard ist dein persönliches Cockpit mit 13 Tabs: Viewer-Trends, Heatmaps, Session-Details, Streamer-Rankings, Zuschauer-Segmente, Chat-Analysen und vieles mehr.",
        details: [
          "Die Overview zeigt dir die wichtigsten Kennzahlen auf einen Blick.",
          "Heatmaps verraten dir, an welchen Tagen und zu welchen Uhrzeiten dein Stream am besten performt.",
          "Session-Details schlüsseln jeden einzelnen Stream nach Viewern, Dauer und Verlauf auf.",
          "Rankings zeigen dir, wo du im Vergleich zu anderen Deadlock-Streamern stehst.",
        ],
        access: "Alle",
        tags: ["dashboard", "analytics", "viewer", "heatmap", "ranking", "session"],
        routes: [
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
          { label: "Demo", href: TWITCH_DEMO_DASHBOARD_URL },
        ],
      },
      {
        question: "Wie aktuell sind die Daten?",
        answer:
          "Die Viewer-Daten werden alle 15 Sekunden aktualisiert. Dein Dashboard zeigt dir also nahezu in Echtzeit, was in deinem Stream passiert.",
        details: [
          "Viewer-Zahlen werden im 15-Sekunden-Takt erfasst und gespeichert.",
          "Session-Daten werden automatisch nach Stream-Ende zusammengefasst.",
          "Historische Daten stehen dir dauerhaft zur Verfügung — nichts wird gelöscht.",
        ],
        access: "Alle",
        tags: ["echtzeit", "aktuell", "daten", "tracking", "live"],
        routes: [
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
        ],
      },
      {
        question: "Kann ich mich mit anderen Streamern vergleichen?",
        answer:
          "Ja! Das Ranking-System zeigt dir, wo du im Deadlock-Streamer-Netzwerk stehst. Du kannst deine Zahlen direkt mit anderen vergleichen.",
        details: [
          "Vergleiche Viewer-Zahlen, Stream-Dauer und Wachstum mit anderen Deadlock-Streamern.",
          "Das Ranking wird regelmäßig aktualisiert und basiert auf echten Daten.",
          "Du siehst auch Trends — ob du im Vergleich zum Netzwerk wächst oder stagnierst.",
        ],
        access: "Alle",
        tags: ["ranking", "vergleich", "streamer", "wettbewerb", "netzwerk"],
        routes: [
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
        ],
      },
      {
        question: "Was ist das KI-Coaching?",
        answer:
          "Das KI-Coaching analysiert deine Stream-Daten und gibt dir personalisierte Empfehlungen: Beste Streaming-Zeiten, welche Titel gut funktionieren, wie du Zuschauer länger hältst.",
        details: [
          "Die KI wertet deine historischen Daten aus und erkennt Muster.",
          "Du bekommst konkrete Tipps, z. B. wann die besten Uhrzeiten für deinen Stream sind.",
          "Titel-Performance zeigt dir, welche Stream-Titel mehr Zuschauer anziehen.",
          "Retention-Analyse verrät, wie lange Zuschauer im Schnitt bleiben.",
        ],
        access: "Alle",
        tags: ["ki", "coaching", "ai", "empfehlung", "tipps", "optimierung"],
        routes: [
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
        ],
      },
    ],
  },
);

FAQ_SECTIONS.push(
  {
    id: "raids",
    badge: "Raids",
    title: "Auto-Raid-Netzwerk",
    description:
      "Wie das automatische Raid-System funktioniert und warum es dein wichtigstes Wachstums-Tool ist.",
    items: [
      {
        question: "Was ist der Auto-Raid?",
        answer:
          "Der Auto-Raid ist ein Kern-Feature von DDC: Wenn dein Stream endet, leitet der Bot deine Zuschauer automatisch an einen passenden Live-Partner im Deadlock-Netzwerk weiter. Das passiert ohne dein Zutun — der Raid ist immer aktiv.",
        details: [
          "Du musst den Auto-Raid nicht aktivieren oder konfigurieren — er läuft automatisch.",
          "Der Bot wählt intelligent den besten Raid-Partner basierend auf mehreren Kriterien.",
          "So bleiben deine Zuschauer im Deadlock-Ökosystem und du hilfst gleichzeitig anderen Streamern.",
        ],
        access: "Alle",
        tags: ["auto-raid", "raid", "automatisch", "netzwerk", "weiterleitung"],
        routes: [
          { label: "Bot für deinen Kanal aktivieren", href: buildTwitchBotAuthUrl() },
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
        ],
      },
      {
        question: "Wie funktioniert die Raid-Auswahl?",
        answer:
          "Der Bot berücksichtigt mehrere Faktoren: Wer ist gerade live, wie viele Viewer hat der Partner, wann wurde zuletzt dorthin geraidet, und weitere Netzwerk-Kriterien. Ziel ist immer eine sinnvolle Weiterleitung, kein Zufall.",
        details: [
          "Live-Status der Partner wird in Echtzeit geprüft.",
          "Cooldowns verhindern, dass immer derselbe Streamer geraidet wird.",
          "Das System sorgt für faire Verteilung im gesamten Netzwerk.",
        ],
        access: "Alle",
        tags: ["raid-auswahl", "algorithmus", "partner", "kriterien"],
        routes: [
          { label: "Bot für deinen Kanal aktivieren", href: buildTwitchBotAuthUrl() },
          { label: "FAQ", href: `${TWITCH_FAQ_URL}#raids` },
        ],
      },
      {
        question: "Kann ich sehen, wen ich geraidet habe?",
        answer:
          "Ja. Nach dem Verbinden siehst du im Dashboard, wie dein Kanal im Netzwerk läuft und welche Raid-Daten für dich erfasst wurden.",
        details: [
          "Dort bekommst du den Überblick über deine Raid-Daten und deine Entwicklung im Netzwerk.",
          "Du siehst Muster über Zeit, statt nur einen einzelnen Moment.",
          "Das Dashboard ist der richtige Einstiegspunkt für deinen Streamer-Bereich.",
        ],
        access: "Alle",
        tags: ["raid history", "raid analytics", "verlauf", "statistik"],
        routes: [
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
        ],
      },
      {
        question: "Profitiere ich auch von Raids anderer?",
        answer:
          "Ja, das Netzwerk arbeitet in beide Richtungen! Wenn andere Streamer offline gehen, können deren Zuschauer automatisch zu dir weitergeleitet werden, solange du live bist.",
        details: [
          "Je aktiver du im Netzwerk bist, desto mehr profitierst du von eingehenden Raids.",
          "Eingehende Raids und deine Entwicklung im Netzwerk findest du später gesammelt im Dashboard.",
          "Das System sorgt dafür, dass alle Partner fair berücksichtigt werden.",
        ],
        access: "Alle",
        tags: ["eingehende raids", "netzwerk", "gegenseitig", "wachstum"],
        routes: [
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
        ],
      },
    ],
  },
  {
    id: "community",
    badge: "Community",
    title: "Community, Discord & Netzwerk",
    description:
      "Discord-Automation, Chat-Commands, Affiliate-Links und alles was die Community zusammenhält.",
    items: [
      {
        question: "Wie funktionieren die Go-Live-Posts?",
        answer:
          "Sobald du auf Twitch live gehst, postet der Bot automatisch eine Benachrichtigung im Discord. Alle Community-Mitglieder sehen sofort, dass du streamst — ohne dass du selbst etwas tun musst.",
        details: [
          "Die Posts enthalten deinen Stream-Titel, das Spiel und einen direkten Link zu deinem Stream.",
          "Du musst nichts konfigurieren — die Posts erscheinen automatisch.",
          "Das gibt dir kostenlose Reichweite bei jedem einzelnen Stream.",
        ],
        access: "Alle",
        tags: ["go-live", "discord", "benachrichtigung", "automatisch", "post"],
        routes: [
          { label: "Discord beitreten", href: DISCORD_INVITE_URL },
        ],
      },
      {
        question: "Welche Chat-Commands gibt es?",
        answer:
          "Der wichtigste Command ist !twl — er zeigt deinen Zuschauern, welche anderen Deadlock-Streamer gerade live sind. So können Viewer zwischen Streams wechseln und das Netzwerk wächst.",
        details: [
          "!twl listet alle aktuell live streamenden Partner im Deadlock-Netzwerk.",
          "Der Command funktioniert in jedem Twitch-Chat, in dem der Bot aktiv ist.",
          "Weitere Commands werden laufend ergänzt.",
        ],
        access: "Alle",
        tags: ["commands", "twl", "chat", "twitch", "bot"],
      },
      {
        question: "Was ist das Affiliate-System?",
        answer:
          "Du kannst im Dashboard eigene Affiliate-Links erstellen und deren Klicks in Echtzeit tracken. Ideal, um Produkte oder Services zu bewerben und den Erfolg direkt zu messen.",
        details: [
          "Erstelle Links direkt im Dashboard — kein externes Tool nötig.",
          "Klick-Statistiken zeigen dir, welche Links am besten performen.",
          "Du kannst Links jederzeit bearbeiten, pausieren oder löschen.",
        ],
        access: "Alle",
        tags: ["affiliate", "links", "klicks", "tracking", "werbung"],
        routes: [
          { label: "Affiliate", href: TWITCH_AFFILIATE_URL },
        ],
      },
      {
        question: "Was ist das Viewer-Leaderboard?",
        answer:
          "Das Leaderboard zeigt dir, wer deine treuesten Zuschauer sind. Es trackt, wie oft und wie lange Viewer in deinem Stream sind, und erstellt daraus ein Ranking.",
        details: [
          "Erkenne deine loyalsten Community-Mitglieder auf einen Blick.",
          "Das Leaderboard basiert auf echten Viewing-Daten, nicht auf Chat-Aktivität allein.",
          "Ein tolles Tool, um deine Community besser kennenzulernen und wertzuschätzen.",
        ],
        access: "Alle",
        tags: ["leaderboard", "viewer", "community", "ranking", "treue"],
        routes: [
          {
            label: "Dashboard öffnen",
            href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2"),
          },
        ],
      },
    ],
  },
);

FAQ_SECTIONS.push({
  id: "affiliate",
  badge: "Affiliate",
  title: "Vertriebler-Programm",
  description:
    "Alles über das Affiliate-Programm: Anmeldung, Stripe, Provisionen und wie du Streamer wirbst.",
  items: [
    {
      question: "Was ist das Vertriebler-Programm?",
      answer:
        "Du wirbst Deadlock-Streamer für DDC und bekommst dauerhaft automatisch 30% Provision auf jede Zahlung deiner geworbenen Streamer.",
      details: [
        "Die Provision läuft dauerhaft weiter, solange dein geworbener Streamer zahlt.",
        "Die Zuordnung erfolgt über das Affiliate-Portal und ist eindeutig pro Streamer.",
        "Das Modell ist auf laufende Provisionen statt einmalige Boni ausgelegt.",
      ],
      access: "Alle",
      tags: ["affiliate", "vertriebler", "provision", "programm", "streamer"],
      routes: [
        { label: "Affiliate", href: TWITCH_AFFILIATE_URL },
      ],
    },
    {
      question: "Wie melde ich mich an?",
      answer:
        "Du aktivierst den Bot für deinen Kanal und kannst danach direkt loslegen. Kein Formular, kein Papierkram, in etwa 2 Minuten erledigt.",
      details: [
        "Die Anmeldung läuft direkt über deinen Twitch-Account.",
        "Es werden keine separaten Registrierungsformulare verlangt.",
        "Nach dem Login kannst du direkt ins Affiliate-Portal wechseln.",
      ],
      access: "Alle",
      tags: ["anmeldung", "login", "twitch", "affiliate", "start"],
      routes: [
        { label: "Affiliate", href: TWITCH_AFFILIATE_URL },
      ],
    },
    {
      question: "Wie funktioniert Stripe Connect?",
      answer:
        "Du verbindest dein Stripe-Konto einmalig. Das dauert ungefähr 5 Minuten. Danach werden deine Provisionen bei jeder Streamer-Zahlung automatisch ausgezahlt.",
      details: [
        "Stripe Connect ist die Auszahlungsschiene für deine Provisionen.",
        "Nach der Einrichtung landen Auszahlungen automatisch auf deinem Bankkonto.",
        "Die Verknüpfung ist nur einmalig notwendig.",
      ],
      access: "Alle",
      tags: ["stripe", "stripe connect", "auszahlung", "bankkonto", "affiliate"],
      routes: [
        { label: "Affiliate", href: TWITCH_AFFILIATE_URL },
      ],
    },
    {
      question: "Wie wirbt man Streamer?",
      answer:
        "Du findest Deadlock-Streamer auf Twitch, in Discord-Servern oder in Communities, empfiehlst DDC und beanspruchst den Streamer danach im Portal. Sobald er zahlt, bekommst du 30%.",
      details: [
        "Geeignet sind Streamer, die noch nicht bei DDC registriert sind.",
        "Die Zuordnung erfolgt über den Twitch-Login des Streamers im Portal.",
        "Jeder beanspruchte Streamer kann nur einem Vertriebler zugeordnet sein.",
      ],
      access: "Alle",
      tags: ["streamer", "werben", "claim", "portal", "provision"],
      routes: [
        { label: "Affiliate", href: TWITCH_AFFILIATE_URL },
        { label: "Discord beitreten", href: DISCORD_INVITE_URL },
      ],
    },
    {
      question: "Was passiert ohne Stripe-Konto?",
      answer:
        "Provisionen werden bis 50€ gespeichert. Darüber hinaus verfallen sie. Deshalb lohnt es sich, Stripe möglichst früh zu verbinden.",
      details: [
        "Ohne Stripe gehen kleine Provisionen nicht sofort verloren.",
        "Die Speichergrenze liegt bei insgesamt 50€ offenen Provisionen.",
        "Sobald Stripe verbunden ist, lohnt sich die automatische Auszahlung sofort.",
      ],
      access: "Alle",
      tags: ["ohne stripe", "50 euro", "limit", "pending", "affiliate"],
      routes: [
        { label: "Affiliate", href: TWITCH_AFFILIATE_URL },
      ],
    },
  ],
});

FAQ_SECTIONS.push({
  id: "support",
  badge: "Support",
  title: "Hilfe, Konto & Rechtliches",
  description:
    "Antworten zu Support, Account-Verwaltung und rechtlichen Informationen.",
  items: [
    {
      question: "Wo bekomme ich Hilfe?",
      answer:
        "Der schnellste Weg ist der Discord — dort gibt es einen Support-Bereich, in dem dir direkt geholfen wird. Alternativ findest du Antworten in dieser FAQ oder im Onboarding.",
      details: [
        "Im Discord antworten erfahrene Community-Mitglieder und das Team.",
        "Die FAQ deckt die häufigsten Fragen ab — nutze die Suche oben.",
        "Das Onboarding erklärt den Einstieg Schritt für Schritt.",
      ],
      access: "Alle",
      tags: ["hilfe", "support", "fragen", "kontakt", "discord"],
      routes: [
        { label: "Discord beitreten", href: DISCORD_INVITE_URL },
        { label: "Onboarding", href: TWITCH_ONBOARDING_URL },
      ],
    },
    {
      question: "Wie lösche ich meinen Account oder widerrufe den Zugang?",
      answer:
        "Du kannst den Bot-Zugang jederzeit über deine Twitch-Einstellungen widerrufen. Geh dazu in deine Twitch-Verbindungen und entferne die DDC-Autorisierung. Für eine komplette Datenlöschung kontaktiere uns im Discord.",
      details: [
        "Twitch-Autorisierung widerrufen: Twitch → Einstellungen → Verbindungen → DDC entfernen.",
        "Nach dem Widerruf hat der Bot keinen Zugriff mehr auf deinen Account.",
        "Für eine vollständige Löschung deiner gespeicherten Daten melde dich im Discord.",
      ],
      access: "Alle",
      tags: ["löschen", "account", "widerrufen", "kündigung", "datenlöschung", "abmelden"],
      routes: [
        { label: "Discord (Support)", href: DISCORD_INVITE_URL },
      ],
    },
    {
      question: "Wo finde ich Impressum, Datenschutz und AGB?",
      answer:
        "Impressum, Datenschutzerklärung und AGB sind öffentlich auf der Website verfügbar — kein Login nötig.",
      details: [
        "Alle rechtlichen Dokumente sind jederzeit ohne Anmeldung einsehbar.",
        "Die Links findest du auch im Footer jeder Seite.",
        "Bei Fragen zum Datenschutz kannst du dich im Discord oder per E-Mail melden.",
      ],
      access: "Alle",
      tags: ["impressum", "datenschutz", "agb", "rechtliches", "legal", "dsgvo"],
      routes: [
        { label: "Impressum", href: TWITCH_IMPRESSUM_URL },
        { label: "Datenschutz", href: TWITCH_DATENSCHUTZ_URL },
        { label: "AGB", href: TWITCH_AGB_URL },
      ],
    },
  ],
});
