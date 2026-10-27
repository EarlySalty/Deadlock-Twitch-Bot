import { landingText } from "@/lib/landingLocale";
import { ChevronDown } from "lucide-react";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { HAEUFIGE_FRAGEN } from "@/data/streamerV4Fakten";

function strukturierteDaten(): string {
  return JSON.stringify({
    "@context": "https://schema.org",
    "@type": "FAQPage",
    mainEntity: HAEUFIGE_FRAGEN.map((eintrag) => ({
      "@type": "Question",
      name: landingText(eintrag.frage),
      acceptedAnswer: { "@type": "Answer", text: landingText(eintrag.antwort) },
    })),
  }).replace(/</g, "\\u003c");
}

export function HaeufigeFragen() {
  return (
    <section id="fragen" className="py-24">
      <div className="max-w-4xl mx-auto px-6">
        <SectionHeading
          badge={landingText("Häufige Fragen")}
          title={landingText("Kosten, Testphase, Ausstieg")}
          subtitle={landingText(
            "Die Antworten, die vor dem Beitritt am häufigsten fehlen.",
          )}
        />

        <div className="mt-12 space-y-3">
          {HAEUFIGE_FRAGEN.map((eintrag, index) => (
            <ScrollReveal key={eintrag.id} delay={index * 0.04}>
              <details
                id={`frage-${eintrag.id}`}
                className="panel-card group rounded-2xl px-6 py-5"
              >
                <summary className="flex cursor-pointer list-none items-center justify-between gap-4 text-base font-semibold text-[var(--color-text-primary)]">
                  {landingText(eintrag.frage)}
                  <ChevronDown
                    size={18}
                    className="shrink-0 text-[var(--color-primary)] transition-transform group-open:rotate-180"
                  />
                </summary>
                <p className="mt-3 text-sm leading-relaxed text-[var(--color-text-secondary)]">
                  {landingText(eintrag.antwort)}
                </p>
              </details>
            </ScrollReveal>
          ))}
        </div>
      </div>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: strukturierteDaten() }}
      />
    </section>
  );
}
