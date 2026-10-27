import { landingText } from "@/lib/landingLocale";
import type { ComponentType } from "react";
import { Braces, MessagesSquare, Radio } from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { PUBLIC_API_BASE } from "@/data/streamerV4Fakten";

interface Plattform {
  id: string;
  icon: ComponentType<{ size?: number; className?: string }>;
  name: string;
  text: string;
}

const plattformen: Plattform[] = [
  {
    id: "twitch",
    icon: Radio,
    name: "Twitch",
    text: "Auto-Raids, Live-Erkennung, Chat-Moderation, Clips und Login. Der Zugang läuft über Twitch und ist dort jederzeit widerrufbar.",
  },
  {
    id: "discord",
    icon: MessagesSquare,
    name: "Discord",
    text: "Go-Live-Ankündigungen im Community-Discord und der Austausch mit den anderen Streamern im Netz.",
  },
];

const endpunkte = [
  { pfad: "/network", text: "Live-Partner im Netzwerk" },
  { pfad: "/network-stats", text: "Kennzahlen wie Partner, Raids und weitergereichte Zuschauer" },
  { pfad: "/recent-bans", text: "Zuletzt netzwerkweit gebannte Spam-Konten" },
];

export function Integrationen() {
  return (
    <section id="integrationen" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <SectionHeading
          badge={landingText("Integrationen")}
          title={landingText("Womit der Bot zusammenarbeitet")}
          subtitle={landingText(
            "Die Plattformen, an die das Netz angeschlossen ist, und die offenen Daten für alle, die selbst etwas bauen wollen.",
          )}
        />

        <div className="mt-14 grid grid-cols-1 md:grid-cols-2 gap-6">
          {plattformen.map((plattform, index) => {
            const Icon = plattform.icon;
            return (
              <ScrollReveal key={plattform.id} delay={index * 0.08}>
                <div className="panel-card rounded-2xl p-7 h-full soft-elevate">
                  <div className="flex items-center gap-3">
                    <Icon size={20} className="text-[var(--color-primary)]" />
                    <h3 className="text-base font-semibold text-[var(--color-text-primary)]">
                      {landingText(plattform.name)}
                    </h3>
                  </div>
                  <p className="mt-3 text-sm leading-relaxed text-[var(--color-text-secondary)]">
                    {landingText(plattform.text)}
                  </p>
                </div>
              </ScrollReveal>
            );
          })}
        </div>

        <ScrollReveal className="mt-6">
          <div className="panel-card rounded-2xl p-7">
            <div className="flex items-center gap-3">
              <Braces size={20} className="text-[var(--color-primary)]" />
              <h3 className="text-base font-semibold text-[var(--color-text-primary)]">
                {landingText("Öffentliche Schnittstelle")}
              </h3>
            </div>
            <p className="mt-3 text-sm leading-relaxed text-[var(--color-text-secondary)]">
              {landingText(
                "Rein lesend, JSON, ohne Schlüssel und ohne Anmeldung. Eine Auswahl, eine schreibende Entwickler-API bieten wir nicht an.",
              )}
            </p>
            <ul className="mt-5 space-y-3">
              {endpunkte.map((endpunkt) => (
                <li
                  key={endpunkt.pfad}
                  className="flex flex-col md:flex-row md:items-center gap-1 md:gap-4"
                >
                  <code className="text-xs md:text-sm break-all rounded-lg px-2.5 py-1 text-[var(--color-text-primary)] bg-white/[0.04] border border-[var(--color-border)]">
                    GET {PUBLIC_API_BASE}
                    {endpunkt.pfad}
                  </code>
                  <span className="text-sm text-[var(--color-text-secondary)]">
                    {landingText(endpunkt.text)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}
