import { landingEnglish } from "../data/landingEnglish";

export function isEnglishLanding(
  location:
    Pick<Location, "hostname" | "pathname"> | undefined = typeof window ===
  "undefined"
    ? undefined
    : window.location,
): boolean {
  return Boolean(
    location &&
    ["dead-lock.de", "www.dead-lock.de"].includes(
      location.hostname.toLowerCase(),
    ) &&
    [
      "/",
      "/index.html",
      "/streamer",
      "/streamer/",
      "/streamer/index.html",
    ].includes(location.pathname),
  );
}

export function landingText(text: string): string {
  return isEnglishLanding() ? (landingEnglish[text] ?? text) : text;
}

export function landingLocale(): string {
  return isEnglishLanding() ? "en-GB" : "de-DE";
}

export function landingLink(raw: string): string {
  if (!isEnglishLanding()) return raw;
  const mainOrigin = "https://deutsche-deadlock-community.de";
  const url = new URL(raw, mainOrigin);
  if (url.origin !== mainOrigin) return raw;
  if (
    url.pathname.startsWith("/twitch/") ||
    url.pathname.startsWith("/demo/twitch/")
  ) {
    if (
      ![
        "/twitch/impressum",
        "/twitch/datenschutz",
        "/twitch/agb",
        "/twitch/sicherheit",
      ].includes(url.pathname)
    ) {
      url.searchParams.set("lang", "en");
    }
    if (
      ["/twitch/auth/login", "/twitch/auth/discord/link"].includes(url.pathname)
    ) {
      const fallback =
        url.pathname === "/twitch/auth/discord/link"
          ? "/twitch/verwaltung"
          : "/twitch/dashboard";
      const candidate = url.searchParams.get("next") ?? fallback;
      const next = new URL(candidate, mainOrigin);
      if (
        next.origin === mainOrigin &&
        candidate.startsWith("/") &&
        !candidate.startsWith("//") &&
        !candidate.includes("\\")
      ) {
        next.searchParams.set("lang", "en");
        url.searchParams.set(
          "next",
          `${next.pathname}${next.search}${next.hash}`,
        );
      } else {
        url.searchParams.set("next", `${fallback}?lang=en`);
      }
    }
    return url.toString();
  }
  if (url.pathname.startsWith("/streamer/") && url.pathname !== "/streamer/")
    return url.toString();
  return raw;
}
