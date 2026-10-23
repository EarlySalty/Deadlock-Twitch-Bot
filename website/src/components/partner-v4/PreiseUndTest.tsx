import { landingText, landingLink } from "@/lib/landingLocale";
import { ArrowRight, Check, MonitorPlay } from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import {
  TWITCH_DEMO_DASHBOARD_URL,
  TWITCH_PUBLIC_ORIGIN,
} from "@/data/externalLinks";

const PREISE_URL = `${TWITCH_PUBLIC_ORIGIN}/twitch/pricing`;

interface Stufe {
  id: string;
  name: string;
  preis: string;
  zusatz: string;
  punkte: string[];
  hervorgehoben?: boolean;
}

const stufen: Stufe[] = [
  {
    id: "free",
    name: "Netzwerk Free",
    preis: "0 €",
    zusatz: "Kein Abo, keine Laufzeit.",
    punkte: [
      "Auto-Raid in beide Richtungen",
      "Chat-Schutz und alle Befehle",
      "Overlay, Sendeplan, Tagesform",
      "Go-Live im Community-Discord",
    ],
    hervorgehoben: true,
  },
  {
    id: "plus",
    name: "Netzwerk Plus",
    preis: "4,99 € im Monat",
    zusatz: "Oder 49,90 € im Jahr. Im Monatsabo 30 Tage kostenlos testen.",
    punkte: [
      "Voller Verlauf, Vergleiche, KI-Auswertung",
      "Lurker-Erinnerung und Raid-Vorrang",
      "Community-Hinweise im Chat abschaltbar",
    ],
  },
  {
    id: "studio",
    name: "Studio",
    preis: "ab 12 € im Monat",
    zusatz: "Studio 12 €, Studio KI 15 €, Studio Schnitt 20 €.",
    punkte: [
      "Clips auf YouTube, TikTok und Instagram",
      "Auto-Pilot mit Freigabe oder Einspruch",
      "Je nach Version KI-Titel oder KI-Schnitt",
    ],
  },
];

export function PreiseUndTest() {
  return (
    <section id="preise" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge={landingText("Preise")}
          title={landingText("Was kostet das?")}
          subtitle={landingText(
            "Das Netzwerk bleibt kostenlos. Wer mehr will, bucht freiwillig dazu und kann vorher testen.",
          )}
        />

        <div className="mt-14 grid grid-cols-1 md:grid-cols-3 gap-6">
          {stufen.map((stufe, index) => (
            <ScrollReveal key={stufe.id} delay={index * 0.08}>
              <div
                className="panel-card rounded-2xl p-7 h-full soft-elevate"
                style={
                  stufe.hervorgehoben
                    ? { borderColor: "var(--color-primary)" }
                    : undefined
                }
              >
                <h3 className="text-base font-semibold text-[var(--color-text-primary)]">
                  {landingText(stufe.name)}
                </h3>
                <p className="mt-3 text-3xl font-bold font-display text-[var(--color-primary)]">
                  {landingText(stufe.preis)}
                </p>
                <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
                  {landingText(stufe.zusatz)}
                </p>
                <ul className="mt-5 space-y-2">
                  {stufe.punkte.map((punkt) => (
                    <li
                      key={punkt}
                      className="flex items-start gap-2 text-sm text-[var(--color-text-secondary)]"
                    >
                      <Check
                        size={16}
                        className="mt-0.5 shrink-0 text-[var(--color-accent)]"
                      />
                      {landingText(punkt)}
                    </li>
                  ))}
                </ul>
              </div>
            </ScrollReveal>
          ))}
        </div>

        <ScrollReveal className="mt-8">
          <div className="panel-card rounded-2xl p-6 md:p-7 flex flex-col md:flex-row items-start md:items-center justify-between gap-5">
            <div className="flex items-center gap-4">
              <span
                className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl"
                style={{
                  background:
                    "linear-gradient(135deg, rgba(201,168,106,0.20), rgba(85, 151, 143, 0.20))",
                  border: "1px solid var(--color-border)",
                }}
              >
                <MonitorPlay size={20} className="text-[var(--color-primary)]" />
              </span>
              <div>
                <p className="font-semibold text-[var(--color-text-primary)]">
                  {landingText("Erst mal reinschauen?")}
                </p>
                <p className="text-sm text-[var(--color-text-secondary)]">
                  {landingText(
                    "Das Demo-Dashboard zeigt alle Auswertungen mit Beispieldaten, ohne Anmeldung.",
                  )}
                </p>
              </div>
            </div>
            <a
              href={landingLink(TWITCH_DEMO_DASHBOARD_URL)}
              className="inline-flex items-center gap-2 rounded-xl px-5 py-2.5 text-sm font-medium text-[var(--color-text-primary)] transition-colors"
              style={{
                border: "1px solid var(--color-border)",
                background: "rgba(255,255,255,0.04)",
              }}
            >
              {landingText("Demo ansehen")}
              <ArrowRight size={15} />
            </a>
          </div>
        </ScrollReveal>

        <ScrollReveal className="mt-8 text-center">
          <a
            href={landingLink(PREISE_URL)}
            className="inline-flex items-center gap-2 text-[var(--color-accent)] transition-colors hover:text-[var(--color-accent-hover)]"
          >
            {landingText("Alle Stufen im Vergleich")}
            <ArrowRight size={16} />
          </a>
          <p className="mt-3 text-xs text-[var(--color-text-secondary)]">
            {landingText(
              "Alle Beträge sind Endpreise. Kleinunternehmer nach § 19 UStG, kein Umsatzsteuerausweis.",
            )}
          </p>
        </ScrollReveal>
      </div>
    </section>
  );
}
