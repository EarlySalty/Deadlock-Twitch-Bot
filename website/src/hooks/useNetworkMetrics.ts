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
  /** Vollstaendige Partnerkanaele fuer Hero-Buehne und Live-Block. */
  partnerList: PartnerChannel[];
  bans: BanEntry[];
  banStats: BanStats | null;
  /**
   * true, wenn die API zu den Kanaelen eine Spielkategorie liefert. Ist sie
   * false, kann "gerade live" NICHT auf Deadlock eingegrenzt werden und die
   * Oberflaeche darf das auch nicht behaupten.
   */
  categoryKnown: boolean;
  /** true, sobald mindestens ein Abruf durch ist (egal ob erfolgreich). */
  settled: boolean;
}

interface NetworkStreamer {
  display_name?: string;
  login?: string;
  is_live?: boolean;
  game?: string;
  viewer_count?: number;
  avatar_url?: string;
  deadlock_streams_30d?: number;
  avg_viewers_30d?: number;
}

/** Ein Partnerkanal, so wie ihn Hero-Buehne und Live-Block brauchen. */
export interface PartnerChannel {
  login: string;
  displayName: string;
  isLive: boolean;
  viewers: number;
  game?: string;
  avatarUrl?: string;
  /** Live UND in Deadlock. Nur diese zaehlen als "gerade live". */
  liveDeadlock: boolean;
  dlStreams30d: number;
  avgViewers30d: number;
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
    partnerList: [],
    categoryKnown: false,
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

        // Liefert die API ueberhaupt Kategorien, gilt "live" nur mit Deadlock.
        // Fehlt das Feld komplett, waere die Zahl sonst dauerhaft 0.
        const hasGame = list.some(
          (s) => typeof s.game === "string" && s.game.trim() !== "",
        );

        const channels: PartnerChannel[] = list
          .map((s) => {
            const isLive = !!s.is_live;
            const game =
              typeof s.game === "string" && s.game.trim() !== ""
                ? s.game.trim()
                : undefined;
            return {
              login: s.login || "",
              displayName: s.display_name || s.login || "",
              isLive,
              viewers: typeof s.viewer_count === "number" ? s.viewer_count : 0,
              game,
              avatarUrl:
                typeof s.avatar_url === "string" && s.avatar_url.trim() !== ""
                  ? s.avatar_url
                  : undefined,
              liveDeadlock:
                isLive && (!hasGame || (game ?? "").toLowerCase() === "deadlock"),
              dlStreams30d:
                typeof s.deadlock_streams_30d === "number"
                  ? s.deadlock_streams_30d
                  : 0,
              avgViewers30d:
                typeof s.avg_viewers_30d === "number" ? s.avg_viewers_30d : 0,
            };
          })
          .filter((c) => c.login)
          .sort((a, b) =>
            a.liveDeadlock === b.liveDeadlock
              ? b.viewers - a.viewers
              : a.liveDeadlock
                ? -1
                : 1,
          );

        setMetrics((prev) => ({
          ...prev,
          partners: channels.length,
          liveNow: channels.filter((c) => c.liveDeadlock).length,
          partnerNames: channels.map((c) => c.displayName),
          partnerList: channels,
          categoryKnown: hasGame,
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

    const id = setInterval(() => {
      loadNetwork();
      loadBans();
    }, 45_000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  return metrics;
}
