import { useEffect, useState } from "react";
import { TWITCH_PUBLIC_ORIGIN } from "@/data/externalLinks";

const NETWORK_API = `${TWITCH_PUBLIC_ORIGIN}/twitch/api/v2/public/network`;

/**
 * Aktuelle Anzahl aktiver Partner-Streamer im Netzwerk, live aus der DB
 * (Laenge der /public/network-Streamerliste — der Endpoint filtert bereits
 * auf aktive Partner). Einmaliger Fetch beim Mount reicht fuer eine Landing-
 * Kennzahl; kein Polling, damit die CountUp-Animation nicht neu anlaeuft.
 *
 * Faellt bei Fehler/leerer Antwort auf `null` zurueck -> die Stats-Kachel
 * nutzt dann ihren statischen Default (30+).
 */
export function useNetworkCount(): number | null {
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await fetch(NETWORK_API);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();
        if (!cancelled && Array.isArray(data?.streamers) && data.streamers.length > 0) {
          setCount(data.streamers.length);
        }
      } catch {
        // Fallback bleibt null -> statischer Default in Stats
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return count;
}
