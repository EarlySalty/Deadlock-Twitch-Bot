import { Footer } from "@/components/layout/Footer";
import { NetworkNav } from "@/components/v2/NetworkChrome";
import { NetworkHero } from "@/components/v2/NetworkHero";
import { PartnersSection } from "@/components/v2/NetworkLive";
import { NetworkSecuritySection } from "@/components/v2/NetworkSecurity";
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
 * Aufbau folgt dem Wireframe aus docs/strategie/31, in dieser Ordnung: Hero
 * mit Beweiszeile, das Problem, der Plan, die Leistungen, die Kontrolle ueber
 * den Bot, der Preis, danach erst der Beweis (Partner, offene Zahlen,
 * Lead-Magnet), zuletzt Einwaende und Abschluss.
 *
 * Der Beweis steht bewusst hinter dem Preis und nicht direkt unter dem Hero:
 * die Partnerkacheln zeigen laufende Twitch-Einbettungen und wuerden gleich
 * nach der Raid-Buehne ein zweites bewegtes Stream-Bild setzen, das dem Hero
 * die Wirkung nimmt. Die Metriken werden einmal oben geladen und nach unten
 * gereicht, damit Hero und Zahlen-Block denselben Stand zeigen.
 */
export function StreamerNetworkPage() {
  const metrics = useNetworkMetrics();

  return (
    <>
      <NetworkNav />
      {/* overflow-x-clip: die Lichtinseln ragen absichtlich ueber die
          Textkante hinaus und duerfen dabei keine Seitenscrollleiste erzeugen. */}
      <main className="relative overflow-x-clip">
        <NetworkHero metrics={metrics} />
        <VoidSection />
        <PlanSection />
        <PillarsSection />
        <NetworkSecuritySection />
        <PricingSection />
        <PartnersSection
          partners={metrics.partnerList}
          liveNow={metrics.liveNow}
          total={metrics.partners}
          settled={metrics.settled}
          categoryKnown={metrics.categoryKnown}
        />
        <OpenMetricsSection metrics={metrics} />
        <ChannelReportSection />
        <ObjectionsSection />
        <NetworkCta partners={metrics.partnerList} />
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
