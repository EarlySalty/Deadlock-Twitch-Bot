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
        question: "Was ist Deutsche Deadlock Community?",
        answer:
          "Deutsche Deadlock Community ist eine kostenlose Plattform für Deadlock-Streamer auf Twitch. Du bekommst ein Analytics-Dashboard mit Echtzeit-Daten, ein automatisches Raid-Netzwerk, Discord-Automation und KI-gestütztes Coaching — alles an einem Ort.",
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
        question: "Kostet Deutsche Deadlock Community etwas?",
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
          "Der Auto-Raid ist ein Kern-Feature von Deutsche Deadlock Community: Wenn dein Stream endet, leitet der Bot deine Zuschauer automatisch an einen passenden Live-Partner im Deadlock-Netzwerk weiter. Das passiert ohne dein Zutun — der Raid ist immer aktiv.",
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
        "Du wirbst Deadlock-Streamer für Deutsche Deadlock Community und bekommst dauerhaft automatisch 30% Provision auf jede Zahlung deiner geworbenen Streamer.",
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
        "Du findest Deadlock-Streamer auf Twitch, in Discord-Servern oder in Communities, empfiehlst Deutsche Deadlock Community und beanspruchst den Streamer danach im Portal. Sobald er zahlt, bekommst du 30%.",
      details: [
        "Geeignet sind Streamer, die noch nicht bei Deutsche Deadlock Community registriert sind.",
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
  id: "funktionen",
  badge: "Funktionen",
  title: "Was macht der Bot eigentlich?",
  description:
    "Die wichtigsten Aufgaben des Bots auf einen Blick — was er für dich erledigt und was nicht.",
  items: [
    {
      question: "Welche Hauptaufgaben übernimmt der Bot für mich?",
      answer:
        "Der Bot kümmert sich um vier Dinge: Er leitet beim Stream-Ende deine Zuschauer automatisch an einen live Deadlock-Partner weiter (Auto-Raid), trackt deine Stream-Zahlen für dein Dashboard, schickt bei Bedarf eine dezente Discord-Einladung in deinen Chat und bringt optionale Extras wie Lurker-Erinnerungen oder KI-Stream-Reports mit.",
      details: [
        "Auto-Raid läuft ohne Setup — der Bot wählt den passenden Partner aus.",
        "Analytics werden im Hintergrund erfasst, du musst nichts konfigurieren.",
        "Chat-Werbung kannst du im Dashboard steuern oder mit dem Werbefrei-Plan komplett abschalten.",
        "Stream-Reports (KI) und Lurker-Tax sind optionale Premium-Features.",
      ],
      access: "Alle",
      tags: ["funktionen", "bot", "überblick", "aufgaben", "features"],
      routes: [
        { label: "Dashboard öffnen", href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2") },
      ],
    },
    {
      question: "Welche Rechte braucht der Bot in meinem Kanal?",
      answer:
        "Der Bot meldet sich per Twitch-Login an und braucht ein paar Standard-Rechte: Chat-Nachrichten lesen und schreiben, Live-Status sehen, Raid auslösen. Für ein paar optionale Features (z. B. Lurker-Tax) bittet er zusätzlich um den Moderator-Lesescope für Chatter — das siehst du beim Login.",
      details: [
        "Du autorisierst den Bot über deinen Twitch-Account — kein extra Passwort.",
        "Der Bot muss zusätzlich als Mod in deinem Chat angekündigt sein, damit Announcements sauber funktionieren.",
        "Du kannst die Verbindung jederzeit in deinen Twitch-Einstellungen widerrufen.",
      ],
      access: "Alle",
      tags: ["rechte", "scopes", "mod", "berechtigung", "twitch", "login"],
      routes: [
        { label: "Bot für deinen Kanal aktivieren", href: buildTwitchBotAuthUrl() },
      ],
    },
    {
      question: "Was passiert, wenn ich offline bin?",
      answer:
        "Der Bot bleibt erreichbar, schickt aber keine Chat-Aktionen mehr — Werbung, Lurker-Erinnerungen und ähnliches greifen nur, wenn dein Stream läuft. Sobald dein Stream endet, prüft der Bot, ob er deine Zuschauer per Auto-Raid weiterleiten kann.",
      details: [
        "Während du offline bist, ruhen alle Chat-Trigger.",
        "Der Auto-Raid wird genau einmal pro Stream-Ende ausgelöst.",
        "Deine Analytics-Daten werden weiter im Dashboard zugänglich gemacht.",
      ],
      access: "Alle",
      tags: ["offline", "stream-ende", "raid", "ruhend", "auto-raid"],
    },
    {
      question: "Bekomme ich mit, was der Bot in meinem Chat sendet?",
      answer:
        "Ja — jede vom Bot gesendete Nachricht steht klar erkennbar in deinem Chat mit dem Bot-Account als Absender. Im Dashboard kannst du außerdem die letzten Aktivitäten einsehen und ggf. Werbung deaktivieren oder den Text anpassen.",
      details: [
        "Alle Bot-Nachrichten laufen unter einem dedizierten Bot-Account.",
        "Im Dashboard siehst du jüngste Aktionen und kannst Einstellungen ändern.",
        "Mit dem Werbefrei-Plan unterbindest du die Discord-Einladungen komplett.",
      ],
      access: "Alle",
      tags: ["chat", "transparenz", "nachrichten", "bot-account", "logs"],
      routes: [
        { label: "Dashboard öffnen", href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2") },
      ],
    },
  ],
});

FAQ_SECTIONS.push({
  id: "plaene",
  badge: "Pläne & Preise",
  title: "Pläne, Preise und was sie bringen",
  description:
    "Was kostet was, was bekommst du dafür und wann lohnt sich ein Upgrade — alles ohne Marketing-Sprech.",
  items: [
    {
      question: "Was ist kostenlos und was kostet etwas?",
      answer:
        "Die komplette Plattform inklusive Auto-Raid, Basis-Dashboard, Discord-Go-Live-Posts und Chat-Commands ist und bleibt kostenlos. Bezahl-Pläne gibt es nur für drei Extras: Chat-Werbung deaktivieren, bevorzugte Raid-Platzierung und das volle Analytics-/KI-Coaching.",
      details: [
        "Free reicht für den Großteil dessen, was Streamer aus dem Netzwerk holen.",
        "Bezahlt wird nur, wenn du gezielt Werbung-aus, mehr Raids oder KI-Analysen willst.",
        "Es gibt keine versteckten Limits — Free ist ein vollwertiger Plan.",
      ],
      access: "Alle",
      tags: ["kostenlos", "free", "preis", "abo", "kosten", "übersicht"],
      routes: [
        { label: "Dashboard öffnen", href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2") },
      ],
    },
    {
      question: "Was bringt mir der Werbefrei-Plan (3,99 €/Monat)?",
      answer:
        "Werbefrei macht genau eine Sache: Der Bot schickt keine Discord-Einladung mehr in deinen Chat — egal aus welchem Anlass. Sonst bleibt alles wie im Free-Plan. Ideal, wenn du keine Bot-Werbung in deinem Chat willst, aber auf Raid Boost oder Analytics verzichten kannst.",
      details: [
        "Discord-Einladung wird dauerhaft nicht mehr gesendet.",
        "Greift auch, wenn ein Admin gerade einen globalen Aktions-Text aktiviert hat.",
        "Auto-Raid, Dashboard, Go-Live-Posts laufen ganz normal weiter.",
        "Monatlich kündbar, kein Vertragsbindung-Quatsch.",
      ],
      access: "Alle",
      tags: ["werbefrei", "werbung", "chat-werbung", "3,99", "abo", "plan", "ruhe"],
      routes: [
        { label: "Pläne ansehen", href: buildTwitchDashboardLoginUrl("/twitch/abbo") },
      ],
    },
    {
      question: "Wann lohnt sich Raid Boost (3,99 €/Monat)?",
      answer:
        "Raid Boost lohnt sich, wenn du gezielt mehr eingehende Zuschauer willst. Dein Kanal wird im Raid-Netzwerk bevorzugt als Ziel vorgeschlagen — auch dann, wenn du selbst gerade nicht live bist. Zusätzlich bekommst du Lurker-Tax-Erinnerungen und das KI-Mini-Modell für leichte Stream-Insights.",
      details: [
        "Du wirst öfter als Raid-Ziel ausgewählt — mehr eingehende Viewer.",
        "Lurker-Tax erinnert dich an Zuschauer, die seit langem im Chat hängen ohne zu schreiben.",
        "Analytics-Basics und KI-Mini-Reports sind enthalten.",
        "Chat-Werbung bleibt aktiv — wenn du auch Ruhe willst, nimm das Combo.",
      ],
      access: "Alle",
      tags: ["raid boost", "raid", "boost", "3,99", "wachstum", "lurker"],
      routes: [
        { label: "Pläne ansehen", href: buildTwitchDashboardLoginUrl("/twitch/abbo") },
      ],
    },
    {
      question: "Lohnt sich das Combo 'Werbefrei + Raid Boost' (5,99 €)?",
      answer:
        "Wenn du beides willst — keine Bot-Werbung und mehr eingehende Raids — ist das Combo der günstigere Weg. Einzeln wären die zwei Pläne 7,98 €, im Combo zahlst du nur 5,99 €. Das spart dir 2 € pro Monat.",
      details: [
        "Werbefrei und Raid Boost in einem Plan.",
        "5,99 € statt 7,98 € — ein Viertel günstiger.",
        "Wechsel jederzeit im Dashboard möglich.",
      ],
      access: "Alle",
      tags: ["combo", "bundle", "werbefrei", "raid boost", "5,99", "rabatt"],
      routes: [
        { label: "Pläne ansehen", href: buildTwitchDashboardLoginUrl("/twitch/abbo") },
      ],
    },
    {
      question: "Was ist der Unterschied zwischen Erweitert (8,49 €) und dem großen Bundle (11,49 €)?",
      answer:
        "Erweitert schaltet dir das volle Analytics-Dashboard mit KI-Coaching frei — Viewer-Profile, Retention-Analyse, Coaching-Empfehlungen. Das große Bundle ist Erweitert plus Raid Boost plus Werbefrei in einem Paket — also das komplette Programm in einer Buchung.",
      details: [
        "Erweitert: volles Analytics + KI-Voll-Modell.",
        "Bundle: Erweitert + Raid Boost + Werbefrei.",
        "Wenn du nur Analytics brauchst, reicht Erweitert.",
        "Das Bundle spart, sobald du mehr als zwei der drei Bausteine willst.",
      ],
      access: "Alle",
      tags: ["erweitert", "analyse", "bundle", "11,49", "8,49", "ki", "analytics"],
      routes: [
        { label: "Pläne ansehen", href: buildTwitchDashboardLoginUrl("/twitch/abbo") },
      ],
    },
    {
      question: "Kann ich monatlich kündigen oder zwischen Plänen wechseln?",
      answer:
        "Ja, alle Pläne sind monatlich kündbar. Wenn du 6 oder 12 Monate im Voraus zahlst, gibt es einen Rabatt (10 % bzw. 20 %). Plan-Wechsel sind jederzeit möglich — der Rest des aktuellen Zyklus wird verrechnet.",
      details: [
        "Monatlich, halbjährlich (10 % Rabatt) oder jährlich (20 % Rabatt) wählbar.",
        "Kündigung über das Dashboard, keine E-Mail nötig.",
        "Plan-Wechsel sofort wirksam, Restguthaben wird verrechnet.",
      ],
      access: "Alle",
      tags: ["kündigung", "wechsel", "monatlich", "rabatt", "laufzeit", "upgrade", "downgrade"],
      routes: [
        { label: "Pläne ansehen", href: buildTwitchDashboardLoginUrl("/twitch/abbo") },
      ],
    },
  ],
});

FAQ_SECTIONS.push({
  id: "werbung",
  badge: "Chat-Werbung",
  title: "Chat-Werbung des Bots",
  description:
    "Was der Bot in deinen Chat schickt, wann, und wie du das komplett abstellst.",
  items: [
    {
      question: "Welche Werbung schickt der Bot in meinen Chat?",
      answer:
        "Der Bot postet eine kurze Discord-Einladung in deinen Chat, damit deine Zuschauer Teil der Deadlock-Community werden können. Es geht ausschließlich um den Community-Discord — keine externen Sponsoren, keine fremden Produkte, kein Spam.",
      details: [
        "Inhalt: kurzer Hinweis-Text plus Discord-Einladungslink.",
        "Keine Werbung für externe Produkte oder Drittanbieter.",
        "Du kannst den Text im Dashboard durch deinen eigenen ersetzen.",
      ],
      access: "Alle",
      tags: ["werbung", "discord", "promo", "chat", "einladung"],
      routes: [
        { label: "Dashboard öffnen", href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2") },
      ],
    },
    {
      question: "Wann genau wird die Werbung gepostet?",
      answer:
        "Die Werbung greift nur, wenn dein Stream läuft und nur, wenn auch wirklich neue Zuschauer im Chat sind. Es gibt eingebaute Cooldowns und Mindest-Aktivitäts-Schwellen, damit es nicht spammt — typischerweise sehen Zuschauer eine Einladung höchstens alle paar Stunden.",
      details: [
        "Triggert nur, wenn neue Chatter im aktuellen Fenster aktiv waren.",
        "Cooldown verhindert wiederholte Einblendungen für dieselben Zuschauer.",
        "Bei aktiven Sonder-Events kann der Bot stattdessen einen Aktions-Text einblenden.",
      ],
      access: "Alle",
      tags: ["trigger", "wann", "cooldown", "frequenz", "spam"],
    },
    {
      question: "Wie schalte ich die Chat-Werbung komplett ab?",
      answer:
        "Mit dem Werbefrei-Plan (3,99 €/Monat) oder einem der Bundles, die ihn enthalten. Sobald der Plan aktiv ist, sendet der Bot in deinem Chat keinerlei Werbung mehr — auch nicht, wenn andere Trigger eigentlich greifen würden.",
      details: [
        "Werbefrei: 3,99 €/Monat, einziger Effekt ist Werbung-aus.",
        "Werbefrei + Raid Boost (Combo): 5,99 €/Monat.",
        "Großes Bundle (Erweitert + Raid Boost + Werbefrei): 11,49 €/Monat.",
        "Plan im Dashboard buchen, Effekt greift sofort.",
      ],
      access: "Alle",
      tags: ["abstellen", "deaktivieren", "ausschalten", "werbefrei", "ruhe", "plan"],
      routes: [
        { label: "Pläne ansehen", href: buildTwitchDashboardLoginUrl("/twitch/abbo") },
      ],
    },
    {
      question: "Gilt 'Werbefrei' auch bei Sonder-Events vom Admin?",
      answer:
        "Ja. Wenn ein Admin global einen Aktions-Text aktiviert (z. B. zu einem Community-Event), gilt das ausdrücklich nicht für Streamer mit Werbefrei-Plan. Der Plan überschreibt jeden globalen Werbe-Override — komplett kein Bot-Werbungstext in deinem Chat, ohne Ausnahme.",
      details: [
        "Werbefrei-Streamer bekommen auch bei aktivem globalem Sonder-Text nichts gesendet.",
        "Die Sperre greift in jedem Trigger-Pfad — Chat-Aktivität, Viewer-Anstieg oder Zeitplan.",
        "Du kannst dich darauf verlassen, dass 'Werbefrei' wirklich werbefrei ist.",
      ],
      access: "Alle",
      tags: ["admin", "override", "event", "global", "werbefrei", "ausnahme"],
    },
    {
      question: "Kann ich nur den Werbe-Text anpassen, ohne Werbefrei zu buchen?",
      answer:
        "Ja. Im Dashboard kannst du den Werbe-Text durch einen eigenen ersetzen — dann postet der Bot deinen statt des Default-Texts. Das ist kostenlos und für alle Pläne verfügbar. Den Discord-Link kannst du als Platzhalter einbauen.",
      details: [
        "Eigener Werbe-Text im Dashboard hinterlegbar.",
        "Platzhalter {invite} wird beim Senden durch den Discord-Link ersetzt.",
        "Wenn ein Admin gerade einen Aktions-Text aktiviert hat, hat dieser kurzzeitig Vorrang vor deinem eigenen.",
      ],
      access: "Alle",
      tags: ["text", "anpassen", "eigener text", "promo-text", "override"],
      routes: [
        { label: "Dashboard öffnen", href: buildTwitchDashboardLoginUrl("/twitch/dashboard-v2") },
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
        "Du kannst den Bot-Zugang jederzeit über deine Twitch-Einstellungen widerrufen. Geh dazu in deine Twitch-Verbindungen und entferne die Deutsche Deadlock Community-Autorisierung. Für eine komplette Datenlöschung kontaktiere uns im Discord.",
      details: [
        "Twitch-Autorisierung widerrufen: Twitch → Einstellungen → Verbindungen → Deutsche Deadlock Community entfernen.",
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
