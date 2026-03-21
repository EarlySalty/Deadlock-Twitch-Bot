import { ArrowRight, CheckCircle2, LifeBuoy, Rocket, Sparkles } from "lucide-react";
import { motion } from "framer-motion";
import { GlowOrb } from "@/components/effects/GlowOrb";
import { PublicInfoFooter } from "@/components/layout/PublicInfoFooter";
import { PublicInfoHeader } from "@/components/layout/PublicInfoHeader";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { SectionHeading } from "@/components/ui/SectionHeading";
import {
  DISCORD_INVITE_URL,
  TWITCH_FAQ_URL,
  TWITCH_ONBOARDING_URL,
  buildTwitchBotAuthUrl,
} from "@/data/externalLinks";
import {
  ONBOARDING_CAPABILITIES,
  ONBOARDING_HIGHLIGHTS,
  ONBOARDING_STEPS,
  START_CHECKLIST,
} from "@/data/twitchKnowledgeBase";

const NAV_LINKS = [
  { label: "Ablauf", href: "#ablauf" },
  { label: "Features", href: "#module" },
  { label: "Erste Schritte", href: "#erste-schritte" },
  { label: "FAQ", href: "#faq-hinweis" },
];

export function StreamerOnboardingPage() {
  const onboardingAuthUrl = buildTwitchBotAuthUrl();

  return (
    <>
      <GlowOrb />
      <PublicInfoHeader
        navLinks={NAV_LINKS}
        primaryAction={{
          label: "Bot für deinen Kanal aktivieren",
          href: onboardingAuthUrl,
        }}
        secondaryAction={{ label: "Zur FAQ", href: TWITCH_FAQ_URL, variant: "ghost" }}
      />

      <main className="relative z-10">
        <section className="px-6 pb-16 pt-32">
          <div className="mx-auto grid max-w-7xl gap-10 lg:grid-cols-[1.2fr_0.8fr] lg:items-center">
            <div>
              <motion.div
                initial={{ opacity: 0, y: -12 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5 }}
                className="inline-flex rounded-full border border-border bg-[rgba(16,38,53,0.76)] px-4 py-1.5 text-sm text-accent"
              >
                Streamer Onboarding auf twitch.earlysalty.com
              </motion.div>

              <motion.h1
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.1 }}
                className="mt-6 max-w-4xl text-5xl font-bold leading-tight text-text-primary md:text-6xl lg:text-7xl"
              >
                Werde Teil des
                <br />
                <span className="bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
                  Deadlock-Partnernetzwerks
                </span>
                .
              </motion.h1>

              <motion.p
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.2 }}
                className="mt-6 max-w-2xl text-lg leading-relaxed text-text-secondary md:text-xl"
              >
                Aktiviere den Bot für deinen Kanal und vernetze dich mit anderen
                Deadlock-Streamern. Wenn du Deadlock streamst und offline gehst,
                kann der Bot deine Viewer automatisch an passende Partner
                weiterleiten. Und wenn andere offline gehen, kannst du genauso von
                ihren Raids profitieren.
              </motion.p>

              <motion.div
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.6, delay: 0.3 }}
                className="mt-10 flex flex-wrap gap-4"
              >
                <a
                  href={onboardingAuthUrl}
                  className="gradient-accent inline-flex items-center gap-2 rounded-xl px-7 py-3.5 font-semibold text-white no-underline transition-all duration-200 hover:brightness-110"
                >
                  Bot für deinen Kanal aktivieren
                  <ArrowRight size={18} />
                </a>
                <a
                  href={TWITCH_FAQ_URL}
                  className="inline-flex items-center gap-2 rounded-xl border border-border px-7 py-3.5 font-semibold text-text-primary no-underline transition-all duration-200 hover:border-border-hover hover:bg-white/5"
                >
                  Alle Bot-Funktionen ansehen
                </a>
                <p className="mt-4 text-sm leading-relaxed text-text-secondary">
                  Gilt nur für Deadlock-Streams. Manuelles Raiden bleibt wie
                  gewohnt, der Bot übernimmt nur den automatischen Raid beim
                  Offline-Gehen.
                </p>
              </motion.div>
            </div>

            <motion.aside
              initial={{ opacity: 0, y: 24 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.7, delay: 0.2 }}
              className="panel-card rounded-[2rem] p-7"
            >
              <div className="flex items-center gap-3 text-sm uppercase tracking-[0.16em] text-accent">
                <Rocket size={16} />
                Schnellüberblick
              </div>
              <div className="mt-6 space-y-4">
                {ONBOARDING_HIGHLIGHTS.map((highlight) => (
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
                  Kurzform für neue Streamer
                </p>
                <p className="mt-2 text-sm leading-relaxed text-text-secondary">
                  Kanal verbinden, ins Deadlock-Partnernetzwerk kommen, Auto-Raids
                  nutzen und danach dein Dashboard entdecken.
                </p>
              </div>
            </motion.aside>
          </div>
        </section>

        <section id="ablauf" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <SectionHeading
              badge="Ablauf"
              title="So startest du im Deadlock-Partnernetzwerk"
              subtitle="Erst verstehen, was aktiviert wird. Dann verbinden. Alles Weitere kommt danach."
            />

            <div className="mt-12 grid gap-5 lg:grid-cols-2">
              {ONBOARDING_STEPS.map((step, index) => (
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

                    <h3 className="mt-5 text-2xl font-bold text-text-primary">
                      {step.title}
                    </h3>
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

                    {step.routeHref && step.routeLabel ? (
                      <a
                        href={step.routeHref}
                        className="mt-6 inline-flex items-center gap-2 text-sm font-semibold text-text-primary no-underline transition-colors duration-200 hover:text-accent"
                      >
                        {step.routeLabel}
                        <ArrowRight size={16} />
                      </a>
                    ) : null}
                  </article>
                </ScrollReveal>
              ))}
            </div>
          </div>
        </section>

        <section id="module" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <SectionHeading
              badge="Features"
              title="Was du nach der Aktivierung nutzen kannst"
              subtitle="Der Auto-Raid ist der Einstieg. Danach kommen Dashboard, Community und weitere Tools dazu."
            />

            <div className="mt-12 grid gap-5 md:grid-cols-2 xl:grid-cols-3">
              {ONBOARDING_CAPABILITIES.map((capability, index) => (
                <ScrollReveal key={capability.title} delay={index * 0.04}>
                  <article className="panel-card h-full rounded-[1.75rem] p-6">
                    <div className="inline-flex rounded-full border border-border px-3 py-1 text-xs uppercase tracking-[0.14em] text-accent">
                      Feature {index + 1}
                    </div>
                    <h3 className="mt-5 text-2xl font-bold text-text-primary">
                      {capability.title}
                    </h3>
                    <p className="mt-3 text-sm leading-relaxed text-text-secondary">
                      {capability.description}
                    </p>
                    <ul className="mt-5 space-y-3">
                      {capability.bullets.map((bullet) => (
                        <li key={bullet} className="flex gap-3 text-sm leading-relaxed text-text-secondary">
                          <Sparkles size={16} className="mt-0.5 shrink-0 text-primary" />
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

        <section id="erste-schritte" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <SectionHeading
              badge="Erste Schritte"
              title="Die sinnvollste Reihenfolge für deinen Start"
              subtitle="Du musst nicht alles auf einmal nutzen. Diese Schritte bringen dich am schnellsten sauber ins Netzwerk."
            />

            <div className="mt-12 grid gap-5 xl:grid-cols-2">
              {START_CHECKLIST.map((item, index) => (
                <ScrollReveal key={item.title} delay={index * 0.05}>
                  <article className="panel-card flex h-full flex-col rounded-[1.75rem] p-6">
                    <div className="flex items-center gap-3">
                      <span className="flex h-10 w-10 items-center justify-center rounded-full border border-border bg-[rgba(11,29,40,0.82)] text-base font-semibold text-text-primary">
                        {index + 1}
                      </span>
                      <h3 className="text-xl font-bold text-text-primary">{item.title}</h3>
                    </div>

                    <p className="mt-4 flex-1 text-sm leading-relaxed text-text-secondary">
                      {item.description}
                    </p>

                    {item.href && item.label ? (
                      <a
                        href={item.href}
                        className="mt-6 inline-flex items-center gap-2 text-sm font-semibold text-text-primary no-underline transition-colors duration-200 hover:text-accent"
                      >
                        {item.label}
                        <ArrowRight size={16} />
                      </a>
                    ) : null}
                  </article>
                </ScrollReveal>
              ))}
            </div>
          </div>
        </section>

        <section id="faq-hinweis" className="px-6 py-20">
          <div className="mx-auto max-w-7xl">
            <div className="panel-card overflow-hidden rounded-[2rem] p-8 md:p-10">
              <div className="grid gap-8 lg:grid-cols-[1.1fr_0.9fr] lg:items-center">
                <div>
                  <p className="text-sm uppercase tracking-[0.16em] text-primary">
                    FAQ und Support
                  </p>
                  <h2 className="mt-4 text-4xl font-bold text-text-primary md:text-5xl">
                    In der FAQ findest du alle Details zum Bot und zum Netzwerk.
                  </h2>
                  <p className="mt-5 max-w-2xl text-base leading-relaxed text-text-secondary">
                    Wenn du nach dem Einstieg genauer verstehen willst, wie Auto-Raid,
                    Dashboard, Discord und die restlichen Funktionen zusammenspielen,
                    ist die FAQ die richtige Stelle dafür.
                  </p>
                </div>

                <div className="grid gap-4">
                  <a
                    href={TWITCH_FAQ_URL}
                    className="gradient-accent inline-flex items-center justify-between gap-4 rounded-2xl px-6 py-5 font-semibold text-white no-underline transition-all duration-200 hover:brightness-110"
                  >
                    <span>Komplette FAQ öffnen</span>
                    <ArrowRight size={18} />
                  </a>
                  <a
                    href={DISCORD_INVITE_URL}
                    className="inline-flex items-center justify-between gap-4 rounded-2xl border border-border px-6 py-5 font-semibold text-text-primary no-underline transition-colors duration-200 hover:border-border-hover hover:bg-white/5"
                  >
                    <span>Discord und Support</span>
                    <LifeBuoy size={18} />
                  </a>
                  <a
                    href={TWITCH_ONBOARDING_URL}
                    className="inline-flex items-center justify-between gap-4 rounded-2xl border border-border px-6 py-5 font-semibold text-text-primary no-underline transition-colors duration-200 hover:border-border-hover hover:bg-white/5"
                  >
                    <span>Onboarding erneut lesen</span>
                    <ArrowRight size={18} />
                  </a>
                </div>
              </div>
            </div>
          </div>
        </section>
      </main>

      <PublicInfoFooter />
    </>
  );
}
