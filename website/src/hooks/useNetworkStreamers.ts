import { useEffect, useState } from "react";

const NETWORK_API =
  "https://deutsche-deadlock-community.de/twitch/api/v2/public/network";

export interface NetworkStreamer {
  login: string;
  displayName?: string;
  avatarUrl?: string;
  isLive: boolean;
  viewers: number;
  game?: string;
  dlStreams30d: number;
  avgViewers30d: number;
}

export type NetworkStatus = "loading" | "ready" | "error";

interface NetworkStreamerJson {
  login: string;
  display_name?: string | null;
  avatar_url?: string | null;
  is_live?: boolean;
  viewer_count?: number;
  game?: string | null;
  deadlock_streams_30d?: number;
  avg_viewers_30d?: number;
}

function mapStreamer(raw: NetworkStreamerJson): NetworkStreamer {
  return {
    login: raw.login,
    displayName: raw.display_name ?? undefined,
    avatarUrl: raw.avatar_url ?? undefined,
    isLive: Boolean(raw.is_live),
    viewers: raw.viewer_count ?? 0,
    game: raw.game ?? undefined,
    dlStreams30d: raw.deadlock_streams_30d ?? 0,
    avgViewers30d: raw.avg_viewers_30d ?? 0,
  };
}

function sortStreamers(list: NetworkStreamer[]): NetworkStreamer[] {
  return [...list].sort((a, b) => {
    if (a.isLive !== b.isLive) return a.isLive ? -1 : 1;
    if (a.isLive && b.isLive) {
      if (b.viewers !== a.viewers) return b.viewers - a.viewers;
    } else if (b.dlStreams30d !== a.dlStreams30d) {
      return b.dlStreams30d - a.dlStreams30d;
    }
    const nameA = (a.displayName ?? a.login).toLowerCase();
    const nameB = (b.displayName ?? b.login).toLowerCase();
    return nameA.localeCompare(nameB, "de");
  });
}

let netzwerkPromise: Promise<NetworkStreamer[]> | null = null;

function ladeNetzwerk(): Promise<NetworkStreamer[]> {
  if (netzwerkPromise) return netzwerkPromise;
  netzwerkPromise = (async () => {
    const res = await fetch(NETWORK_API);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json();
    if (!Array.isArray(data?.streamers)) throw new Error("kein streamers-Feld");
    const mapped = (data.streamers as NetworkStreamerJson[])
      .filter((s) => typeof s?.login === "string" && s.login.length > 0)
      .map(mapStreamer);
    return sortStreamers(mapped);
  })();
  return netzwerkPromise;
}

if (typeof window !== "undefined") {
  void ladeNetzwerk().catch(() => {});
}

export function useNetworkStreamers(): {
  streamers: NetworkStreamer[];
  status: NetworkStatus;
} {
  const [streamers, setStreamers] = useState<NetworkStreamer[]>([]);
  const [status, setStatus] = useState<NetworkStatus>("loading");

  useEffect(() => {
    let cancelled = false;
    ladeNetzwerk()
      .then((liste) => {
        if (cancelled) return;
        setStreamers(liste);
        setStatus("ready");
      })
      .catch((err) => {
        if (cancelled) return;
        console.error("useNetworkStreamers: Netzwerk-API nicht ladbar:", err);
        setStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return { streamers, status };
}
