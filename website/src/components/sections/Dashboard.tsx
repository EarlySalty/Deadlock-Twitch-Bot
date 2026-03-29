import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  BarChart3,
  GraduationCap,
  LayoutDashboard,
  MessageSquare,
  Sparkles,
  Target,
  UserSearch,
  Users,
} from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { BrowserMockup } from "@/components/ui/BrowserMockup";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { TWITCH_DEMO_DASHBOARD_URL } from "@/data/externalLinks";

const PRODUCT_TABS: Array<{ id: string; label: string; beta?: boolean }> = [
  { id: "overview", label: "Übersicht" },
  { id: "streams", label: "Streams" },
  { id: "chat", label: "Chat" },
  { id: "growth", label: "Wachstum" },
  { id: "audience", label: "Audience" },
  { id: "viewers", label: "Viewer" },
  { id: "compare", label: "Vergleich" },
  { id: "schedule", label: "Zeitplan" },
  { id: "coaching", label: "Coaching" },
  { id: "monetization", label: "Monetization" },
  { id: "category", label: "Kategorie" },
  { id: "experimental", label: "Labor", beta: true },
  { id: "ai", label: "KI Analyse", beta: true },
] as const;

const TABS = [
  {
    id: "overview",
    label: "Übersicht",
    Icon: LayoutDashboard,
    teaser: "Viewer, Peak, Chat und Momentum in einem schnellen Einstieg.",
    title: "Alles Wichtige in einem Board",
    description:
      "Die Übersicht verdichtet die wichtigsten Signale, bevor du tiefer in Chat, Audience, Viewer, Wachstum oder Coaching gehst.",
    stats: [
      ["Ø Viewer", "142", "+12%"],
      ["Peak", "218", "20:15"],
      ["Chatters", "63", "starke Basis"],
    ],
    signals: [
      "Der schnelle Daily Check-in fuer Performance und Richtung.",
      "Macht auffaellige Veraenderungen sichtbar, bevor man Details liest.",
      "Perfekter Startpunkt fuer alle anderen Analytics-Tabs.",
    ],
  },
  {
    id: "chat",
    label: "Chat",
    Icon: MessageSquare,
    teaser: "Chat-Tiefe, Aktivitaet und Wiederkehrer statt nur Message Count.",
    title: "Chat als echte Community lesen",
    description:
      "Hier wird sichtbar, wann der Chat wirklich lebt, wie tief die Gespräche gehen und ob aus Aktivität echte Bindung entsteht.",
    stats: [
      ["Aktive Chatters", "91", "38 wiederkehrend"],
      ["Penetration", "31%", "stark im Kernslot"],
      ["Msg / 100 VM", "46.2", "gute Tiefe"],
    ],
    signals: [
      "Peak-Momente und Tageszeit-Signale werden sofort lesbar.",
      "Neue und wiederkehrende Chatter lassen sich klar trennen.",
      "Hilft, Unterhaltung statt nur Aktivitaet zu bewerten.",
    ],
  },
  {
    id: "audience",
    label: "Audience",
    Icon: Target,
    teaser: "Core Audience, Discovery und Cross-Community sauber getrennt.",
    title: "Audience mit echtem Kontext",
    description:
      "Audience zeigt nicht nur, wie viele Leute da waren, sondern welche Gruppen bleiben, entdecken oder über Partner-Netzwerke ankommen.",
    stats: [
      ["Core", "46%", "wiederkehrend"],
      ["Neu", "28%", "Discovery"],
      ["Shared", "17%", "Partner-Fit"],
    ],
    signals: [
      "Zeigt, welche Reichweite wirklich Bindung aufbaut.",
      "Hilft bei Raid-Entscheidungen und Partner-Matching.",
      "Macht Cross-Community innerhalb des Netzwerks sichtbar.",
    ],
  },
  {
    id: "viewers",
    label: "Viewer",
    Icon: UserSearch,
    teaser: "Viewer werden als Profile und Segmente lesbar, nicht nur als Zahl.",
    title: "Viewer-Daten endlich nutzbar",
    description:
      "Der Viewer-Tab macht Wiederkehrer, Dormant Viewer und besonders wertvolle Community-Profile sichtbar.",
    stats: [
      ["Wiederkehrer", "58%", "mehrfach aktiv"],
      ["Dormant", "24", "rueckholbar"],
      ["High Value", "19", "Chat + Watchtime"],
    ],
    signals: [
      "Hilft bei Rewards, Reaktivierung und Community-Pflege.",
      "Macht Viewer-Verhalten über einzelne Streams hinaus sichtbar.",
      "Verbindet Chat-, Audience- und Growth-Signale sinnvoll.",
    ],
  },
  {
    id: "growth",
    label: "Wachstum",
    Icon: BarChart3,
    teaser: "Wachstum liest Titel, Timing, Trends und Raid-Retention zusammen.",
    title: "Wachstum als Muster statt Zufall",
    description:
      "Hier laufen Monatsentwicklung, Tags, Titel, Wochentage und Raid-Retention zusammen, damit du den Grund hinter dem Wachstum erkennst.",
    stats: [
      ["Hours Watched", "18.4k", "+16%"],
      ["Follower", "+412", "stabil"],
      ["Raid Retention", "63%", "nach 10 Min."],
    ],
    signals: [
      "Macht echte Wachstumshebel statt nur Endwerte sichtbar.",
      "Verbindet Schedule, Titel und Reichweite in einer Sicht.",
      "Zeigt, ob Partner-Raids nachhaltig weitertragen.",
    ],
  },
  {
    id: "coaching",
    label: "Coaching",
    Icon: GraduationCap,
    teaser: "Analytics wird in konkrete nächste Schritte übersetzt.",
    title: "Coaching macht Analytics handlungsfaehig",
    description:
      "Der Coaching-Tab verdichtet Daten in priorisierte Empfehlungen für Timing, Titel, Retention, Community und Netzwerk.",
    stats: [
      ["Top Hebel", "3", "priorisiert"],
      ["Gap", "6%", "zum Peer-Cluster"],
      ["Effizienz", "1.8x", "Viewer-Hours"],
    ],
    signals: [
      "Bringt Analyse und konkrete Maßnahmen zusammen.",
      "Gibt Daten durch Peer-Vergleiche einen Maßstab.",
      "Hilft direkt bei Content-, Titel- und Timing-Entscheidungen.",
    ],
  },
] as const;

