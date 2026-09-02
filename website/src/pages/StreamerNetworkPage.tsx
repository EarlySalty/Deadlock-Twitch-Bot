import { MotionConfig } from "framer-motion";
import { Footer } from "@/components/layout/Footer";
import { NetworkAmbient } from "@/components/v2/NetworkAmbient";
import { NetworkNav } from "@/components/v2/NetworkChrome";
import { NetworkHero } from "@/components/v2/NetworkHero";
import { PartnersSection } from "@/components/v2/NetworkLive";
import { NetworkSecuritySection } from "@/components/v2/NetworkSecurity";
import {
  PillarsSection,
  PlanSection,
  VoidSection,
} from "@/components/v2/NetworkStory";
import { OpenMetricsSection } from "@/components/v2/NetworkProof";
import {
  NetworkCta,
  ObjectionsSection,
  PricingSection,
} from "@/components/v2/NetworkOffer";
import { useNetworkMetrics } from "@/hooks/useNetworkMetrics";

export function StreamerNetworkPage() {
  const metrics = useNetworkMetrics();

  return (
    <MotionConfig reducedMotion="user">
      <NetworkNav />
      <NetworkAmbient />
      <main className="relative z-10 overflow-x-clip">
        <NetworkHero metrics={metrics} />
        <PartnersSection
          partners={metrics.partnerList}
          liveNow={metrics.liveNow}
          total={metrics.partners}
          settled={metrics.settled}
          categoryKnown={metrics.categoryKnown}
        />
        <VoidSection />
        <PlanSection />
        <NetworkSecuritySection />
        <OpenMetricsSection metrics={metrics} />
        <ObjectionsSection />
        <PillarsSection />
        <PricingSection />
        <NetworkCta partners={metrics.partnerList} />
      </main>
      <div className="relative z-10 mx-auto max-w-[84rem] px-6">
        <p className="pb-10 pl-6 text-xs text-[rgba(183,170,145,0.45)] sm:pl-12">
          Vorschau der neuen Streamer-Seite. Die aktive Seite liegt weiterhin
          unter /streamer/.
        </p>
      </div>
      <div className="relative z-10">
        <Footer />
      </div>
    </MotionConfig>
  );
}
