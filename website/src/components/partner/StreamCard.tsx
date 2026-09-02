import { clipPfp, clipSrc, NETWORK_CLIPS, twitchUrl } from "@/data/partnerPage";
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

export function knownClip(login: string): boolean {
  return NETWORK_CLIPS.some((clip) => clip.login === login);
}

export function livePreviewUrl(login: string): string {
  const tick = Math.floor(Date.now() / 60_000);
  return `https://static-cdn.jtvnw.net/previews-ttv/live_user_${encodeURIComponent(login)}-1280x720.jpg?t=${tick}`;
}

export function Avatar({
  login,
  avatarUrl,
  size = 40,
}: {
  login: string;
  avatarUrl?: string;
  size?: number;
}) {
  const local = knownClip(login) ? clipPfp(login) : undefined;
  const src = avatarUrl || local;
  return (
    <span
      className="pn-avatar"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.38,
        background: avatarColor(login),
      }}
    >
      {initials(login)}
      {src ? (
        <img
          src={src}
          alt=""
          onError={(event) => {
            event.currentTarget.style.display = "none";
          }}
        />
      ) : null}
    </span>
  );
}

type StreamCardProps = {
  login: string;
  displayName: string;
  live?: boolean;
  viewers?: number;
  avatarUrl?: string;
  ended?: boolean;
  dim?: boolean;
};

export function StreamCard({
  login,
  displayName,
  live = false,
  viewers,
  avatarUrl,
  ended = false,
  dim = false,
}: StreamCardProps) {
  const hasClip = knownClip(login);

  return (
    <a
      className={`pn-card${dim ? " is-dim" : ""}`}
      href={twitchUrl(login)}
      target="_blank"
      rel="noopener noreferrer"
    >
      <div className="pn-player">
        {live && !ended ? (
          <img
            src={livePreviewUrl(login)}
            alt=""
            onError={(event) => {
              event.currentTarget.style.display = "none";
            }}
          />
        ) : hasClip ? (
          <video src={clipSrc(login)} muted autoPlay loop playsInline />
        ) : null}
        <div className="pn-scrim" />
        {live && !ended ? (
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
      <span className="pn-bar">
        <Avatar login={login} avatarUrl={avatarUrl} size={36} />
        <span>
          <strong>{displayName}</strong>
          <small>
            {live && typeof viewers === "number"
              ? `${viewers.toLocaleString("de-DE")} Zuschauer`
              : "Deadlock"}
          </small>
        </span>
      </span>
    </a>
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
