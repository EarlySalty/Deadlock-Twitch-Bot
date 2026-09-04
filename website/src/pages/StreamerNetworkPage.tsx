import { Navbar } from "@/components/layout/Navbar";
import { Footer } from "@/components/layout/Footer";
import { SiteChatbot } from "@/components/layout/SiteChatbot";
import { GlowOrb } from "@/components/effects/GlowOrb";
import { Hero } from "@/components/partner-clean/Hero";
import { PartnerPitch } from "@/components/partner-clean/PartnerPitch";
import { PartnerNetwork } from "@/components/partner-clean/PartnerNetwork";
import { RaidExplainer } from "@/components/partner-clean/RaidExplainer";
import { BanFeed } from "@/components/partner-clean/BanFeed";
import { Stats } from "@/components/partner-clean/Stats";
import { Features } from "@/components/partner-clean/Features";
import { ClipManager } from "@/components/partner-clean/ClipManager";
import { Community } from "@/components/partner-clean/Community";
import { Security } from "@/components/partner-clean/Security";
import { CTA } from "@/components/partner-clean/CTA";
import { useNetworkStreamers } from "@/hooks/useNetworkStreamers";

export function StreamerNetworkPage() {
  const { streamers, status } = useNetworkStreamers();

  return (
    <>
      <GlowOrb />
      <Navbar />
      <main>
        <Hero />
        <PartnerPitch streamers={streamers} />
        <PartnerNetwork streamers={streamers} status={status} />
        <RaidExplainer />
        <BanFeed />
        <Stats streamers={streamers} status={status} />
        <Features />
        <ClipManager />
        <Community />
        <Security />
        <CTA />
      </main>
      <Footer />
      <SiteChatbot />
    </>
  );
}
