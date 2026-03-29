import { useCallback, useEffect, useRef, useState } from "react";

const API_URL =
  "https://twitch.earlysalty.com/twitch/api/v2/public/recent-raids";
const POLL_INTERVAL = 30_000;

export interface RaidEvent {
  from_channel: string;
  to_channel: string;
  viewers: number;
  executed_at: string;
}

function generateFallbackRaids(): RaidEvent[] {
  const now = Date.now();
  return [
    {
      from_channel: "Nachtfalke",
      to_channel: "EarlySalty",
      viewers: 47,
      executed_at: new Date(now - 12 * 60_000).toISOString(),
    },
    {
      from_channel: "EarlySalty",
      to_channel: "PixelRaid",
      viewers: 63,
      executed_at: new Date(now - 38 * 60_000).toISOString(),
    },
    {
      from_channel: "Sturmjäger",
      to_channel: "SaltyViper",
      viewers: 29,
      executed_at: new Date(now - 72 * 60_000).toISOString(),
    },
    {
      from_channel: "IceBreaker",
      to_channel: "Drachenatem",
      viewers: 51,
      executed_at: new Date(now - 105 * 60_000).toISOString(),
    },
    {
      from_channel: "Flammenherz",
      to_channel: "Frostbiss",
      viewers: 34,
      executed_at: new Date(now - 140 * 60_000).toISOString(),
    },
    {
      from_channel: "Nebeljagd",
      to_channel: "Klingenwind",
      viewers: 22,
      executed_at: new Date(now - 180 * 60_000).toISOString(),
    },
  ];
}

interface RaidEventsData {
  raids: RaidEvent[];
  isLoading: boolean;
}

export function useRaidEvents(): RaidEventsData {
  const [raids, setRaids] = useState<RaidEvent[]>(generateFallbackRaids);
  const [isLoading, setIsLoading] = useState(true);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchRaids = useCallback(async () => {
    try {
      const res = await fetch(API_URL);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);

      const data: RaidEvent[] = await res.json();
      if (Array.isArray(data) && data.length > 0) {
        setRaids(data);
      }
    } catch {
      // Keep fallback data
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchRaids();
    timerRef.current = setInterval(fetchRaids, POLL_INTERVAL);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [fetchRaids]);

  return { raids, isLoading };
}
