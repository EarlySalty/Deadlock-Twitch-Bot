import {
  clipPfp,
  clipSrc,
  NETWORK_CLIPS,
  twitchUrl,
} from "@/data/partnerPage";
import type { PartnerChannel } from "@/hooks/useNetworkMetrics";

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
  for (let i = 0; i < login.length; i += 1) {
    hash = (hash * 31 + login.charCodeAt(i)) >>> 0;
  }
  return AVATAR_COLORS[hash % AVATAR_COLORS.length];
}

export function initials(login: string): string {
  return login.replace(/[^A-Za-z0-9]/g, "").slice(0, 2).toUpperCase() || "DL";
}

export function twitchParent(): string {
  if (typeof window !== "undefined" && window.location.hostname) {
    return window.location.hostname;
  }
  return "deutsche-deadlock-community.de";
}

export function knownClip(login: string): boolean {
  return NETWORK_CLIPS.some((clip) => clip.login === login);
}

type StreamCardProps = {
  login: string;
  displayName: string;
  live?: boolean;
  viewers?: number;
  avatarUrl?: string;
  ended?: boolean;
  dim?: boolean;
  embed?: boolean;
};

export function StreamCard({
  login,
  displayName,
  live = false,
  viewers,
  avatarUrl,
  ended = false,
  dim = false,
  embed = false,
}: StreamCardProps) {
  const hasClip = knownClip(login);
  const pfp = clipPfp(login);
  const showEmbed = embed && live;

  return (
    <article className={`pn-card${dim ? " is-dim" : ""}`}>
      <div className="pn-player">
        {showEmbed ? (
          <iframe
            title={`Live-Stream von ${displayName}`}
            src={`https://player.twitch.tv/?channel=${encodeURIComponent(login)}&parent=${twitchParent()}&muted=true&autoplay=true`}
            allow="autoplay; fullscreen"
            allowFullScreen
          />
        ) : hasClip ? (
          <video src={clipSrc(login)} muted autoPlay loop playsInline />
        ) : avatarUrl ? (
          <img
            src={avatarUrl}
            alt=""
            onError={(event) => {
              event.currentTarget.style.display = "none";
            }}
          />
        ) : null}
        <div className="pn-scrim" />
        {live && !ended && !showEmbed ? (
          <span className="pn-live">
            <i />
            Live
          </span>
        ) : null}
        {ended ? (
          <div className="pn-ended">
            Stream zu Ende
            <span>Die Viewer sind weg</span>
          </div>
        ) : null}
      </div>
      <a
        className="pn-bar"
        href={twitchUrl(login)}
        target="_blank"
        rel="noopener noreferrer"
      >
        <span className="pn-avatar" style={{ background: avatarColor(login) }}>
          {initials(login)}
          {avatarUrl ? (
            <img
              src={avatarUrl}
              alt=""
              onError={(event) => {
                event.currentTarget.style.display = "none";
              }}
            />
          ) : hasClip ? (
            <img src={pfp} alt="" />
          ) : null}
        </span>
        <span>
          <strong>{displayName}</strong>
          <small>
            {live && typeof viewers === "number"
              ? `${viewers.toLocaleString("de-DE")} Zuschauer`
              : "Deadlock"}
          </small>
        </span>
      </a>
    </article>
  );
}

export function clipPartners(): PartnerChannel[] {
  return NETWORK_CLIPS.map((clip) => ({
    login: clip.login,
    displayName: clip.displayName,
    isLive: false,
    viewers: 0,
    liveDeadlock: false,
    dlStreams30d: 0,
    avgViewers30d: 0,
    avatarUrl: clipPfp(clip.login),
  }));
}
