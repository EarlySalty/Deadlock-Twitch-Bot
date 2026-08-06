import { Footer } from "@/components/layout/Footer";
import { NetworkNav } from "@/components/v2/NetworkChrome";
import { NetworkHero } from "@/components/v2/NetworkHero";
import {
  PillarsSection,
  PlanSection,
  VoidSection,
} from "@/components/v2/NetworkStory";
import {
  ChannelReportSection,
  OpenMetricsSection,
} from "@/components/v2/NetworkProof";
import {
  NetworkCta,
  ObjectionsSection,
  PricingSection,
} from "@/components/v2/NetworkOffer";
import { useNetworkMetrics } from "@/hooks/useNetworkMetrics";

/**
 * Streamer-Landing V2 unter /streamer/v2/.
 *
 * Aufbau folgt dem Wireframe aus docs/strategie/31: Hero mit Beweiszeile,
 * das Problem, der Plan, die Leistungen, offene Zahlen, Lead-Magnet, Preise,
 * Einwaende, Abschluss. Die Metriken werden einmal oben geladen und nach
 * unten gereicht, damit Hero und Zahlen-Block denselben Stand zeigen.
 */
export function StreamerNetworkPage() {
  const metrics = useNetworkMetrics();

  return (
    <>
      <NetworkNav />
      <main>
        <NetworkHero metrics={metrics} />
        <VoidSection />
        <PlanSection />
        <PillarsSection />
        <OpenMetricsSection metrics={metrics} />
        <ChannelReportSection />
        <PricingSection />
        <ObjectionsSection />
        <NetworkCta />
      </main>
      {/* Gleiche Textkante wie die Abschnitte, damit der Hinweis nicht auf
          der Leitung klebt. */}
      <div className="mx-auto max-w-[84rem] px-6">
        <p className="pb-10 pl-6 text-xs text-[rgba(183,170,145,0.45)] sm:pl-12">
          Vorschau der neuen Streamer-Seite. Die aktive Seite liegt weiterhin
          unter /streamer/.
        </p>
      </div>
      <Footer />
    </>
  );
}
