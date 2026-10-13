import { ShieldCheck, KeyRound, PowerOff, Layers, ArrowRight, Lock } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { GradientText } from "@/components/ui/GradientText";
import { TWITCH_SECURITY_URL } from "@/data/externalLinks";
import { tx } from "@/i18n/locale";

const ACCESS_TOKEN_BLOB = "01027631cc1a318fc855cd096f414d4eb9140a1efaabbe7033619b7376eb2ade6399c5600e0bde49e5a9a314708da992a6e6c430146335861b9766631c795c9379b40f03da76d94ee642ec";
const REFRESH_TOKEN_BLOB = "010276315877678bb2ed322b62caed1893dda920643dafb46c11418b47095a2276fa41c0b95787b2fba008224c4d32173bfd6631bbca88554ed73a66aea84ff9de382af161cde3f65398480e";

const pillars = [
  {
    icon: ShieldCheck,
    title: tx("Keine Broadcaster-Rechte", "No broadcaster permissions"),
    body: tx(
      "Der Bot moderiert, mehr nicht. Streamtitel, Kategorie oder Kanal-Einstellungen kann er technisch gar nicht anfassen.",
      "The bot moderates, nothing more. It technically can't touch your stream title, category or channel settings.",
    ),
  },
  {
    icon: KeyRound,
    title: tx("Zugang einzeln verschlüsselt", "Access encrypted field by field"),
    body: tx(
      "Deine Zugangsdaten liegen feldweise mit AES-256-GCM verschlüsselt, der Schlüssel getrennt davon. Selbst ein Datenbank-Leak gibt sie nicht preis.",
      "Your credentials are encrypted field by field with AES-256-GCM, with the key stored separately. Even a database leak won't expose them.",
    ),
  },
  {
    icon: PowerOff,
    title: tx("Jederzeit widerrufbar", "Revoke any time"),
    body: tx(
      "Ein Klick in deinen Twitch-Einstellungen entzieht dem Bot den Zugriff. Er erkennt das und schaltet sich für deinen Kanal selbst ab.",
      "One click in your Twitch settings revokes the bot's access. It notices and switches itself off for your channel.",
    ),
  },
  {
    icon: Layers,
    title: tx("Schutz in mehreren Schichten", "Layered protection"),
    body: tx(
      "Doppelte Firewall, Zwei-Faktor auf allen Konten, verschlüsselter Secret-Tresor und rund-um-die-Uhr-Überwachung mit Sofort-Alarm.",
      "Double firewall, two-factor on every account, an encrypted secrets vault and 24/7 monitoring with instant alerts.",
    ),
  },
];

