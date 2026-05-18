import { useDeferredValue, useState } from "react";
import { ArrowRight, Search } from "lucide-react";
import { GlowOrb } from "@/components/effects/GlowOrb";
import { PublicInfoFooter } from "@/components/layout/PublicInfoFooter";
import { PublicInfoHeader } from "@/components/layout/PublicInfoHeader";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { SectionHeading } from "@/components/ui/SectionHeading";
import {
  TWITCH_ONBOARDING_URL,
  buildTwitchBotAuthUrl,
} from "@/data/externalLinks";
import { FAQ_SECTIONS } from "@/data/twitchKnowledgeBase";

const NAV_LINKS = [
  { label: "Einstieg", href: "#einstieg" },
  { label: "Analytics", href: "#analytics" },
  { label: "Raids", href: "#raids" },
  { label: "Community", href: "#community" },
  { label: "Affiliate", href: "#affiliate" },
  { label: "Funktionen", href: "#funktionen" },
  { label: "Pläne", href: "#plaene" },
  { label: "Werbung", href: "#werbung" },
  { label: "Support", href: "#support" },
];

function countFaqItems() {
  return FAQ_SECTIONS.reduce((sum, section) => sum + section.items.length, 0);
}

function normalize(value: string) {
  return value.trim().toLowerCase();
}

