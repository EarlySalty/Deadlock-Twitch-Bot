import { useState, type ComponentType } from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import {
  BarChart2,
  ChevronDown,
  Clapperboard,
  ShieldCheck,
  Swords,
  Users,
  Zap,
} from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";

interface Vorteil {
  id: string;
  icon: ComponentType<{ size?: number; className?: string }>;
  titel: string;
  beschreibung: string;
}

const vorteile: Vorteil[] = [
  {
    id: "auto-raid",
    icon: Swords,
    titel: "Auto-Raid im Netzwerk",
    beschreibung:
      "Endet dein Stream, landen deine Zuschauer beim passenden Live-Partner. Gehen andere offline, landen ihre bei dir.",
  },
  {
    id: "discord-live",
    icon: Zap,
    titel: "Sichtbar ab der ersten Minute",
    beschreibung:
      "Du gehst live, der Discord weiß es sofort. Verschluckt Twitch das Ereignis, fragt der Bot alle 15 Sekunden selbst nach.",
  },
  {
    id: "zahlen",
    icon: BarChart2,
    titel: "Deine Zahlen im Blick",
    beschreibung:
      "Zuschauer, Chat, Raids, Bestwerte und der Vergleich mit dem Netzwerk. Nach jedem Stream siehst du, was funktioniert hat.",
  },
  {
    id: "clips",
    icon: Clapperboard,
    titel: "Clips direkt aus dem Chat",
    beschreibung: "!clip im Chat, fertig ist der Clip.",
  },
  {
    id: "community",
    icon: Users,
    titel: "Eine Community im Rücken",
    beschreibung:
      "Treue Zuschauer werden belohnt, Lurker gezielt angesprochen. Im Streamer-Bereich im Discord tauschst du dich mit den anderen aus.",
  },
  {
    id: "schutz",
    icon: ShieldCheck,
    titel: "Ein Schutzschild für alle",
    beschreibung:
      "Scam- und Spam-Konten fliegen aus deinem Chat, bevor jemand sie sieht. Dazu die Ban-Liste des ganzen Netzwerks, alles pro Kanal abschaltbar.",
  },
];

export function NebenDeinenBots() {
  const reduce = useReducedMotion();
  const [ausgeklappt, setAusgeklappt] = useState(false);

  return (
    <section id="neben" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          title="Läuft neben deinen Bots"
          subtitle="Nichts wird ersetzt und kein bestehender Bot muss weichen. Der Anschluss gilt immer, weitergereicht wird nach Deadlock-Streams, ab und zu Deadlock reicht."
        />

        <ScrollReveal className="mt-12">
          <div className="flex flex-wrap justify-center gap-3">
            {vorteile.map((vorteil) => {
              const Icon = vorteil.icon;
              return (
                <span
                  key={vorteil.id}
                  className="inline-flex items-center gap-2 rounded-full border border-[var(--color-border)] bg-black/20 px-4 py-2 text-sm font-medium text-[var(--color-text-primary)]"
                >
                  <Icon size={15} className="text-[var(--color-accent)]" />
                  {vorteil.titel}
                </span>
              );
            })}
          </div>

          <div className="mt-6 text-center">
            <button
              type="button"
              onClick={() => setAusgeklappt((v) => !v)}
              aria-expanded={ausgeklappt}
              className="inline-flex items-center gap-2 rounded-xl border border-[var(--color-border)] bg-black/20 px-5 py-2.5 text-sm font-medium uppercase tracking-wider text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-border-hover)] hover:text-[var(--color-text-primary)]"
            >
              {ausgeklappt ? "Weniger anzeigen" : "Mehr anzeigen"}
              <ChevronDown
                size={16}
                className={`transition-transform ${ausgeklappt ? "rotate-180" : ""}`}
              />
            </button>
          </div>

          <AnimatePresence initial={false}>
            {ausgeklappt ? (
              <motion.div
                key="beschreibungen"
                initial={reduce ? false : { height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={reduce ? { opacity: 0 } : { height: 0, opacity: 0 }}
                transition={{ duration: reduce ? 0 : 0.3 }}
                className="overflow-hidden"
              >
                <div className="mt-8 grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-5">
                  {vorteile.map((vorteil) => {
                    const Icon = vorteil.icon;
                    return (
                      <div
                        key={vorteil.id}
                        className="panel-card rounded-2xl p-6 soft-elevate"
                      >
                        <div className="flex items-center gap-3">
                          <span
                            className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl"
                            style={{
                              background:
                                "linear-gradient(135deg, rgba(201,168,106,0.26), rgba(85, 151, 143, 0.34))",
                              border: "1px solid rgba(85, 151, 143, 0.45)",
                            }}
                          >
                            <Icon
                              size={17}
                              className="text-[var(--color-accent-hover)]"
                            />
                          </span>
                          <h3 className="text-base font-semibold text-[var(--color-text-primary)]">
                            {vorteil.titel}
                          </h3>
                        </div>
                        <p className="mt-3 text-sm leading-relaxed text-[var(--color-text-secondary)]">
                          {vorteil.beschreibung}
                        </p>
                      </div>
                    );
                  })}
                </div>
              </motion.div>
            ) : null}
          </AnimatePresence>
        </ScrollReveal>
      </div>
    </section>
  );
}
