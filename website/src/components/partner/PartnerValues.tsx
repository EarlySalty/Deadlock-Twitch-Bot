import { motion } from "framer-motion";
import { Clapperboard, LineChart, ShieldCheck, Swords } from "lucide-react";
import { ProtocolSection } from "@/components/v2/NetworkChrome";
import { PillarVisual } from "@/components/v2/NetworkPillarVisuals";
import { networkValues, VALUES_COPY } from "@/data/networkPage";
import type { NetworkValue } from "@/data/networkPage";

const VALUE_ICONS = {
  raids: Swords,
  schutz: ShieldCheck,
  coaching: LineChart,
  clips: Clapperboard,
} as const;

function ValueCard({ value, index }: { value: NetworkValue; index: number }) {
  const Icon = VALUE_ICONS[value.id];
  const accent =
    value.tone === "primary" ? "var(--color-primary)" : "var(--color-accent)";

  return (
    <motion.article
      initial={{ opacity: 0, y: 24 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-70px" }}
      transition={{ duration: 0.55, delay: (index % 2) * 0.1 }}
      className="v2-tile panel-card soft-elevate overflow-hidden rounded-2xl"
    >
      <PillarVisual id={value.id} />

      <div className="p-8">
        <div className="flex items-center gap-4">
          <span
            className="icon-tile flex h-11 w-11 items-center justify-center rounded-xl"
            style={{ color: accent }}
          >
            <Icon size={20} />
          </span>
          <div>
            <p className="v2-stamp" style={{ color: accent, opacity: 0.85 }}>
              {value.kicker}
            </p>
            <h3 className="text-xl font-bold text-[var(--color-text-primary)]">
              {value.title}
            </h3>
          </div>
        </div>

        <p className="mt-5 leading-relaxed text-[var(--color-text-secondary)]">
          {value.body}
        </p>
      </div>
    </motion.article>
  );
}

export function PartnerValuesSection() {
  return (
    <ProtocolSection
      id="netzwerk"
      ambientSide="left"
      stamp={VALUES_COPY.stamp}
      headline={VALUES_COPY.headline}
      intro={VALUES_COPY.intro}
    >
      <div className="grid gap-6 lg:grid-cols-2">
        {networkValues.map((value, i) => (
          <ValueCard key={value.id} value={value} index={i} />
        ))}
      </div>
    </ProtocolSection>
  );
}
