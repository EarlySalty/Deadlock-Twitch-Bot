import type { NetworkStreamer } from "@/hooks/useNetworkStreamers";

export interface Partnergliederung {
  embeds: NetworkStreamer[];
  weitereDeadlock: NetworkStreamer[];
  allePartner: NetworkStreamer[];
}

export function istDeadlock(s: { game?: string }): boolean {
  return typeof s.game === "string" && s.game.trim().toLowerCase() === "deadlock";
}

export function impactScore(
  s: { dlStreams30d: number; avgViewers30d: number },
  maxDlStreams: number,
  maxAvgViewers: number,
): number {
  const anteilStreams = maxDlStreams > 0 ? s.dlStreams30d / maxDlStreams : 0;
  const anteilViewers = maxAvgViewers > 0 ? s.avgViewers30d / maxAvgViewers : 0;
  return 0.5 * anteilStreams + 0.5 * anteilViewers;
}

function nameVon(s: NetworkStreamer): string {
  return (s.displayName ?? s.login).toLowerCase();
}

export function gliederePartner(liste: NetworkStreamer[]): Partnergliederung {
  const deadlockLive = liste
    .filter((s) => s.isLive && istDeadlock(s))
    .sort((a, b) => {
      if (b.viewers !== a.viewers) return b.viewers - a.viewers;
      return nameVon(a).localeCompare(nameVon(b), "de");
    });

  const embeds = deadlockLive.slice(0, 3);
  const weitereDeadlock = deadlockLive.slice(3);

  const gezeigt = new Set(deadlockLive.map((s) => s.login));
  const maxDlStreams = liste.reduce((m, s) => Math.max(m, s.dlStreams30d), 0);
  const maxAvgViewers = liste.reduce((m, s) => Math.max(m, s.avgViewers30d), 0);

  const allePartner = liste
    .filter((s) => !gezeigt.has(s.login))
    .sort((a, b) => {
      const scoreA = impactScore(a, maxDlStreams, maxAvgViewers);
      const scoreB = impactScore(b, maxDlStreams, maxAvgViewers);
      if (scoreB !== scoreA) return scoreB - scoreA;
      return nameVon(a).localeCompare(nameVon(b), "de");
    });

  return { embeds, weitereDeadlock, allePartner };
}

const AVATAR_COLORS = [
  "#c8a86b",
  "#55978f",
  "#dd6a4d",
  "#e0912f",
  "#46c07b",
  "#7a9cc6",
];

export function avatarColor(login: string): string {
  let hash = 0;
  for (let i = 0; i < login.length; i++) {
    hash = (hash * 31 + login.charCodeAt(i)) >>> 0;
  }
  return AVATAR_COLORS[hash % AVATAR_COLORS.length];
}

export function initials(login: string): string {
  return login.replace(/[^A-Za-z0-9]/g, "").slice(0, 2).toUpperCase() || "DL";
}

export function twitchUrl(login: string): string {
  return `https://twitch.tv/${login}`;
}

export function twitchParent(): string {
  if (typeof window !== "undefined" && window.location.hostname) {
    return window.location.hostname;
  }
  return "deutsche-deadlock-community.de";
}

export function previewImageUrl(login: string): string {
  return `https://static-cdn.jtvnw.net/previews-ttv/live_user_${encodeURIComponent(login)}-640x360.jpg`;
}

export function zuschauerSchnitt(n: number): string {
  return Math.round(n).toLocaleString("de-DE", { maximumFractionDigits: 0 });
}
