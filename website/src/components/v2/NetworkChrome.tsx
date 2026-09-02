import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { motion } from "framer-motion";
import { GradientText } from "@/components/ui/GradientText";
import { buildTwitchBotAuthUrl } from "@/data/externalLinks";

const NAV_ITEMS = [
  { href: "#partner", label: "Partner" },
  { href: "#leere", label: "Das Problem" },
  { href: "#ablauf", label: "Ablauf" },
  { href: "#leistungen", label: "Leistungen" },
  { href: "#zahlen", label: "Zahlen" },
  { href: "#preise", label: "Preise" },
  { href: "#einwaende", label: "Fragen" },
];

/**
 * Kopfleiste der Landing V2. Bewusst schmaler als die produktive Navbar:
 * Wortmarke links, Ankerpunkte mittig, ein einziger goldener Knopf rechts.
 */
export function NetworkNav() {
  const [scrolled, setScrolled] = useState(false);

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 24);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <header
      className={`fixed inset-x-0 top-0 z-50 transition-all duration-300 ${
        scrolled ? "glass-bar py-3" : "py-5"
      }`}
    >
      <div className="mx-auto flex max-w-[84rem] items-center gap-6 px-6">
        <a href="/streamer/v2/" className="flex items-center gap-2.5 no-underline">
          <img
            src="/brand/logo/logo-192.png"
            alt=""
            width={30}
            height={30}
            className="rounded-md"
          />
          <span className="hidden flex-col leading-tight sm:flex">
            <GradientText className="font-semibold tracking-tight">
              Deutsche Deadlock Community
            </GradientText>
            <span className="text-[0.7rem] font-medium tracking-wide text-[var(--color-text-secondary)]">
              Streamer-Netzwerk
            </span>
          </span>
        </a>

        <nav className="ml-auto hidden items-center gap-7 lg:flex">
          {NAV_ITEMS.map((item) => (
            <a
              key={item.href}
              href={item.href}
              className="text-sm text-[var(--color-text-secondary)] no-underline transition-colors hover:text-[var(--color-primary)]"
            >
              {item.label}
            </a>
          ))}
        </nav>

        <a
          href={buildTwitchBotAuthUrl()}
          className="gradient-accent ml-auto rounded-lg px-4 py-2 text-sm font-semibold no-underline transition-all hover:brightness-110 lg:ml-0"
        >
          Kostenlos verbinden
        </a>
      </div>
    </header>
  );
}

interface ProtocolSectionProps {
  id: string;
  stamp: string;
  headline: ReactNode;
  intro?: ReactNode;
  /** Farbe der Lichtinsel hinter dem Abschnitt. "none" laesst sie weg. */
  ambient?: "gold" | "teal" | "none";
  /** Seite, an der die Lichtinsel sitzt. Abwechselnd gesetzt ergibt Rhythmus. */
  ambientSide?: "left" | "right";
  children: ReactNode;
}

/**
 * Sektions-Rahmen der Seite. Links laeuft eine durchgehende Leitung mit einem
 * Knoten pro Abschnitt, darueber steht eine Protokoll-Marke. Das ist die
 * gestalterische Klammer der Seite: alles ist ein Signalweg.
 */
export function ProtocolSection({
  id,
  stamp,
  headline,
  intro,
  ambient = "gold",
  ambientSide = "right",
  children,
}: ProtocolSectionProps) {
  return (
    <section
      id={id}
      className="relative mx-auto max-w-[84rem] px-6"
      // Ankersprung darf nicht unter der festen Kopfleiste landen.
      style={{ scrollMarginTop: "5.5rem" }}
    >
      {ambient === "none" ? null : (
        <div
          className={`v2-ambient ${
            ambient === "teal" ? "v2-ambient-teal" : "v2-ambient-gold"
          }`}
          style={{
            top: "6%",
            left: ambientSide === "left" ? "-8%" : undefined,
            right: ambientSide === "right" ? "-8%" : undefined,
            width: "min(42rem, 70vw)",
            height: "min(42rem, 70vw)",
          }}
          aria-hidden="true"
        />
      )}
      <div className="relative pl-6 sm:pl-12">
        <div className="v2-rail" aria-hidden="true" />
        <div
          className="v2-node v2-node-live"
          style={{ top: "0.55rem" }}
          aria-hidden="true"
        />

        <div className="py-16 sm:py-20">
          <motion.div
            initial={{ opacity: 0, y: 18 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true, margin: "-90px" }}
            transition={{ duration: 0.55, ease: "easeOut" }}
          >
            <p className="v2-stamp">{stamp}</p>
            <h2 className="mt-4 max-w-3xl text-3xl font-bold leading-[1.1] tracking-tight text-[var(--color-text-primary)] sm:text-4xl md:text-5xl">
              {headline}
            </h2>
            {intro ? (
              <p className="mt-5 max-w-2xl text-lg leading-relaxed text-[var(--color-text-secondary)]">
                {intro}
              </p>
            ) : null}
            <div className="v2-hairline mt-9 max-w-xl" />
          </motion.div>

          <div className="mt-12">{children}</div>
        </div>
      </div>
    </section>
  );
}
