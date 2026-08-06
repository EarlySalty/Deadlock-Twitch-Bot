import { useEffect, useState } from "react";
import type { BanEntry, BanStats } from "@/hooks/useBanFeed";

const NETWORK_API =
  "https://deutsche-deadlock-community.de/twitch/api/v2/public/network";
const BANS_API =
  "https://deutsche-deadlock-community.de/twitch/api/v2/public/recent-bans";

export interface NetworkMetrics {
  /** Aktive Partner im Netzwerk, null solange unbekannt. */
  partners: number | null;
  /** Live-Kanaele aus derselben Antwort, null solange unbekannt. */
  liveNow: number | null;
  /** Namen der Partnerkanaele fuer das Laufband. */
  partnerNames: string[];
  bans: BanEntry[];
  banStats: BanStats | null;
  /** true, sobald mindestens ein Abruf durch ist (egal ob erfolgreich). */
  settled: boolean;
}

interface NetworkStreamer {
  display_name?: string;
  login?: string;
  is_live?: boolean;
}

/**
 * Kennzahlen fuer den Open-Metrics-Block der Landing V2.
 *
 * Bewusster Unterschied zu useBanFeed: Dieser Hook hat KEINE Beispieldaten als
 * Rueckfallebene. Kapitel 31 der Strategie verlangt, dass jede Zahl auf der
 * Seite live gemessen ist. Faellt ein Abruf aus, bleibt der Wert null und die
 * Oberflaeche zeigt an dieser Stelle einen Hinweis statt einer Zahl.
 */
export function useNetworkMetrics(): NetworkMetrics {
  const [metrics, setMetrics] = useState<NetworkMetrics>({
    partners: null,
    liveNow: null,
    partnerNames: [],
    bans: [],
    banStats: null,
    settled: false,
  });

  useEffect(() => {
    let cancelled = false;

    async function loadNetwork() {
      try {
        const res = await fetch(NETWORK_API);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        const list: NetworkStreamer[] = Array.isArray(data?.streamers)
          ? data.streamers
          : [];
        if (cancelled || list.length === 0) return;
        setMetrics((prev) => ({
          ...prev,
          partners: list.length,
          liveNow: list.filter((s) => s.is_live).length,
          partnerNames: list
            .map((s) => s.display_name || s.login || "")
            .filter(Boolean),
        }));
      } catch {
        // Kein Fallback: partners bleibt null, die Kachel meldet das offen.
      }
    }

    async function loadBans() {
      try {
        const res = await fetch(BANS_API);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        if (cancelled) return;
        setMetrics((prev) => ({
          ...prev,
          bans: Array.isArray(data?.bans) ? data.bans.slice(0, 6) : [],
          banStats: data?.stats ?? null,
        }));
      } catch {
        // Kein Fallback, siehe oben.
      }
    }

    Promise.allSettled([loadNetwork(), loadBans()]).then(() => {
      if (!cancelled) setMetrics((prev) => ({ ...prev, settled: true }));
    });

    const id = setInterval(loadBans, 45_000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  return metrics;
}