type DemoTabId = (typeof TABS)[number]["id"];

const callouts = [
  {
    Icon: LayoutDashboard,
    title: "13 echte Perspektiven",
    description:
      "Neben der Vorschau gehören auch Streams, Vergleich, Zeitplan, Kategorie, Monetization, Labor und KI Analyse zum Produkt.",
  },
  {
    Icon: Users,
    title: "Viewer mit Struktur",
    description:
      "Nicht nur Durchschnittswerte: Wiederkehrer, Discovery, Core Audience und Cross-Community lassen sich getrennt lesen.",
  },
  {
    Icon: Sparkles,
    title: "Coaching mit Kontext",
    description:
      "Empfehlungen entstehen aus Timing, Titeln, Retention, Konkurrenz und Netzwerk statt aus isolierten Metriken.",
  },
];

export function Dashboard() {
  const [activeTab, setActiveTab] = useState<DemoTabId>("chat");
  const activeDemo = TABS.find((tab) => tab.id === activeTab) ?? TABS[0];
  const ActiveIcon = activeDemo.Icon;
  const secondaryTabs = PRODUCT_TABS.filter(
    (tab) => !TABS.some((previewTab) => previewTab.id === tab.id),
  );

  return (
    <section id="dashboard" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge="Analytics"
          title="Analytics auf einem neuen Level"
          subtitle="13 spezialisierte Tabs fuer jeden Aspekt deines Streams. Chat, Audience, Viewer, Wachstum und Coaching greifen direkt ineinander statt isoliert nebeneinander zu stehen."
        />

        {/* Live Demo iframe */}
        <ScrollReveal delay={0.1}>
          <div className="mt-12">
            <BrowserMockup url="demo.earlysalty.com/twitch/demo">
              <div className="relative aspect-video overflow-hidden rounded bg-gradient-to-br from-[var(--color-card)] to-[var(--color-bg)]">
                <iframe
                  src={TWITCH_DEMO_DASHBOARD_URL}
                  title="Twitch Analyse Demo Live View"
                  className="absolute inset-0 w-full h-full border-0"
                  loading="lazy"
                  referrerPolicy="no-referrer"
                />
                <span className="pointer-events-none absolute left-3 top-3 rounded-full border border-[var(--color-border)] bg-[rgba(7,21,29,0.78)] px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-[var(--color-accent)]">
                  Live Demo
                </span>
              </div>
            </BrowserMockup>
            <div className="mt-4 text-center">
              <a
                href={TWITCH_DEMO_DASHBOARD_URL}
                target="_blank"
                rel="noopener noreferrer"
                className="text-[var(--color-primary)] hover:text-[var(--color-primary-hover)] font-semibold transition text-sm"
              >
                Vollansicht öffnen →
              </a>
            </div>
          </div>
        </ScrollReveal>

        {/* Tab buttons */}
        <ScrollReveal delay={0.15}>
          <div className="mt-12 flex flex-wrap gap-2 justify-center">
            {TABS.map((tab) => {
              const isActive = tab.id === activeTab;
              const Icon = tab.Icon;

              return (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={[
                    "rounded-lg px-4 py-2 text-sm transition inline-flex items-center gap-2",
                    isActive
                      ? "gradient-accent text-white"
                      : "bg-[var(--color-card)] border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]",
                  ].join(" ")}
                >
                  <Icon size={16} />
                  {tab.label}
                </button>
              );
            })}
          </div>

          <p className="mt-4 text-center text-sm text-[var(--color-text-secondary)] max-w-3xl mx-auto">
            {activeDemo.teaser}
          </p>
        </ScrollReveal>

        {/* Active tab content */}
        <ScrollReveal delay={0.2}>
          <div className="mt-8">
            <AnimatePresence mode="wait">
              <motion.div
                key={activeTab}
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -8 }}
                transition={{ duration: 0.28, ease: "easeOut" }}
              >
                <div className="panel-card rounded-2xl p-6 md:p-8">
                  <div className="flex items-center gap-3 mb-4">
                    <div className="w-10 h-10 rounded-lg gradient-accent flex items-center justify-center">
                      <ActiveIcon size={18} className="text-white" />
                    </div>
                    <h3 className="text-xl font-semibold text-[var(--color-text-primary)]">
                      {activeDemo.title}
                    </h3>
                  </div>
                  <p className="text-[var(--color-text-secondary)] leading-relaxed mb-6">
                    {activeDemo.description}
                  </p>

                  {/* Stats row */}
                  <div className="grid grid-cols-3 gap-3 mb-6">
                    {activeDemo.stats.map(([label, value, detail]) => (
                      <div
                        key={label}
                        className="rounded-xl border border-[var(--color-border)] bg-[rgba(255,255,255,0.03)] p-3"
                      >
                        <p className="text-xs uppercase tracking-wider text-[var(--color-text-secondary)]">
                          {label}
                        </p>
                        <p className="mt-1 text-lg font-bold text-[var(--color-text-primary)]">
                          {value}
                        </p>
                        <p className="text-xs text-[var(--color-text-secondary)]">
                          {detail}
                        </p>
                      </div>
                    ))}
                  </div>

                  {/* Signals */}
                  <div className="space-y-3">
                    {activeDemo.signals.map((signal) => (
                      <div
                        key={signal}
                        className="flex items-start gap-3 text-sm text-[var(--color-text-secondary)]"
                      >
                        <span className="mt-1.5 h-2 w-2 rounded-full bg-[var(--color-accent)] shrink-0" />
                        <span>{signal}</span>
                      </div>
                    ))}
                  </div>
                </div>
              </motion.div>
            </AnimatePresence>

            {/* Secondary tabs badge box */}
            <div className="mt-6 panel-card rounded-xl p-4">
              <p className="text-xs font-semibold uppercase tracking-wider text-[var(--color-text-secondary)] mb-3">
                Weitere Tabs live im Produkt
              </p>
              <div className="flex flex-wrap gap-2">
                {secondaryTabs.map((tab) => (
                  <span
                    key={tab.id}
                    className="rounded-full border border-[var(--color-border)] bg-[rgba(7,21,29,0.75)] px-3 py-1.5 text-xs text-[var(--color-text-secondary)]"
                  >
                    {tab.label}
                    {tab.beta && (
                      <span className="ml-1 text-[9px] uppercase text-[var(--color-accent)]">
                        Beta
                      </span>
                    )}
                  </span>
                ))}
              </div>
            </div>
          </div>
        </ScrollReveal>

        {/* Callout cards */}
        <ScrollReveal delay={0.25}>
          <div className="mt-12 grid grid-cols-1 md:grid-cols-3 gap-6">
            {callouts.map(({ Icon, title, description }, i) => (
              <motion.div
                key={title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true, margin: "-60px" }}
                transition={{ duration: 0.5, delay: i * 0.1, ease: "easeOut" }}
                className="panel-card rounded-xl p-6 soft-elevate"
              >
                <div className="w-10 h-10 rounded-lg gradient-accent flex items-center justify-center mb-4">
                  <Icon size={18} className="text-white" />
                </div>
                <h3 className="text-base font-semibold text-[var(--color-text-primary)] mb-2">
                  {title}
                </h3>
                <p className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
                  {description}
                </p>
              </motion.div>
            ))}
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}
