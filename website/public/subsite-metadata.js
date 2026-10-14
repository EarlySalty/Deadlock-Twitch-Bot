(() => {
  const language = new URLSearchParams(window.location.search).get("lang");
  if (language === "de" || (language !== "en" && !["dead-lock.de", "www.dead-lock.de"].includes(window.location.hostname.toLowerCase()))) return;
  const path = window.location.pathname.replace(/\/index\.html$/, "/").replace(/\/$/, "");
  const page = path.split("/").pop();
  const pages = {
    faq: {
      title: "Reception · Questions about the Twitch bot",
      description: "The German Deadlock Community concierge answers your questions about the Twitch bot: setup, chat commands, auto-raids, overlays, plans and privacy.",
      social: "Ask the concierge about the Twitch bot. He answers only from the bot's records and otherwise points you elsewhere.",
    },
    vergleich: {
      title: "DDL Streamer Comparison | German Deadlock Community",
      description: "Public comparison of active Deadlock streamers in the DDL network: reach, momentum and measurable raid impact with transparent methodology.",
      social: "Compare active Deadlock streamers fairly and see where raids, momentum or more data could provide the next boost.",
    },
    onboarding: {
      title: "EarlySalty Streamer Onboarding",
      description: "German Deadlock Community streamer onboarding on deutsche-deadlock-community.de/twitch. Getting started, sign-in, dashboard, raids, announcements and billing on one product page.",
      social: "How the EarlySalty partner dashboard actually works for new streamers.",
    },
    clips: {
      title: "Clip of the Month | German Deadlock Community",
      description: "Monthly clip contest from the German Deadlock Community.",
      social: "Submit Deadlock clips, vote and see the top 3 in the archive.",
    },
  };
  const copy = pages[page];
  if (!copy) return;
  document.documentElement.lang = "en";
  document.title = copy.title;
  const canonical = new URL(`/streamer/${page}/`, window.location.origin);
  if (!["dead-lock.de", "www.dead-lock.de"].includes(window.location.hostname.toLowerCase())) canonical.searchParams.set("lang", "en");
  const values = {
    'meta[name="description"]': copy.description,
    'meta[property="og:title"]': copy.title,
    'meta[property="og:description"]': copy.social,
    'meta[property="og:locale"]': "en_GB",
    'meta[property="og:url"]': canonical.toString(),
    'meta[property="og:image:alt"]': "German Deadlock Community logo",
    'meta[name="twitter:title"]': copy.title,
    'meta[name="twitter:description"]': copy.social,
  };
  for (const [selector, value] of Object.entries(values)) document.querySelector(selector)?.setAttribute("content", value);
  document.querySelector('link[rel="canonical"]')?.setAttribute("href", canonical.toString());
})();
