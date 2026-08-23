import { useState } from "react";
import { motion } from "framer-motion";
import {
  ArrowRight,
  KeyRound,
  LogOut,
  ShieldCheck,
  ToggleRight,
  Users,
} from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import {
  TWITCH_DATENSCHUTZ_URL,
  TWITCH_SECURITY_URL,
} from "@/data/externalLinks";

interface Guard {
  icon: React.ReactNode;
  title: string;
  body: string;
}

/** Vier Zusagen, je ein Satz. Den Rest zeigt das Schaltpult daneben. */
const GUARDS: Guard[] = [
  {
    icon: <Users size={18} />,
    title: "Nur echte Partner",
    body: "Du wirst nur mit Kanälen der Community verbunden, nie mit fremden.",
  },
  {
    icon: <ToggleRight size={18} />,
    title: "Du entscheidest",
    body: "Jedes Feature schaltest du einzeln an und aus.",
  },
  {
    icon: <KeyRound size={18} />,
    title: "Nur die nötigen Rechte",
    body: "Wir fragen bei Twitch nur ab, was ein Feature wirklich braucht.",
  },
  {
    icon: <ShieldCheck size={18} />,
    title: "Raus in einer Minute",
    body: "Verbindung trennen ist ein Klick, ohne Mindestlaufzeit.",
  },
];

const SWITCH_ROWS = [
  { id: "raid-out", label: "Am Stream-Ende weitergeben", initial: true },
  { id: "raid-in", label: "Zuschauer von Partnern empfangen", initial: true },
  { id: "guard", label: "Schutz im Chat", initial: true },
  { id: "clips", label: "Clips automatisch schneiden", initial: false },
];

/**
 * Das Schaltpult ist der Beweis fuer den Satz daneben: die Schalter lassen
 * sich hier wirklich umlegen. Es ist eine Nachbildung des Dashboards, kein
 * Zugriff auf einen Kanal, deshalb steht das auch dran.
 */
function ControlPanel() {
  const [state, setState] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(SWITCH_ROWS.map((row) => [row.id, row.initial])),
  );

  return (
    <motion.div
      initial={{ opacity: 0, y: 22 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-70px" }}
      transition={{ duration: 0.55 }}
      className="panel-card overflow-hidden rounded-2xl"
    >
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-6 py-4">
        <span className="v2-stamp">Dein Dashboard</span>
        <span className="v2-stamp v2-stamp-dim">Beispielansicht, probier sie aus</span>
      </div>

      <div className="divide-y divide-[rgba(201,168,106,0.09)]">
        {SWITCH_ROWS.map((row) => (
          <button
            key={row.id}
            type="button"
            onClick={() =>
              setState((prev) => ({ ...prev, [row.id]: !prev[row.id] }))
            }
            aria-pressed={state[row.id]}
            className="flex w-full items-center justify-between gap-5 bg-transparent px-6 py-4 text-left transition-colors hover:bg-white/[0.03]"
          >
            <span className="text-sm text-[var(--color-text-primary)]">
              {row.label}
            </span>
            <span className="flex items-center gap-3">
              <span className="v2-stamp v2-stamp-dim w-8 text-right">
                {state[row.id] ? "an" : "aus"}
              </span>
              <span
                className={`v2-switch shrink-0 ${
                  state[row.id] ? "v2-switch-on" : ""
                }`}
                aria-hidden="true"
              />
            </span>
          </button>
        ))}
      </div>

      <div className="flex items-center justify-between gap-4 border-t border-[var(--color-border)] px-6 py-4">
        <span className="inline-flex items-center gap-2 text-sm text-[rgba(183,170,145,0.62)]">
          <LogOut size={15} />
          Verbindung trennen
        </span>
        <span className="v2-stamp v2-stamp-dim">jederzeit, ein Klick</span>
      </div>
    </motion.div>
  );
}

/**
 * Sicherheit und Vertrauen. Nicht als Feature-Liste, sondern als Bild: links
 * die Schalter, die dem Streamer gehoeren, rechts vier kurze Zusagen. Belege
 * verlinken auf die bestehenden Sicherheits- und Datenschutzseiten.
 */
export function NetworkSecuritySection() {
  return (
    <ProtocolSection
      id="sicherheit"
      stamp="04 · Vertrauen"
      ambient="teal"
      ambientSide="left"
      headline="Du behältst die Kontrolle. Immer."
      intro="Alles, was das Netzwerk auf deinem Kanal tut, hängt an einem Schalter, den du umlegst."
    >
      <div className="grid gap-6 lg:grid-cols-[1.05fr_0.95fr] lg:items-start">
        <ControlPanel />

        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-1">
          {GUARDS.map((guard, i) => (
            <motion.article
              key={guard.title}
              initial={{ opacity: 0, y: 18 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true, margin: "-70px" }}
              transition={{ duration: 0.45, delay: i * 0.08 }}
              className="panel-card v2-tile flex items-center gap-4 rounded-2xl px-5 py-4"
            >
              <span className="icon-tile flex h-10 w-10 shrink-0 items-center justify-center rounded-xl text-[var(--color-accent)]">
                {guard.icon}
              </span>
              <div className="min-w-0">
                <h3 className="font-bold text-[var(--color-text-primary)]">
                  {guard.title}
                </h3>
                <p className="mt-0.5 text-sm leading-relaxed text-[var(--color-text-secondary)]">
                  {guard.body}
                </p>
              </div>
            </motion.article>
          ))}
        </div>
      </div>

      <div className="mt-6 flex flex-wrap gap-x-7 gap-y-3">
        <a
          href={TWITCH_SECURITY_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center gap-1.5 text-sm font-semibold text-[var(--color-accent)] no-underline hover:text-[var(--color-accent-hover)]"
        >
          Wie wir mit deinem Konto umgehen
          <ArrowRight size={15} />
        </a>
        <a
          href={TWITCH_DATENSCHUTZ_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center gap-1.5 text-sm font-semibold text-[var(--color-accent)] no-underline hover:text-[var(--color-accent-hover)]"
        >
          Datenschutz
          <ArrowRight size={15} />
        </a>
      </div>
    </ProtocolSection>
  );
}
