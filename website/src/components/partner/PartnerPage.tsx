import { motion } from "framer-motion";
import { ArrowRight, MessageCircle } from "lucide-react";
import {
  buildTwitchBotAuthUrl,
  DISCORD_INVITE_URL,
  TWITCH_SECURITY_URL,
} from "@/data/externalLinks";
import { PARTNER_COPY } from "@/data/partnerPage";
import { HERO_COPY } from "@/data/networkPage";
import { useNetworkMetrics } from "@/hooks/useNetworkMetrics";
import { PartnerFooter } from "@/components/partner/PartnerFooter";
import { PartnerNav } from "@/components/partner/PartnerNav";
import { PartnerValuesSection } from "@/components/partner/PartnerValues";
import { PartnerBanFeedSection } from "@/components/partner/PartnerBanFeed";
import { NetworkAmbient } from "@/components/v2/NetworkAmbient";
import { NetworkHero } from "@/components/v2/NetworkHero";
import { PartnersSection } from "@/components/v2/NetworkLive";
import { VoidSection } from "@/components/v2/NetworkStory";
import { NetworkSecuritySection } from "@/components/v2/NetworkSecurity";
import "./partner.css";

function Ctas() {
  return (
    <div className="flex flex-wrap items-center justify-center gap-4">
      <a
        href={buildTwitchBotAuthUrl()}
        className="gradient-accent inline-flex w-full items-center justify-center gap-2 rounded-xl px-6 py-3.5 font-semibold no-underline transition-all hover:brightness-110 hover:shadow-[0_0_28px_4px_rgba(201,168,106,0.28)] sm:w-auto"
      >
        {HERO_COPY.ctaPrimary}
        <ArrowRight size={18} />
      </a>
      <a
        href={DISCORD_INVITE_URL}
        target="_blank"
        rel="noopener noreferrer"
        className="inline-flex w-full items-center justify-center gap-2 rounded-xl border border-[rgba(255,255,255,0.14)] px-6 py-3.5 font-semibold text-[var(--color-text-primary)] no-underline transition-all hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] sm:w-auto"
      >
        <MessageCircle size={18} />
        {HERO_COPY.ctaSecondary}
      </a>
    </div>
  );
}

export function PartnerPage() {
  const metrics = useNetworkMetrics();

  return (
    <>
      <NetworkAmbient />
      <PartnerNav />
      <main className="relative">
        <NetworkHero metrics={metrics} />

        <PartnersSection
          partners={metrics.partnerList}
          liveNow={metrics.liveNow}
          total={metrics.partners}
          settled={metrics.settled}
          categoryKnown={metrics.categoryKnown}
        />

        <VoidSection />

        <PartnerValuesSection />

        <PartnerBanFeedSection />

        <NetworkSecuritySection />

        <section
          id="abschluss"
          className="relative mx-auto max-w-[84rem] px-6 py-24 text-center"
          style={{ scrollMarginTop: "5.5rem" }}
        >
          <motion.h2
            initial={{ opacity: 0, y: 18 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true, margin: "-80px" }}
            transition={{ duration: 0.55 }}
            className="mx-auto max-w-3xl text-3xl font-bold leading-[1.15] tracking-tight text-[var(--color-text-primary)] sm:text-4xl"
          >
            {PARTNER_COPY.closeHeadline}
          </motion.h2>

          <div className="mt-9">
            <Ctas />
          </div>

          <p className="mt-6 text-sm text-[var(--color-text-secondary)]">
            {PARTNER_COPY.closeNote}
          </p>
          <a
            href={TWITCH_SECURITY_URL}
            className="mt-3 inline-flex items-center gap-1.5 text-sm font-semibold text-[var(--color-accent)] no-underline hover:text-[var(--color-accent-hover)]"
          >
            {PARTNER_COPY.closeSafetyLink}
            <ArrowRight size={15} />
          </a>
        </section>
      </main>
      <PartnerFooter />
    </>
  );
}
