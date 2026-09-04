import { motion, useReducedMotion } from "framer-motion";
import { ExternalLink, Radio, ShieldCheck, Users } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { GradientText } from "@/components/ui/GradientText";
import { Avatar } from "@/components/partner-clean/partnerShared";
import { type NetworkStreamer } from "@/hooks/useNetworkStreamers";
import { twitchUrl } from "@/lib/partnerNetwork";
import { buildTwitchBotAuthUrl } from "@/data/externalLinks";

const anchors = [
  {
    icon: Radio,
    lead: "Du gehst live",
    rest: "das Netzwerk merkt es von selbst",
  },
  {
    icon: ShieldCheck,
    lead: "Du streamst",
    rest: "der Bot passt auf deinen Chat auf",
  },
  {
    icon: Users,
    lead: "Dein Stream endet",
    rest: "deine Zuschauer bleiben im Netzwerk",
  },
];

function NetworkPulse() {
  return (
    <svg
      viewBox="0 0 220 160"
      fill="none"
      className="h-full w-full"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      {[
        [40, 40],
        [180, 46],
        [110, 84],
        [46, 122],
        [176, 118],
      ].map(([x, y], i) => (
        <motion.line
          key={`l-${i}`}
          x1={110}
          y1={84}
          x2={x}
          y2={y}
          stroke="var(--color-accent)"
          strokeWidth="1.2"
          initial={{ opacity: 0.15 }}
          animate={{ opacity: [0.15, 0.6, 0.15] }}
          transition={{ duration: 2.4, repeat: Infinity, delay: i * 0.35, ease: "easeInOut" }}
        />
      ))}
      {[
        [40, 40, 5],
        [180, 46, 5],
        [46, 122, 5],
        [176, 118, 5],
      ].map(([x, y, r], i) => (
        <circle key={`n-${i}`} cx={x} cy={y} r={r} fill="var(--color-accent)" opacity={0.75} />
      ))}
      <motion.circle
        cx={110}
        cy={84}
        r={9}
        fill="var(--color-primary)"
        animate={{ opacity: [1, 0.6, 1], r: [9, 11, 9] }}
        transition={{ duration: 2.6, repeat: Infinity, ease: "easeInOut" }}
      />
    </svg>
  );
}

export function PartnerPitch({ streamers }: { streamers: NetworkStreamer[] }) {
  const reduce = useReducedMotion();
  const marquee = streamers.slice(0, 14);
  const loop = marquee.length > 0 ? [...marquee, ...marquee] : [];

  return (
    <section id="ablauf" className="py-24">
      <div className="max-w-7xl mx-auto px-6">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-14 items-center">
          <ScrollReveal>
            <div className="inline-flex items-center rounded-full px-4 py-1.5 bg-[var(--color-card)] border border-[var(--color-border)] text-sm text-[var(--color-accent)]">
              Deutsche Deadlock Community
            </div>

            <h2 className="mt-6 text-3xl md:text-4xl lg:text-5xl font-bold text-[var(--color-text-primary)] font-display leading-tight">
              Du wirst Partner.
              <br />
              <GradientText>Der Bot macht den Rest.</GradientText>
            </h2>

            <p className="mt-6 text-lg text-[var(--color-text-secondary)] leading-relaxed">
              Wir sind die deutsche Deadlock-Community: ein Netzwerk aus
              Streamern und ein aktiver Discord, in dem täglich gezockt wird.
              Kein Anbieter, kein Abo. Du kommst dazu, du gehörst dazu.
            </p>

            <p className="mt-4 text-base text-[var(--color-text-secondary)] leading-relaxed">
              Sobald du Partner bist, läuft der Rest von selbst. Der Bot schickt
              deine Zuschauer am Stream-Ende zum passenden Live-Partner, kündigt
              dich im Discord an, sobald du live gehst, hält Scam- und
              Spam-Konten aus deinem Chat, bringt Befehle wie !clip und !lurk mit
              und legt dir nach jedem Stream die Auswertung hin. Du richtest
              nichts ein und verwaltest nichts.
            </p>

            <ul className="mt-8 divide-y divide-[var(--color-border)] border-y border-[var(--color-border)]">
              {anchors.map((a) => {
                const Icon = a.icon;
                return (
                  <li key={a.lead} className="flex items-center gap-4 py-4">
                    <span
                      className="w-11 h-11 rounded-xl shrink-0 flex items-center justify-center"
                      style={{
                        background:
                          "linear-gradient(135deg, rgba(201,168,106,0.26), rgba(85, 151, 143, 0.34))",
                        border: "1px solid rgba(85, 151, 143, 0.45)",
                      }}
                    >
                      <Icon size={18} className="text-[var(--color-accent-hover)]" />
                    </span>
                    <span className="text-[var(--color-text-secondary)] leading-relaxed">
                      <strong className="text-[var(--color-text-primary)] font-semibold">
                        {a.lead}:
                      </strong>{" "}
                      {a.rest}
                    </span>
                  </li>
                );
              })}
            </ul>

            <div className="mt-8">
              <a
                href={buildTwitchBotAuthUrl()}
                className="gradient-accent rounded-xl px-7 py-3.5 font-semibold inline-flex items-center gap-2 transition-all duration-200 hover:brightness-110 hover:shadow-[0_0_24px_4px_rgba(201,168,106,0.3)]"
              >
                <ExternalLink size={18} />
                Jetzt Partner werden
              </a>
            </div>
          </ScrollReveal>

          <ScrollReveal delay={0.2}>
            <div className="panel-card rounded-2xl p-8 relative overflow-hidden">
              <div
                className="absolute inset-0 pointer-events-none"
                style={{
                  background:
                    "radial-gradient(70% 60% at 50% 30%, rgba(201,168,106,0.14), transparent 70%)",
                }}
              />
              <p className="relative text-sm uppercase tracking-wider font-medium text-[var(--color-primary)] mb-6">
                Dein Kanal hängt am Netzwerk
              </p>

              {loop.length > 0 ? (
                <div className="relative overflow-hidden py-4" style={{ maskImage: "linear-gradient(90deg, transparent, #000 12%, #000 88%, transparent)" }}>
                  <motion.div
                    className="flex gap-4 w-max"
                    animate={reduce ? undefined : { x: ["0%", "-50%"] }}
                    transition={{ duration: 26, repeat: Infinity, ease: "linear" }}
                  >
                    {loop.map((s, i) => (
                      <a
                        key={`${s.login}-${i}`}
                        href={twitchUrl(s.login)}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex flex-col items-center gap-2 no-underline group"
                      >
                        <span className="relative">
                          <Avatar login={s.login} avatarUrl={s.avatarUrl} size={56} />
                          {s.isLive ? (
                            <span className="v2-pulse absolute -bottom-0.5 -right-0.5 h-3 w-3 rounded-full bg-[var(--color-success)] ring-2 ring-[var(--color-card)]" />
                          ) : null}
                        </span>
                        <span className="text-xs text-[var(--color-text-secondary)] group-hover:text-[var(--color-primary)] max-w-[4.5rem] truncate">
                          {s.displayName ?? s.login}
                        </span>
                      </a>
                    ))}
                  </motion.div>
                </div>
              ) : (
                <div className="relative h-44">
                  <NetworkPulse />
                </div>
              )}

              <p className="relative mt-6 text-sm text-[var(--color-text-secondary)] leading-relaxed">
                Jeder Punkt ist ein Kanal, der schon dabei ist. Gehst du offline,
                landen deine Zuschauer beim nächsten von ihnen. Geht einer von
                ihnen offline, landen seine bei dir.
              </p>
            </div>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}
