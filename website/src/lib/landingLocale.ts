import { landingEnglish } from "../data/landingEnglish";

export function isEnglishLanding(
  location:
    | (Pick<Location, "hostname" | "pathname"> &
        Partial<Pick<Location, "search">>)
    | undefined = typeof window === "undefined" ? undefined : window.location,
): boolean {
  if (!location) return false;
  const language = new URLSearchParams(location.search ?? "").get("lang");
  if (language === "de") return false;
  if (
    ["dead-lock.de", "www.dead-lock.de"].includes(
      location.hostname.toLowerCase(),
    )
  )
    return true;
  const path =
    location.pathname.replace(/\/index\.html$/, "/").replace(/\/$/, "") || "/";
  if (
    ![
      "/",
      "/streamer",
      "/streamer/faq",
      "/streamer/onboarding",
      "/streamer/vergleich",
      "/streamer/clips",
      "/twitch/faq",
      "/twitch/onboarding",
      "/clips",
      "/streamer/affiliate-portal",
      "/streamer/vertriebler",
      "/streamer/v1",
      "/streamer/v3",
      "/affiliate-portal",
      "/vertriebler",
      "/v1",
      "/v3",
      "/twitch/affiliate/portal",
    ].includes(path)
  )
    return false;
  return language === "en";
}

export function landingText(text: string): string {
  return isEnglishLanding() ? (landingEnglish[text] ?? text) : text;
}

export function landingLocale(): string {
  return isEnglishLanding() ? "en-GB" : "de-DE";
}

export function landingLink(raw: string): string {
  if (!isEnglishLanding() || raw.startsWith("#")) return raw;
  const mainOrigin = "https://deutsche-deadlock-community.de";
  const url = new URL(raw, mainOrigin);
  if (
    ![mainOrigin, "https://dead-lock.de", "https://www.dead-lock.de"].includes(
      url.origin,
    )
  )
    return raw;
  const publicPaths: Record<string, string> = {
    "/": "/streamer/",
    "/streamer": "/streamer/",
    "/streamer/": "/streamer/",
    "/twitch/faq": "/streamer/faq/",
    "/twitch/onboarding": "/streamer/onboarding/",
    "/clips": "/streamer/clips/",
  };
  const normalized = url.pathname.replace(/\/$/, "");
  if (
    publicPaths[url.pathname] ||
    publicPaths[normalized] ||
    /^\/streamer\/(faq|onboarding|vergleich|clips|affiliate-portal|vertriebler|v1|v3)(\/|\/index\.html)?$/.test(
      url.pathname,
    )
  ) {
    url.pathname =
      publicPaths[url.pathname] ?? publicPaths[normalized] ?? url.pathname;
    url.searchParams.set("lang", "en");
    return `${url.pathname}${url.search}${url.hash}`;
  }
  if (
    url.pathname === "/twitch/affiliate" ||
    url.pathname.startsWith("/twitch/affiliate/")
  ) {
    url.searchParams.set("lang", "en");
    return `${url.pathname}${url.search}${url.hash}`;
  }
  url.protocol = "https:";
  url.host = "deutsche-deadlock-community.de";
  if (
    url.pathname.startsWith("/twitch/") ||
    url.pathname.startsWith("/demo/twitch/") ||
    url.pathname.startsWith("/clips/auth/")
  ) {
    if (
      ![
        "/twitch/impressum",
        "/twitch/datenschutz",
        "/twitch/agb",
        "/twitch/sicherheit",
      ].includes(url.pathname)
    )
      url.searchParams.set("lang", "en");
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
      } else url.searchParams.set("next", `${fallback}?lang=en`);
    }
    return url.toString();
  }
  if (url.pathname.startsWith("/streamer/") && url.pathname !== "/streamer/")
    return url.toString();
  return raw;
}
