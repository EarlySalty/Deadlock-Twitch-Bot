import { motion } from "framer-motion";
import { ExternalLink } from "lucide-react";
import { GradientText } from "@/components/ui/GradientText";
import { RaidDemo } from "@/components/sections/RaidDemo";
import { useEinblendung } from "@/components/ui/ScrollReveal";
import { buildTwitchBotAuthUrl } from "@/data/externalLinks";
import { useNetworkCount } from "@/hooks/useNetworkCount";

export const LEITSAETZE = [
  "Kein zweiter Bot. Dein Anschluss ans deutsche Deadlock-Netz.",
  "Deutsche Deadlock-Streamer reichen sich ihre Zuschauer weiter. Du kannst dabei sein.",
  "Allein streamen ist die schwerste Variante. Hier reicht dich ein ganzes Netz weiter.",
] as const;

const VERTRAUENSZEILEN = [
  "Keine E-Mail, kein Passwort, keine Zahlungsdaten.",
  "Titel und Kanal-Einstellungen bleiben bei dir.",
  "Ein Klick bei Twitch und der Bot ist raus.",
];

function kopfUndKern(leitsatz: string): { kopf: string; kern: string } {
  const trenn = leitsatz.indexOf(". ");
  if (trenn === -1) return { kopf: leitsatz, kern: "" };
  return { kopf: leitsatz.slice(0, trenn + 1), kern: leitsatz.slice(trenn + 2) };
}

export function Hero() {
  const leitsatz = kopfUndKern(LEITSAETZE[0]);
  const badge = useEinblendung<HTMLDivElement>();
  const titel = useEinblendung<HTMLHeadingElement>();
  const text = useEinblendung<HTMLParagraphElement>();
  const cta = useEinblendung<HTMLDivElement>();
  const partnerzahl = useNetworkCount();

  return (
    <section
      id="hero"
      className="relative min-h-screen flex flex-col justify-center overflow-hidden"
    >
      <div className="max-w-[96rem] mx-auto px-6 pt-32 pb-20 w-full">
        <div className="text-center">
          <motion.div
            ref={badge.ref}
            initial={false}
            animate={{ opacity: badge.versteckt ? 0 : 1, y: badge.versteckt ? -12 : 0 }}
            transition={badge.versteckt ? { duration: 0 } : { duration: 0.5 }}
            className="inline-flex items-center rounded-full px-4 py-1.5 bg-[var(--color-card)] border border-[var(--color-border)] text-sm text-[var(--color-accent)]"
          >
            Das Netz der deutschen Deadlock-Streamer
          </motion.div>

          <motion.h1
            ref={titel.ref}
            initial={false}
            animate={{ opacity: titel.versteckt ? 0 : 1, y: titel.versteckt ? 20 : 0 }}
            transition={titel.versteckt ? { duration: 0 } : { duration: 0.6, delay: 0.1 }}
            className="mt-6 text-5xl md:text-6xl lg:text-7xl font-bold leading-tight text-[var(--color-text-primary)]"
          >
            {leitsatz.kopf}
            <br />
            <GradientText>{leitsatz.kern}</GradientText>
          </motion.h1>

          <motion.p
            ref={text.ref}
            initial={false}
            animate={{ opacity: text.versteckt ? 0 : 1, y: text.versteckt ? 20 : 0 }}
            transition={text.versteckt ? { duration: 0 } : { duration: 0.6, delay: 0.2 }}
            className="mt-6 text-xl text-[var(--color-text-secondary)] max-w-2xl mx-auto"
          >
            Endet dein Stream, reicht das Netz deine Zuschauer an den nächsten
            deutschen Deadlock-Stream weiter.
          </motion.p>
        </div>

        <div className="mt-16 mx-auto w-full max-w-[1400px]">
          <RaidDemo />
        </div>

        <motion.div
          ref={cta.ref}
          initial={false}
          animate={{ opacity: cta.versteckt ? 0 : 1, y: cta.versteckt ? 20 : 0 }}
          transition={cta.versteckt ? { duration: 0 } : { duration: 0.6, delay: 0.5 }}
          className="mt-10 text-center"
        >
          {partnerzahl != null ? (
            <p className="mb-5 text-sm font-medium uppercase tracking-wider text-[var(--color-accent)]">
              {partnerzahl} Partner sind im Netz
            </p>
          ) : null}
          <a
            href={buildTwitchBotAuthUrl()}
            className="gradient-accent rounded-xl px-8 py-4 font-semibold text-lg inline-flex items-center gap-2 transition-all duration-200 hover:brightness-110 hover:shadow-[0_0_24px_4px_rgba(201,168,106,0.3)]"
          >
            <ExternalLink size={20} />
            Partner werden
          </a>
          <ul className="mt-6 space-y-1.5">
            {VERTRAUENSZEILEN.map((zeile) => (
              <li
                key={zeile}
                className="text-sm text-[var(--color-text-secondary)]"
              >
                {zeile}
              </li>
            ))}
          </ul>
        </motion.div>
      </div>
    </section>
  );
}
