(() => {
  const language = new URLSearchParams(window.location.search).get("lang");
  if (
    language === "de" ||
    (language !== "en" &&
      !["dead-lock.de", "www.dead-lock.de"].includes(
        window.location.hostname.toLowerCase(),
      ))
  )
    return;
  const path = window.location.pathname
    .replace(/\/index\.html$/, "/")
    .replace(/\/$/, "");
  const page =
    path === "/twitch/affiliate/portal"
      ? "affiliate-portal"
      : path.split("/").pop();
  const pages = {
    "affiliate-portal": {
      title: "EarlySalty Affiliate Portal",
      description:
        "EarlySalty affiliate portal for sign-in, streamer claims, commissions and Stripe payouts.",
      social:
        "Manage streamer claims, commissions and automatic Stripe payouts in the EarlySalty affiliate portal.",
    },
    vertriebler: {
      title: "EarlySalty Affiliate Programme",
      description:
        "Become an EarlySalty affiliate and earn 30% commission on every payment from the streamers you refer.",
      social:
        "Earn ongoing commissions with the EarlySalty affiliate programme for streamers.",
    },
    v1: {
      title:
        "Deadlock Auto-Raid Bot for Twitch · Streamer Network · German Deadlock Community",
      description:
        "The largest Deadlock raid network on Twitch. When your stream ends, our bot sends your viewers automatically to the next Deadlock streamer. Auto-raids, a live dashboard, a clip manager and a Discord community for German Deadlock streamers.",
      social:
        "The largest Deadlock raid network on Twitch. When your stream ends, your viewers automatically find the next Deadlock streamer. Auto-raids, a live dashboard and a clip manager.",
    },
    v3: {
      title:
        "Your Connection to the German Deadlock Network · German Deadlock Community",
      description:
        "The partner network for German Deadlock streamers on Twitch. When your stream ends, the network sends your viewers to the next live partner. Your viewers remain yours, and one click on Twitch lets you leave.",
      social:
        "German Deadlock streamers pass their viewers on to each other. No email, password or payment details.",
    },
    v4: {
      title:
        "Your Connection to the German Deadlock Network · German Deadlock Community",
      description:
        "The free partner network for German Deadlock streamers on Twitch. Pricing, free trial, cancellation, integrations and public API at a glance. Your viewers remain yours, and one click on Twitch lets you leave.",
      social:
        "German Deadlock streamers pass their viewers on to each other. No email, password or payment details.",
    },
    faq: {
      title: "Reception · Questions about the Twitch bot",
      description:
        "The German Deadlock Community concierge answers your questions about the Twitch bot: setup, chat commands, auto-raids, overlays, plans and privacy.",
      social:
        "Ask the concierge about the Twitch bot. He answers only from the bot's records and otherwise points you elsewhere.",
    },
    vergleich: {
      title: "DDL Streamer Comparison | German Deadlock Community",
      description:
        "Public comparison of active Deadlock streamers in the DDL network: reach, momentum and measurable raid impact with transparent methodology.",
      social:
        "Compare active Deadlock streamers fairly and see where raids, momentum or more data could provide the next boost.",
    },
    onboarding: {
      title: "EarlySalty Streamer Onboarding",
      description:
        "German Deadlock Community streamer onboarding on deutsche-deadlock-community.de/twitch. Getting started, sign-in, dashboard, raids, announcements and billing on one product page.",
      social:
        "How the EarlySalty partner dashboard actually works for new streamers.",
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
  if (
    !["dead-lock.de", "www.dead-lock.de"].includes(
      window.location.hostname.toLowerCase(),
    )
  )
    canonical.searchParams.set("lang", "en");
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
  for (const [selector, value] of Object.entries(values))
    document.querySelector(selector)?.setAttribute("content", value);
  document
    .querySelector('link[rel="canonical"]')
    ?.setAttribute("href", canonical.toString());
  if (page !== "v1") return;
  const schema = document.querySelector('script[type="application/ld+json"]');
  if (!schema) return;
  const data = JSON.parse(schema.textContent);
  data["@graph"] = data["@graph"].filter(
    (entry) => entry["@type"] !== "FAQPage",
  );
  for (const entry of data["@graph"]) {
    if (["WebSite", "WebPage", "SoftwareApplication"].includes(entry["@type"]))
      entry.inLanguage = "en-GB";
    if (["WebPage", "SoftwareApplication"].includes(entry["@type"])) {
      entry.url = canonical.toString();
      entry.name = copy.title;
      entry.description = copy.description;
    }
    if (entry["@type"] === "SoftwareApplication")
      entry.featureList = [
        "Automatic Twitch raids at the end of your stream",
        "The largest Deadlock raid network in the German-speaking region",
        "Live analytics dashboard for streamers",
        "Clip manager with automatic recording",
        "Discord integration and community posts",
        "Free for German Deadlock streamers",
      ];
    if (entry["@type"] === "BreadcrumbList")
      entry.itemListElement[0].name = "Home";
  }
  schema.textContent = JSON.stringify(data);
})();
