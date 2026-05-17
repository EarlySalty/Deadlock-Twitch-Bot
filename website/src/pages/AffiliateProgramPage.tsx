import {
  ArrowRight,
  CheckCircle2,
  CreditCard,
  Infinity,
  LayoutDashboard,
  LifeBuoy,
  LogIn,
  Percent,
  UserPlus,
} from "lucide-react";
import { motion } from "framer-motion";
import type { ComponentType } from "react";
import { GlowOrb } from "@/components/effects/GlowOrb";
import { AffiliateNavbar } from "@/components/layout/AffiliateNavbar";
import { Footer } from "@/components/layout/Footer";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { affiliateFeatures } from "@/data/affiliateFeatures";
import { DISCORD_INVITE_URL, TWITCH_AFFILIATE_URL } from "@/data/externalLinks";

const HERO_HIGHLIGHTS = [
  { label: "Provision", value: "30%" },
  { label: "Modell", value: "Dauerhaft" },
  { label: "Auszahlung", value: "Automatisch" },
];

const HOW_IT_WORKS_STEPS = [
  {
    eyebrow: "1. Mit Twitch anmelden",
    title: "Ein Klick, kein Formular",
    description:
      "Du meldest dich mit deinem Twitch-Account an und bist sofort registriert. Keine Adressdaten, kein Papierkram.",
    bullets: [
      "Mit deinem Twitch-Account anmelden und direkt loslegen.",
      "Sofort registriert ohne manuelles Formular.",
      "Keine Adressdaten und kein zusätzlicher Papierkram nötig.",
    ],
  },
  {
    eyebrow: "2. Stripe-Konto verbinden",
    title: "Auszahlungen sauber aufsetzen",
    description:
      "Im Portal klickst du auf \"Stripe-Konto verbinden\". Stripe ist der Zahlungsdienstleister, über den deine Provisionen automatisch auf dein Bankkonto kommen. Die Einrichtung dauert ca. 5 Minuten. Ohne Stripe werden Provisionen bis 50€ für dich gespeichert.",
    bullets: [
      "Stripe Connect einmalig mit deinem Bankkonto verknüpfen.",
      "Die Einrichtung dauert ungefähr 5 Minuten inklusive Verifizierung.",
      "Ohne Stripe werden Provisionen bis 50€ zwischengespeichert.",
    ],
  },
  {
    eyebrow: "3. Streamer finden und werben",
    title: "Direkt im Deadlock-Umfeld akquirieren",
    description:
      "Finde Deadlock-Streamer, die noch nicht bei Deutsche Deadlock Community sind, auf Twitch, in Discord-Servern oder in Deadlock-Communities. Empfiehl Deutsche Deadlock Community direkt oder teile deinen Referral-Link.",
    bullets: [
      "Deadlock-Streamer auf Twitch, Discord und in Communities identifizieren.",
      "Deutsche Deadlock Community direkt empfehlen oder deinen Referral-Link teilen.",
      "Nur neue Streamer bringen dir laufende Provisionen.",
    ],
  },
  {
    eyebrow: "4. Streamer im Portal beanspruchen",
    title: "Zuordnung sichern",
    description:
      "Gib den Twitch-Namen des Streamers im Portal ein und klicke \"Beanspruchen\". Damit wird er dir zugeordnet. Jeder Streamer kann nur von einem Vertriebler beansprucht werden: first come, first served.",
    bullets: [
      "Den Twitch-Login im Portal eintragen.",
      "Per Klick dem eigenen Account zuordnen.",
      "Jeder Streamer kann nur einmal beansprucht werden.",
    ],
  },
  {
    eyebrow: "5. Provision kassieren",
    title: "Automatisch mitverdienen",
    description:
      "Bei jeder Zahlung deines Streamers bekommst du automatisch 30%. Die Auszahlung läuft über Stripe direkt auf dein Konto. Im Portal siehst du in Echtzeit deine Verdienste, ausstehende Zahlungen und die History.",
    bullets: [
      "30% Provision auf jede Zahlung deiner geworbenen Streamer.",
      "Automatische Auszahlung über Stripe auf dein Bankkonto.",
      "Verdienste, offene Zahlungen und History im Portal live im Blick.",
    ],
  },
];

const AFFILIATE_FAQ = [
  {
    question: "Muss ich selbst Streamer sein?",
    answer:
      "Nein. Du kannst Deutsche Deadlock Community auch dann empfehlen, wenn du selbst nicht streamst. Entscheidend ist nur, dass du neue Deadlock-Streamer wirbst.",
  },
  {
    question: "Wie funktioniert die Auszahlung?",
    answer:
      "Die Auszahlung läuft über Stripe Connect. Sobald dein Stripe-Konto verbunden ist, werden Provisionen bei jeder Streamer-Zahlung automatisch auf dein Bankkonto ausgezahlt.",
  },
  {
    question: "Was passiert ohne Stripe-Konto?",
    answer:
      "Provisionen werden bis 50€ für dich gespeichert. Alles darüber hinaus verfällt, solange kein Stripe-Konto verbunden ist.",
  },
  {
    question: "Gibt es ein Limit?",
    answer:
      "Nein. Es gibt keine Obergrenze und keine zeitliche Begrenzung. Solange dein geworbener Streamer zahlt, verdienst du dauerhaft mit.",
  },
  {
    question: "Was wenn ein Streamer kündigt?",
    answer:
      "Dann endet die laufende Provision für diesen Streamer. Bereits ausgezahlte Provisionen bleiben natürlich bei dir.",
  },
  {
    question: "Muss ich Steuern zahlen?",
    answer:
      "Ja, du bist selbst für die Versteuerung deiner Einnahmen verantwortlich. Deutsche Deadlock Community übernimmt keine steuerliche Bewertung für dich.",
  },
];

