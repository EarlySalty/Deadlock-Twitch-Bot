import { AnimatedCounter } from "@/components/ui/AnimatedCounter";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { useNetworkCount } from "@/hooks/useNetworkCount";

interface PartnerStat {
  label: string;
  value: number;
  suffix: string;
}

const partnerStats: PartnerStat[] = [
  { label: "Streamer", value: 30, suffix: "+" },
  { label: "Bausteine", value: 7, suffix: "" },
  { label: "Analytics-Tabs", value: 13, suffix: "" },
  { label: "Polling", value: 15, suffix: "s" },
  { label: "Online", value: 24, suffix: "/7" },
];

export function Stats() {
  const streamerCount = useNetworkCount();

  return (
    <section id="stats" className="relative z-10">
      <div className="max-w-5xl mx-auto px-6 -mt-8 relative z-10">
        <ScrollReveal>
          <div className="panel-card rounded-2xl p-8">
            <div className="grid grid-cols-2 md:grid-cols-5 gap-8">
              {partnerStats.map((stat) => {
                const live = stat.label === "Streamer" && streamerCount != null;
                return (
                  <AnimatedCounter
                    key={stat.label}
                    end={live ? streamerCount : stat.value}
                    suffix={live ? "" : stat.suffix}
                    label={stat.label}
                  />
                );
              })}
            </div>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}
