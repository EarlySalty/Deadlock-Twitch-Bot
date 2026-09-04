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
  return `https://static-cdn.jtvnw.net/previews-ttv/live_user_${login}-640x360.jpg`;
}
