import { useEffect, useState } from "react";

const NETWORK_STATS_API =
  "https://deutsche-deadlock-community.de/twitch/api/v2/public/network-stats";

export interface NetworkStats {
  activePartners: number;
  raidsTotal: number;
  raids7d: number;
  viewersForwardedTotal: number | null;
}

function positiveZahl(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0
    ? value
    : null;
}

export function useNetworkStats(): NetworkStats | null {
  const [stats, setStats] = useState<NetworkStats | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await fetch(NETWORK_STATS_API);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        if (cancelled) return;
        setStats({
          activePartners: positiveZahl(data?.active_partners) ?? 0,
          raidsTotal: positiveZahl(data?.raids_total) ?? 0,
          raids7d: positiveZahl(data?.raids_7d) ?? 0,
          viewersForwardedTotal: positiveZahl(data?.viewers_forwarded_total),
        });
      } catch {
        if (!cancelled) setStats(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return stats;
}