export function Security() {
  return (
    <section id="sicherheit" className="py-24 relative overflow-hidden">
      <div className="absolute inset-0 bg-gradient-to-b from-transparent via-[var(--color-primary)]/[0.03] to-transparent pointer-events-none" />

      <div className="max-w-7xl mx-auto px-6 relative z-10">
        <ScrollReveal className="text-center">
          <p className="text-sm uppercase tracking-wider font-medium text-[var(--color-primary)] mb-3">
            {tx(<>Sicherheit &amp; Vertrauen</>, <>Security &amp; trust</>)}
          </p>
          <h2 className="text-4xl md:text-5xl font-bold text-[var(--color-text-primary)] font-display">
            {tx(
              <>
                Du gibst dem Bot <GradientText>Mod-Rechte</GradientText>.
              </>,
              <>
                You&apos;re giving the bot <GradientText>mod rights</GradientText>.
              </>,
            )}
          </h2>
          <p className="text-lg text-[var(--color-text-secondary)] mt-4 max-w-2xl mx-auto">
            {tx(
              <>
                Fair, dass du genau weißt, was wir damit tun, und was wir bewusst
                nicht können. Hier ohne Marketing, einfach wie es ist.
              </>,
              <>
                It&apos;s only fair you know exactly what we do with them, and what we
                deliberately can&apos;t do. No marketing, just how it is.
              </>,
            )}
          </p>
        </ScrollReveal>

        <div className="mt-16 grid grid-cols-1 lg:grid-cols-2 gap-12 items-center">
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-5">
            {pillars.map((pillar, index) => {
              const Icon = pillar.icon;
              return (
                <ScrollReveal key={pillar.title} delay={index * 0.08}>
                  <div className="panel-card rounded-2xl p-6 h-full">
                    <div
                      className="w-11 h-11 rounded-xl flex items-center justify-center mb-4"
                      style={{
                        background:
                          "linear-gradient(135deg, rgba(201,168,106,0.20), rgba(85, 151, 143, 0.20))",
                        border: "1px solid var(--color-border)",
                      }}
                    >
                      <Icon size={20} className="text-[var(--color-primary)]" />
                    </div>
                    <h3 className="text-base font-semibold text-[var(--color-text-primary)] mb-2">
                      {pillar.title}
                    </h3>
                    <p className="text-sm text-[var(--color-text-secondary)] leading-relaxed">
                      {pillar.body}
                    </p>
                  </div>
                </ScrollReveal>
              );
            })}
          </div>

          <ScrollReveal direction="left" delay={0.1}>
            <div className="panel-card rounded-2xl p-6">
              <div className="flex items-center gap-2 mb-1">
                <Lock size={15} className="text-[var(--color-primary)]" />
                <span className="text-sm font-semibold text-[var(--color-text-primary)]">
                  {tx("So liegt dein Zugang bei uns", "How we store your access")}
                </span>
              </div>
              <p className="text-xs text-[var(--color-text-secondary)] mb-4">
                {tx(
                  <>
                    Ein echter Zugang sieht in unserer Datenbank so aus, verschlüsselt,
                    ohne unseren getrennt verwahrten Schlüssel reiner Datenmüll:
                  </>,
                  <>
                    This is what real access looks like in our database: encrypted, and
                    pure garbage without our separately stored key:
                  </>,
                )}
              </p>

              <div
                className="rounded-xl p-4 font-mono text-[12px] leading-relaxed break-all select-all space-y-2"
                style={{
                  background: "var(--theme-token-demo-bg, #1c150d)",
                  border: "1px solid var(--color-border)",
                  color: "var(--theme-token-demo-text, #55978f)",
                }}
              >
                <div>
                  <span className="text-[var(--color-text-secondary)]">access_token&nbsp;=&nbsp;</span>
                  {ACCESS_TOKEN_BLOB}
                </div>
                <div>
                  <span className="text-[var(--color-text-secondary)]">refresh_token&nbsp;=&nbsp;</span>
                  {REFRESH_TOKEN_BLOB}
                </div>
              </div>

              <div className="mt-4 flex items-start gap-2">
                <span className="relative flex h-2 w-2 mt-1.5 shrink-0">
                  <span className="animate-ping absolute h-full w-full rounded-full bg-[var(--color-success)] opacity-60" />
                  <span className="relative rounded-full h-2 w-2 bg-[var(--color-success)]" />
                </span>
                <p className="text-xs text-[var(--color-text-secondary)] leading-relaxed">
                  {tx(
                    <>
                      Format-Beispiel mit Wegwerf-Schlüssel, kein echter Zugang. Beim
                      Original ist jeder Wert einzeln verschlüsselt und fest an dein Konto
                      gebunden, sodass er sich nicht auf einen anderen Streamer übertragen lässt.
                    </>,
                    <>
                      Format example with a throwaway key, not real access. In the real
                      thing, every value is encrypted individually and bound to your
                      account, so it can&apos;t be transferred to another streamer.
                    </>,
                  )}
                </p>
              </div>
            </div>

            <div className="mt-6 flex flex-wrap gap-4 items-center">
              <a
                href={TWITCH_SECURITY_URL}
                className="gradient-accent rounded-xl px-7 py-3.5 font-semibold inline-flex items-center gap-2"
              >
                {tx("Ganzes Sicherheitskonzept lesen", "Read the full security concept")}
                <ArrowRight size={18} />
              </a>
              <span className="text-sm text-[var(--color-text-secondary)]">
                {tx(
                  "Zugangsdaten · Server · KI-Schutz · Notfall-Kontakt",
                  "Credentials · Servers · AI protection · Emergency contact",
                )}
              </span>
            </div>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}
