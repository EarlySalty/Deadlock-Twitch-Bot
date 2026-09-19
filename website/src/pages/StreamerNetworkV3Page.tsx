import { Navbar } from "@/components/layout/Navbar";
import { Footer } from "@/components/layout/Footer";
import { SiteChatbot } from "@/components/layout/SiteChatbot";
import { GlowOrb } from "@/components/effects/GlowOrb";
import { PartnerNetwork } from "@/components/partner-clean/PartnerNetwork";
import { PublicProfiles } from "@/components/partner-clean/PublicProfiles";
import { CTA } from "@/components/partner-clean/CTA";
import { Hero } from "@/components/partner-v3/Hero";
import { ZuschauerWanderung } from "@/components/partner-v3/ZuschauerWanderung";
import { NebenDeinenBots } from "@/components/partner-v3/NebenDeinenBots";
import { Vertrauen } from "@/components/partner-v3/Vertrauen";
import { CommunityPitch } from "@/components/partner-v3/CommunityPitch";
import { useNetworkStreamers } from "@/hooks/useNetworkStreamers";

export function StreamerNetworkV3Page() {
  const { streamers, status } = useNetworkStreamers();

  return (
    <>
      <GlowOrb />
      <Navbar />
      <main>
        <Hero />
        <PartnerNetwork streamers={streamers} status={status} />
        <ZuschauerWanderung />
        <NebenDeinenBots />
        <Vertrauen />
        <CommunityPitch />
        <CTA />
        <PublicProfiles />
      </main>
      <Footer />
      <SiteChatbot />
    </>
  );
}