export function BotFaqPage() {
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(normalize(query));
  const onboardingAuthUrl = buildTwitchBotAuthUrl();

  const visibleSections = FAQ_SECTIONS.map((section) => {
    if (!deferredQuery) {
      return section;
    }

    const sectionMatches = normalize(`${section.title} ${section.description} ${section.badge}`).includes(
      deferredQuery,
    );
    const items = section.items.filter((item) => {
      const haystack = normalize(
        [
          item.question,
          item.answer,
          item.access,
          item.tags.join(" "),
          item.details.join(" "),
          item.routes?.map((route) => route.label).join(" ") ?? "",
        ].join(" "),
      );
      return haystack.includes(deferredQuery);
    });

    if (sectionMatches) {
      return section;
    }

    return { ...section, items };
  }).filter((section) => section.items.length > 0);

  const totalItems = countFaqItems();

  return (
    <>
      <GlowOrb />
      <PublicInfoHeader
        navLinks={NAV_LINKS}
        primaryAction={{
          label: "Bot für deinen Kanal aktivieren",
          href: onboardingAuthUrl,
        }}
        secondaryAction={{
          label: "Zum Onboarding",
          href: TWITCH_ONBOARDING_URL,
          variant: "ghost",
        }}
      />

      <main className="relative z-10">
        <section className="px-6 pb-12 pt-32">
          <div className="mx-auto max-w-7xl">
            <div className="grid gap-8 lg:grid-cols-[1.1fr_0.9fr] lg:items-end">
              <div>
                <p className="inline-flex rounded-full border border-border bg-[rgba(16,38,53,0.76)] px-4 py-1.5 text-sm text-accent">
                  Häufige Fragen
                </p>
                <h1 className="mt-6 max-w-5xl text-5xl font-bold leading-tight text-text-primary md:text-6xl">
                  Antworten auf die
                  <br />
                  <span className="bg-gradient-to-r from-primary to-accent bg-clip-text text-transparent">
                    wichtigsten Fragen
                  </span>{" "}
                  rund um die Deutsche Deadlock Community.
                </h1>
                <p className="mt-6 max-w-3xl text-lg leading-relaxed text-text-secondary">
                  Hier findest du Antworten zu Einstieg, Analytics, Raids,
                  Discord-Automation und deinem Konto.
                </p>
              </div>

              <div className="panel-card rounded-[1.75rem] p-6">
                <p className="text-sm uppercase tracking-[0.16em] text-primary">
                  Wissensbasis
                </p>
                <div className="mt-5 grid grid-cols-2 gap-4">
                  <div className="rounded-2xl border border-border bg-[rgba(7,21,29,0.46)] p-4">
                    <p className="text-xs uppercase tracking-[0.14em] text-text-secondary">
                      Themenbereiche
                    </p>
                    <p className="mt-2 text-3xl font-bold text-text-primary">
                      {FAQ_SECTIONS.length}
                    </p>
                  </div>
                  <div className="rounded-2xl border border-border bg-[rgba(7,21,29,0.46)] p-4">
                    <p className="text-xs uppercase tracking-[0.14em] text-text-secondary">
                      Einzelfunktionen
                    </p>
                    <p className="mt-2 text-3xl font-bold text-text-primary">
                      {totalItems}
                    </p>
                  </div>
                </div>
              </div>
            </div>

            <div className="panel-card mt-10 rounded-[1.75rem] p-6">
              <label className="block text-sm font-semibold text-text-primary" htmlFor="faq-search">
                FAQ durchsuchen
              </label>
              <div className="mt-3 flex items-center gap-3 rounded-2xl border border-border bg-[rgba(7,21,29,0.52)] px-4 py-3">
                <Search size={18} className="shrink-0 text-text-secondary" />
                <input
                  id="faq-search"
                  type="text"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder="z. B. kosten, raid, dashboard, clips, kündigung"
                  className="w-full border-0 bg-transparent text-base text-text-primary outline-none placeholder:text-text-secondary"
                />
              </div>
              <p className="mt-3 text-sm text-text-secondary">
                Suche über Fragen, Antworten und Themen.
              </p>
            </div>
          </div>
        </section>

        <section className="px-6 pb-20 pt-8">
          <div className="mx-auto max-w-7xl">
            <SectionHeading
              badge="FAQ"
              title="Alle Themen"
              subtitle="Von Einstieg über Analytics bis Support — alle Antworten auf einen Blick."
            />

            <div className="mt-12 space-y-8">
              {visibleSections.map((section, sectionIndex) => (
                <ScrollReveal key={section.id} delay={sectionIndex * 0.03}>
                  <section id={section.id} className="panel-card rounded-[2rem] p-6 md:p-8">
                    <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
                      <div>
                        <p className="text-sm uppercase tracking-[0.16em] text-primary">
                          {section.badge}
                        </p>
                        <h2 className="mt-3 text-3xl font-bold text-text-primary md:text-4xl">
                          {section.title}
                        </h2>
                        <p className="mt-4 max-w-3xl text-base leading-relaxed text-text-secondary">
                          {section.description}
                        </p>
                      </div>
                      <span className="rounded-full border border-border px-4 py-2 text-sm text-text-secondary">
                        {section.items.length} Einträge
                      </span>
                    </div>

                    <div className="mt-8 grid gap-4">
                      {section.items.map((item) => (
                        <details key={`${section.id}-${item.question}`} className="faq-details panel-card rounded-[1.5rem] border border-border bg-[rgba(7,21,29,0.44)] p-5">
                          <summary className="faq-summary flex cursor-pointer list-none items-start justify-between gap-4">
                            <div>
                              <div className="mb-3 inline-flex rounded-full border border-border px-3 py-1 text-xs uppercase tracking-[0.14em] text-accent">
                                {item.access}
                              </div>
                              <h3 className="text-xl font-bold text-text-primary">
                                {item.question}
                              </h3>
                              <p className="mt-3 text-sm leading-relaxed text-text-secondary">
                                {item.answer}
                              </p>
                            </div>
                            <span className="shrink-0 rounded-full border border-border px-3 py-1 text-xs text-text-secondary">
                              Öffnen
                            </span>
                          </summary>

                          <div className="mt-5 border-t border-border pt-5">
                            <ul className="space-y-3">
                              {item.details.map((detail) => (
                                <li
                                  key={detail}
                                  className="flex gap-3 text-sm leading-relaxed text-text-secondary"
                                >
                                  <span className="mt-1.5 h-2 w-2 shrink-0 rounded-full bg-accent" />
                                  <span>{detail}</span>
                                </li>
                              ))}
                            </ul>

                            <div className="mt-5 flex flex-wrap gap-2">
                              {item.tags.map((tag) => (
                                <span
                                  key={tag}
                                  className="rounded-full border border-border px-3 py-1 text-xs text-text-secondary"
                                >
                                  {tag}
                                </span>
                              ))}
                            </div>

                            {item.routes?.length ? (
                              <div className="mt-5 flex flex-wrap gap-3">
                                {item.routes.map((route) => (
                                  <a
                                    key={route.href}
                                    href={route.href}
                                    className="inline-flex items-center gap-2 text-sm font-semibold text-text-primary no-underline transition-colors duration-200 hover:text-accent"
                                  >
                                    {route.label}
                                    <ArrowRight size={16} />
                                  </a>
                                ))}
                              </div>
                            ) : null}
                          </div>
                        </details>
                      ))}
                    </div>
                  </section>
                </ScrollReveal>
              ))}
            </div>

            {visibleSections.length === 0 ? (
              <div className="panel-card mt-8 rounded-[1.75rem] p-8 text-center">
                <p className="text-lg font-semibold text-text-primary">
                  Keine Einträge für "{query}" gefunden.
                </p>
                <p className="mt-3 text-sm text-text-secondary">
                  Probiere allgemeinere Begriffe wie dashboard, billing, raids,
                  clips, ai oder community.
                </p>
              </div>
            ) : null}

            <div className="panel-card mt-12 rounded-[2rem] p-8 md:p-10">
              <div className="grid gap-6 lg:grid-cols-[1fr_auto] lg:items-center">
                <div>
                  <p className="text-sm uppercase tracking-[0.16em] text-primary">
                    Weiter im Flow
                  </p>
                  <h2 className="mt-4 text-3xl font-bold text-text-primary md:text-4xl">
                    FAQ gelesen, jetzt den nächsten sinnvollen Schritt gehen.
                  </h2>
                  <p className="mt-4 max-w-2xl text-base leading-relaxed text-text-secondary">
                    Wenn du neu dabei bist, starte mit dem Onboarding und aktiviere
                    den Bot für deinen Kanal. Wenn dein Kanal schon verbunden ist,
                    ist das Dashboard der schnellste nächste Schritt.
                  </p>
                </div>

                <div className="flex flex-col gap-3 sm:flex-row lg:flex-col">
                  <a
                    href={TWITCH_ONBOARDING_URL}
                    className="inline-flex items-center justify-center gap-2 rounded-xl border border-border px-6 py-3 font-semibold text-text-primary no-underline transition-colors duration-200 hover:border-border-hover hover:bg-white/5"
                  >
                    Onboarding
                  </a>
                  <a
                    href={onboardingAuthUrl}
                    className="gradient-accent inline-flex items-center justify-center gap-2 rounded-xl px-6 py-3 font-semibold text-white no-underline transition-all duration-200 hover:brightness-110"
                  >
                    Bot aktivieren
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
