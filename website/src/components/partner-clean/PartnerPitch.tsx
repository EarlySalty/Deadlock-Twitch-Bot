import { motion, useReducedMotion } from "framer-motion";
import { ExternalLink, Radio, ShieldCheck, Users } from "lucide-react";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { GradientText } from "@/components/ui/GradientText";
import { Avatar } from "@/components/partner-clean/partnerShared";
import {
  type NetworkStatus,
  type NetworkStreamer,
} from "@/hooks/useNetworkStreamers";
import { istDeadlock, twitchUrl } from "@/lib/partnerNetwork";
import { buildTwitchBotAuthUrl } from "@/data/externalLinks";

const anchors = [
  {
    icon: Users,
    lead: "Du gehörst dazu",
    rest: "dein Kanal steht auf dieser Seite und im Discord neben allen anderen Partnern.",
  },
  {
    icon: ShieldCheck,
    lead: "Der Bot managt deinen Kanal",
    rest: "Raids, Live-Ankündigung, Chat-Schutz, Auswertung. Du richtest nichts ein.",
  },
  {
    icon: Radio,
    lead: "Kein Anbieter, kein Abo",
    rest: "eine Community, die ihre Streamer zusammenbringt.",
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

export function PartnerPitch({
  streamers,
  status,
}: {
  streamers: NetworkStreamer[];
  status: NetworkStatus;
}) {
  const reduce = useReducedMotion();
  const marquee = streamers.slice(0, 14);
  const loop = marquee.length > 0 ? [...marquee, ...marquee] : [];
  const platzText =
    status === "ready" && streamers.length > 0
      ? `Du wärst Partner Nr. ${streamers.length + 1}.`
      : "Die Partner siehst du unten auf der Seite.";

  return (
    <section id="ablauf" className="py-24">
      <div className="max-w-[1600px] mx-auto px-6">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-14 items-center">
          <ScrollReveal>
            <div className="inline-flex items-center rounded-full px-4 py-1.5 bg-[var(--color-card)] border border-[var(--color-border)] text-sm text-[var(--color-accent)]">
              Partner werden
            </div>

            <h2 className="mt-6 text-3xl md:text-4xl lg:text-5xl font-bold text-[var(--color-text-primary)] font-display leading-tight">
              Werde Partner der
              <br />
              <GradientText>Deutschen Deadlock Community.</GradientText>
            </h2>

            <p className="mt-6 text-xl md:text-2xl text-[var(--color-text-secondary)] leading-relaxed">
              Dein Kanal wird Teil der deutschen Deadlock-Community. Der Bot
              managt ihn ab dann komplett.
            </p>

            <p className="mt-10 text-sm uppercase tracking-wider font-medium text-[var(--color-primary)]">
              Was Partner heißt
            </p>

            <ul className="mt-4 divide-y divide-[var(--color-border)] border-y border-[var(--color-border)]">
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
            <div className="rounded-2xl p-8 relative overflow-hidden bg-[var(--color-card)] border border-[rgba(201,168,106,0.55)] shadow-[0_0_50px_-18px_rgba(201,168,106,0.85)]">
              <div
                className="absolute inset-0 pointer-events-none"
                style={{
                  background:
                    "radial-gradient(70% 60% at 50% 30%, rgba(201,168,106,0.16), transparent 70%)",
                }}
              />
              <p className="relative text-sm uppercase tracking-wider font-medium text-[var(--color-primary)] mb-5">
                Dein Platz im Netzwerk
              </p>

              <div className="relative flex items-center gap-3.5 rounded-xl border border-[rgba(201,168,106,0.55)] bg-black/25 px-4 py-3.5">
                <span
                  className="flex h-12 w-12 shrink-0 items-center justify-center rounded-full text-lg font-bold text-black/85"
                  style={{
                    background:
                      "linear-gradient(135deg, var(--color-accent), var(--color-primary))",
                  }}
                  aria-hidden="true"
                >
                  DU
                </span>
                <span className="min-w-0 flex-1 truncate text-lg font-semibold text-[var(--color-text-primary)]">
                  dein_kanal
                </span>
                <span className="shrink-0 rounded bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-bold uppercase tracking-wider text-black/85">
                  Partner
                </span>
              </div>

              {loop.length > 0 ? (
                <div className="relative overflow-hidden py-4 mt-5" style={{ maskImage: "linear-gradient(90deg, transparent, #000 12%, #000 88%, transparent)" }}>
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
                          {istDeadlock(s) && s.isLive ? (
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
                <div className="relative h-44 mt-5">
                  <NetworkPulse />
                </div>
              )}

              <p className="relative mt-6 text-sm text-[var(--color-text-secondary)] leading-relaxed">
                {platzText}
              </p>
            </div>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}
