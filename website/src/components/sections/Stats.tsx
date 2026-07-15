import { stats } from "@/data/stats";
import { AnimatedCounter } from "@/components/ui/AnimatedCounter";
import { ScrollReveal } from "@/components/ui/ScrollReveal";
import { useNetworkCount } from "@/hooks/useNetworkCount";

export function Stats() {
  const streamerCount = useNetworkCount();

  return (
    <section id="stats" className="relative z-10">
      <div className="max-w-5xl mx-auto px-6 -mt-8 relative z-10">
        <ScrollReveal>
          <div className="panel-card rounded-2xl p-8">
            <div className="grid grid-cols-2 md:grid-cols-5 gap-8">
              {stats.map((stat) => {
                // Die "Streamer"-Kachel zieht die echte Netzwerk-Groesse live
                // aus der DB (/public/network). Faellt der Fetch aus, greift der
                // statische Default (30+) aus data/stats.ts.
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