const iconMap: Record<string, ComponentType<{ size?: number; className?: string }>> = {
  Percent,
  UserPlus,
  CreditCard,
  Infinity,
  LayoutDashboard,
  LogIn,
};

export default function AffiliateProgramPage() {
  return (
    <>
      <GlowOrb />
      <AffiliateNavbar />

      <main className="relative z-10">
        <section className="px-6 pb-16 pt-32">
          <div className="mx-auto grid max-w-7xl gap-10 lg:grid-cols-[1.15fr_0.85fr] lg:items-center">
            <div>
              <motion.div
                initial={{ opacity: 0, y: -12 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5 }}
                className="inline-flex rounded-full border border-border bg-[rgba(16,38,53,0.76)] px-4 py-1.5 text-sm text-accent"
              >
                Affiliate-Programm
              </motion.div>

              <motion.h1
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.1 }}
                className="mt-6 max-w-5xl text-5xl font-bold leading-tight text-text-primary md:text-6xl lg:text-7xl"
              >
                Verdiene 30% Provision
                <br />
                <span className="bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
                  dauerhaft, ohne Limit
                </span>
              </motion.h1>

              <motion.p
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.2 }}
                className="mt-6 max-w-3xl text-lg leading-relaxed text-text-secondary md:text-xl"
              >
                Werde Deutsche Deadlock Community-Vertriebler, wirb Deadlock-Streamer und verdiene
                bei jeder Zahlung mit. Anmeldung in 2 Minuten über deinen
                Twitch-Account.
              </motion.p>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.3 }}
                className="mt-10 flex flex-wrap gap-4"
              >
                <a
                  href={TWITCH_AFFILIATE_URL}
                  className="gradient-accent inline-flex items-center gap-2 rounded-xl px-7 py-3.5 font-semibold text-white no-underline transition-all duration-200 hover:brightness-110"
                >
                  Jetzt Vertriebler werden
                  <ArrowRight size={18} />
                </a>
                <a
                  href="#ablauf"
                  className="inline-flex items-center gap-2 rounded-xl border border-border px-7 py-3.5 font-semibold text-text-primary no-underline transition-all duration-200 hover:border-border-hover hover:bg-white/5"
                >
                  So funktioniert es
                </a>
              </motion.div>
            </div>

            <motion.aside
              initial={{ opacity: 0, y: 24 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.7, delay: 0.2 }}
              className="panel-card rounded-[2rem] p-7"
            >
              <div className="flex items-center gap-3 text-sm uppercase tracking-[0.16em] text-accent">
                <LifeBuoy size={16} />
                Schnellüberblick
              </div>

              <div className="mt-6 space-y-4">
                {HERO_HIGHLIGHTS.map((highlight) => (
                  <div
                    key={highlight.label}
                    className="rounded-2xl border border-border bg-[rgba(7,21,29,0.46)] p-4"
                  >
                    <p className="text-xs uppercase tracking-[0.14em] text-text-secondary">
                      {highlight.label}
                    </p>
                    <p className="mt-2 text-lg font-semibold text-text-primary">
                      {highlight.value}
                    </p>
                  </div>
                ))}
              </div>

              <div className="mt-6 rounded-2xl border border-border bg-[rgba(11,29,40,0.74)] p-4">
                <p className="text-sm font-semibold text-text-primary">
                  Start in wenigen Minuten
                </p>
                <p className="mt-2 text-sm leading-relaxed text-text-secondary">
                  Mit Twitch anmelden, Stripe verbinden, Streamer beanspruchen und
                  Provisionen automatisch aufbauen.
                </p>
              </div>
            </motion.aside>
          </div>
        </section>

        <section id="ablauf" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <SectionHeading
              badge="How it works"
              title="So läuft das Affiliate-Programm ab"
              subtitle="Vom Twitch-Login bis zur automatischen Auszahlung ist der Ablauf klar und ohne unnötige Zwischenschritte aufgebaut."
            />

            <div className="mt-12 grid gap-5 lg:grid-cols-2">
              {HOW_IT_WORKS_STEPS.map((step, index) => (
                <ScrollReveal key={step.title} delay={index * 0.05}>
                  <article className="panel-card h-full rounded-[1.75rem] p-6">
                    <div className="flex items-center justify-between gap-4">
                      <p className="text-sm uppercase tracking-[0.18em] text-primary">
                        {step.eyebrow}
                      </p>
                      <span className="rounded-full border border-border px-3 py-1 text-xs text-text-secondary">
                        Schritt {index + 1}
                      </span>
                    </div>

                    <h2 className="mt-5 text-2xl font-bold text-text-primary">
                      {step.title}
                    </h2>
                    <p className="mt-4 text-base leading-relaxed text-text-secondary">
                      {step.description}
                    </p>

                    <ul className="mt-5 space-y-3">
                      {step.bullets.map((bullet) => (
                        <li key={bullet} className="flex gap-3 text-sm leading-relaxed text-text-secondary">
                          <CheckCircle2 size={18} className="mt-0.5 shrink-0 text-accent" />
                          <span>{bullet}</span>
                        </li>
                      ))}
                    </ul>
                  </article>
                </ScrollReveal>
              ))}
            </div>
          </div>
        </section>

        <section id="features" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <SectionHeading
              badge="Vorteile"
              title="Was du als Vertriebler konkret bekommst"
              subtitle="Die Produktfläche ist auf laufende Provisionen, klare Zuordnung und automatische Auszahlung ausgelegt."
            />

            <div className="mt-12 grid gap-5 md:grid-cols-2 xl:grid-cols-3">
              {affiliateFeatures.map((feature, index) => {
                const Icon = iconMap[feature.icon];

                return (
                  <ScrollReveal key={feature.title} delay={index * 0.04}>
                    <article className="panel-card h-full rounded-[1.75rem] p-6">
                      <div className="flex h-12 w-12 items-center justify-center rounded-2xl gradient-accent">
                        {Icon ? (
                          <Icon size={22} className="text-white" />
                        ) : (
                          <span className="text-sm font-bold text-white">
                            {feature.icon[0]}
                          </span>
                        )}
                      </div>
                      <h2 className="mt-5 text-2xl font-bold text-text-primary">
                        {feature.title}
                      </h2>
                      <p className="mt-3 text-sm leading-relaxed text-text-secondary">
                        {feature.description}
                      </p>
                    </article>
                  </ScrollReveal>
                );
              })}
            </div>
          </div>
        </section>

        <section id="faq" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <SectionHeading
              badge="Affiliate-FAQ"
              title="Die wichtigsten Fragen direkt beantwortet"
              subtitle="Die Kernpunkte zu Anmeldung, Stripe, Limits und Steuern ohne Umweg durch das Portal."
            />

            <div className="mt-12 grid gap-5 md:grid-cols-2 xl:grid-cols-3">
              {AFFILIATE_FAQ.map((item, index) => (
                <ScrollReveal key={item.question} delay={index * 0.04}>
                  <article className="panel-card h-full rounded-[1.75rem] p-6">
                    <p className="inline-flex rounded-full border border-border px-3 py-1 text-xs uppercase tracking-[0.14em] text-accent">
                      Frage {index + 1}
                    </p>
                    <h2 className="mt-5 text-2xl font-bold text-text-primary">
                      {item.question}
                    </h2>
                    <p className="mt-4 text-sm leading-relaxed text-text-secondary">
                      {item.answer}
                    </p>
                  </article>
                </ScrollReveal>
              ))}
            </div>
          </div>
        </section>

        <section className="px-6 pb-24 pt-20">
          <div className="mx-auto max-w-7xl">
            <div className="panel-card overflow-hidden rounded-[2rem] p-8 md:p-10">
              <div className="grid gap-8 lg:grid-cols-[1.1fr_0.9fr] lg:items-center">
                <div>
                  <p className="text-sm uppercase tracking-[0.16em] text-primary">
                    Nächster Schritt
                  </p>
                  <h2 className="mt-4 text-4xl font-bold text-text-primary md:text-5xl">
                    Jetzt Vertriebler werden und die ersten Streamer sichern.
                  </h2>
                  <p className="mt-5 max-w-2xl text-base leading-relaxed text-text-secondary">
                    Der Einstieg läuft über deinen Twitch-Account. Für Fragen,
                    Austausch und Support kannst du direkt zusätzlich in den
                    Discord kommen.
                  </p>
                </div>

                <div className="grid gap-4">
                  <a
                    href={TWITCH_AFFILIATE_URL}
                    className="gradient-accent inline-flex items-center justify-between gap-4 rounded-2xl px-6 py-5 font-semibold text-white no-underline transition-all duration-200 hover:brightness-110"
                  >
                    <span>Jetzt Vertriebler werden</span>
                    <ArrowRight size={18} />
                  </a>
                  <a
                    href={DISCORD_INVITE_URL}
                    className="inline-flex items-center justify-between gap-4 rounded-2xl border border-border px-6 py-5 font-semibold text-text-primary no-underline transition-colors duration-200 hover:border-border-hover hover:bg-white/5"
                  >
                    <span>Discord beitreten</span>
                    <LifeBuoy size={18} />
                  </a>
                </div>
              </div>
            </div>
          </div>
        </section>
      </main>

      <Footer />
    </>
  );
}
