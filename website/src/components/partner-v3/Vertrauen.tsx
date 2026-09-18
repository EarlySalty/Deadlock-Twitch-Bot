import type { ComponentType, ReactNode } from "react";
import { ArrowRight, KeyRound, Lock, PowerOff, ShieldCheck } from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import {
  TWITCH_PUBLIC_ORIGIN,
  TWITCH_SECURITY_URL,
} from "@/data/externalLinks";

const VERWALTUNG_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/verwaltung`;

interface Karte {
  id: string;
  icon: ComponentType<{ size?: number; className?: string }>;
  titel: string;
  inhalt: ReactNode;
}

const karten: Karte[] = [
  {
    id: "zugang-darf",
    icon: ShieldCheck,
    titel: "Was der Bot mit dem Partner-Zugang darf",
    inhalt: (
      <ul className="mt-3 space-y-1.5">
        {[
          "Raiden",
          "Moderieren",
          "Im Chat schreiben",
          "Clips erstellen",
          "Kanalpunkt-Einlösungen lesen",
          "Bits und Werbezeiten lesen",
        ].map((zeile) => (
          <li
            key={zeile}
            className="flex items-start gap-2 text-sm text-[var(--color-text-secondary)]"
          >
            <span className="mt-2 h-1 w-1 shrink-0 rounded-full bg-[var(--color-accent)]" />
            {zeile}
          </li>
        ))}
      </ul>
    ),
  },
  {
    id: "zugang-nicht",
    icon: Lock,
    titel: "Was er mit diesem Zugang nicht kann",
    inhalt: (
      <p className="mt-3 text-sm leading-relaxed text-[var(--color-text-secondary)]">
        Streamtitel, Kategorie und Kanal-Einstellungen ändern. Das gilt für
        diesen Zugang: Zusatzrechte wie Titel oder Uplink holt sich niemand
        automatisch. Sie kommen nur über einen eigenen Knopf in der Verwaltung,
        den du selbst drückst.
      </p>
    ),
  },
  {
    id: "nicht-haben",
    icon: KeyRound,
    titel: "Was wir nicht haben",
    inhalt: (
      <p className="mt-3 text-sm leading-relaxed text-[var(--color-text-secondary)]">
        Keine E-Mail-Adresse, kein Passwort, keine Zahlungsdaten. Der Login
        läuft bei Twitch, mehr als deinen Kanal sehen wir dadurch nicht.
      </p>
    ),
  },
  {
    id: "rauskommen",
    icon: PowerOff,
    titel: "Wie du rauskommst",
    inhalt: (
      <>
        <p className="mt-3 text-sm leading-relaxed text-[var(--color-text-secondary)]">
          Ein Klick in deinen Twitch-Einstellungen unter Verbindungen genügt.
          Der Bot merkt es und schaltet sich für deinen Kanal ab. Denselben Weg
          gibt es hier selbstbedient:
        </p>
        <a
          href={VERWALTUNG_URL}
          className="mt-3 inline-flex items-center gap-1.5 text-sm font-medium text-[var(--color-accent)] transition-colors hover:text-[var(--color-accent-hover)]"
        >
          Zur Selbstverwaltung für deinen Kanal
          <ArrowRight size={15} />
        </a>
      </>
    ),
  },
];

export function Vertrauen() {
  return (
    <section id="sicherheit" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge="Klartext"
          title="Vertrauen in Klartext"
          subtitle="Was der Zugang darf, was er nicht kann, was bei uns liegt und wie du wieder rauskommst."
        />

        <div className="mt-14 grid grid-cols-1 md:grid-cols-2 gap-6">
          {karten.map((karte, index) => {
            const Icon = karte.icon;
            return (
              <ScrollReveal key={karte.id} delay={index * 0.08}>
                <div className="panel-card rounded-2xl p-7 h-full soft-elevate">
                  <div className="flex items-center gap-3">
                    <span
                      className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl"
                      style={{
                        background:
                          "linear-gradient(135deg, rgba(201,168,106,0.20), rgba(85, 151, 143, 0.20))",
                        border: "1px solid var(--color-border)",
                      }}
                    >
                      <Icon size={20} className="text-[var(--color-primary)]" />
                    </span>
                    <h3 className="text-base font-semibold text-[var(--color-text-primary)]">
                      {karte.titel}
                    </h3>
                  </div>
                  {karte.inhalt}
                </div>
              </ScrollReveal>
            );
          })}
        </div>

        <ScrollReveal className="mt-10 text-center">
          <a
            href={TWITCH_SECURITY_URL}
            className="inline-flex items-center gap-2 text-[var(--color-accent)] transition-colors hover:text-[var(--color-accent-hover)]"
          >
            Ganzes Sicherheitskonzept lesen
            <ArrowRight size={16} />
          </a>
        </ScrollReveal>
      </div>
    </section>
  );
}
