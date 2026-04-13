import { useEffect, useState, useCallback } from "react";

const API_URL =
  "https://deutsche-deadlock-community.de/twitch/api/v2/public/recent-bans";

export interface BanEntry {
  target_login: string;
  moderator_login: string;
  reason: string;
  received_at: string;
}

export interface BanStats {
  today: number;
  total_30d: number;
  channels_protected: number;
}

export interface BanFeedData {
  bans: BanEntry[];
  stats: BanStats;
}

const FALLBACK_BANS: BanEntry[] = [
  {
    target_login: "spambot_9182",
    moderator_login: "EarlySalty",
    reason: "Cheap Viewers streamboo .com",
    received_at: new Date(Date.now() - 2 * 60_000).toISOString(),
  },
  {
    target_login: "xViewerz_boost",
    moderator_login: "EarlySalty",
    reason: "Best viewers smaihype.ru",
    received_at: new Date(Date.now() - 5 * 60_000).toISOString(),
  },
  {
    target_login: "free_follows_23",
    moderator_login: "EarlySalty",
    reason: "Buy followers cheap streamrise .net",
    received_at: new Date(Date.now() - 11 * 60_000).toISOString(),
  },
  {
    target_login: "promo_tv_live",
    moderator_login: "EarlySalty",
    reason: "Wanna become famous? Visit viewbotz .com",
    received_at: new Date(Date.now() - 18 * 60_000).toISOString(),
  },
  {
    target_login: "botnet_viewer",
    moderator_login: "EarlySalty",
    reason: "Free viewers viewerking .pro",
    received_at: new Date(Date.now() - 32 * 60_000).toISOString(),
  },
  {
    target_login: "spam_raid_x",
    moderator_login: "EarlySalty",
    reason: "Get real viewers at boostchat .gg",
    received_at: new Date(Date.now() - 47 * 60_000).toISOString(),
  },
  {
    target_login: "cheap_promo_33",
    moderator_login: "EarlySalty",
    reason: "Grow your channel streambots .xyz",
    received_at: new Date(Date.now() - 68 * 60_000).toISOString(),
  },
  {
    target_login: "followbot_ru",
    moderator_login: "EarlySalty",
    reason: "Best followers 4u at twitchgrow .shop",
    received_at: new Date(Date.now() - 95 * 60_000).toISOString(),
  },
];

const FALLBACK_STATS: BanStats = {
  today: 47,
  total_30d: 1234,
  channels_protected: 32,
};

export function useBanFeed() {
  const [bans, setBans] = useState<BanEntry[]>(FALLBACK_BANS);
  const [stats, setStats] = useState<BanStats>(FALLBACK_STATS);
  const [isLoading, setIsLoading] = useState(true);

  const fetchData = useCallback(async () => {
    try {
      const res = await fetch(API_URL);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data: BanFeedData = await res.json();
      setBans(data.bans);
      setStats(data.stats);
    } catch {
      // Keep fallback data on error
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
    const id = setInterval(fetchData, 30_000);
    return () => clearInterval(id);
  }, [fetchData]);

  return { bans, stats, isLoading };
}
