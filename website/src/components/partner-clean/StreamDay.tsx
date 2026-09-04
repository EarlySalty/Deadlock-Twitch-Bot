import { ArrowUpRight, MessagesSquare, Radio, Rocket } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { SectionHeading } from "@/components/ui/SectionHeading";

const phases = [
  {
    icon: Radio,
    step: "01",
    title: "Du gehst live",
    intro: "Das Netzwerk merkt es von selbst. Du musst nirgendwo Bescheid sagen.",
    points: [
      "Dein Deadlock-Stream wird automatisch erkannt, rund um die Uhr.",
      "Du landest im Community-Discord, auf Wunsch mit Ping-Rolle.",
      "Hunderte Community-Mitglieder sehen dich ab der ersten Minute.",
    ],
  },
  {
    icon: MessagesSquare,
    step: "02",
    title: "Während du streamst",
    intro: "Das Netzwerk arbeitet im Hintergrund, damit du dich aufs Spiel konzentrierst.",
    points: [
      "Chat-Befehle wie !clip und !lurk laufen mit, einzeln abschaltbar.",
      "Ein KI-Wächter hält Scam-Konten aus deinem Chat, bevor jemand sie sieht.",
      "Zuschauer-, Chat- und Raid-Zahlen werden automatisch mitgeschrieben.",
    ],
  },
  {
    icon: Rocket,
    step: "03",
    title: "Dein Stream endet",
    intro: "Der Moment, für den es das Netzwerk gibt: Deine Viewer gehen nicht verloren.",
    points: [
      "Deine Viewer wandern zum passendsten Live-Partner im Netzwerk.",
      "Gehen andere Partner offline, landen ihre Raids genauso bei dir.",
      "Im Dashboard wartet die Auswertung: Bestwerte, Trends, Netzwerk-Vergleich.",
    ],
  },
];

export function StreamDay() {
  return (
    <section id="ablauf" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge="So funktioniert's"
          title="Was im Netzwerk für dich läuft"
          subtitle="Die deutsche Deadlock-Szene ist klein. Wer allein streamt, kämpft allein um jeden Viewer. Als Partner hängt dein Kanal an einem Netzwerk aus Streamern und einem aktiven Community-Discord: Zuschauer werden geteilt statt verloren."
        />

        <div className="mt-16 grid grid-cols-1 md:grid-cols-3 gap-6">
          {phases.map((phase, index) => {
            const Icon = phase.icon;
            return (
              <ScrollReveal key={phase.step} delay={index * 0.12}>
                <div className="panel-card rounded-2xl p-7 h-full flex flex-col">
                  <div className="flex items-center justify-between mb-5">
                    <div
                      className="w-12 h-12 rounded-xl flex items-center justify-center"
                      style={{
                        background:
                          "linear-gradient(135deg, rgba(201,168,106,0.26), rgba(85, 151, 143, 0.34))",
                        border: "1px solid rgba(85, 151, 143, 0.45)",
                      }}
                    >
                      <Icon size={20} className="text-[var(--color-accent-hover)]" />
                    </div>
                    <span className="text-sm font-semibold tracking-[0.2em] text-[var(--color-accent)] opacity-70">
                      {phase.step}
                    </span>
                  </div>

                  <h3 className="text-xl font-semibold text-[var(--color-text-primary)]">
                    {phase.title}
                  </h3>
                  <p className="text-sm text-[var(--color-text-secondary)] mt-2 mb-5 leading-relaxed">
                    {phase.intro}
                  </p>

                  <ul className="space-y-3 mt-auto">
                    {phase.points.map((point) => (
                      <li key={point} className="flex items-start gap-2.5">
                        <span
                          aria-hidden
                          className="mt-[0.55rem] w-1.5 h-1.5 rounded-full shrink-0 bg-[var(--color-accent)]"
                        />
                        <span className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
                          {point}
                        </span>
                      </li>
                    ))}
                  </ul>
                </div>
              </ScrollReveal>
            );
          })}
        </div>

        <ScrollReveal delay={0.3}>
          <div className="mt-10 flex flex-wrap justify-center gap-x-8 gap-y-3">
            <a
              href="/twitch/demo"
              className="inline-flex items-center gap-1.5 text-sm font-medium text-[var(--color-accent)] hover:text-[var(--color-accent-hover)] transition-colors duration-200"
            >
              Dashboard mit Demo-Daten ansehen
              <ArrowUpRight size={15} />
            </a>
            <a
              href="/streamer/vergleich/"
              className="inline-flex items-center gap-1.5 text-sm font-medium text-[var(--color-accent)] hover:text-[var(--color-accent-hover)] transition-colors duration-200"
            >
              Alle Funktionen im Vergleich
              <ArrowUpRight size={15} />
            </a>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}
