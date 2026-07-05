import { useCallback, useEffect, useState } from "react";

export interface LiveStreamer {
  login: string;
  display_name: string;
  started_at: string | null;
}

export interface NetworkStats {
  active_partners: number;
  raids_total: number;
  raids_7d: number;
  viewers_forwarded_total: number | null;
  live: LiveStreamer[];
}

const API_URL = "/twitch/api/v2/public/network-stats";
const REFRESH_MS = 60_000;

/**
 * Lädt die öffentlichen Netzwerk-Metriken. Liefert `null`, solange nichts da
 * ist (erste Ladephase oder Endpoint nicht erreichbar) — die Sektionen
 * degradieren dann sichtbar ehrlich ("—"), statt erfundene Zahlen zu zeigen.
 */
export function useNetworkStats(): { stats: NetworkStats | null; failed: boolean } {
  const [stats, setStats] = useState<NetworkStats | null>(null);
  const [failed, setFailed] = useState(false);

  const load = useCallback(async () => {
    try {
      const res = await fetch(API_URL, { headers: { Accept: "application/json" } });
      if (!res.ok) throw new Error(`status ${res.status}`);
      const body = (await res.json()) as NetworkStats;
      if (typeof body.active_partners !== "number" || !Array.isArray(body.live)) {
        throw new Error("unexpected shape");
      }
      setStats(body);
      setFailed(false);
    } catch {
      setFailed(true);
    }
  }, []);

  useEffect(() => {
    load();
    const id = setInterval(load, REFRESH_MS);
    return () => clearInterval(id);
  }, [load]);

  return { stats, failed };
}

export function formatCount(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  return value.toLocaleString("de-DE");
}
