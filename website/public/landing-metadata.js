(() => {
  if (
    !["dead-lock.de", "www.dead-lock.de"].includes(
      window.location.hostname.toLowerCase(),
    )
  )
    return;
  document.documentElement.lang = "en";
  const canonical = "https://dead-lock.de/streamer/";
  const title =
    "Deadlock Partner Network for Twitch Streamers · German Deadlock Community";
  const description =
    "Join the German Deadlock Community partner network: automatic raids, Discord live announcements, chat protection and analytics. The bot manages your channel while you stream.";
  document.title = title;
  const values = {
    'meta[name="description"]': description,
    'meta[name="keywords"]':
      "Deadlock partner network, Deadlock Twitch streamers, automatic raids, Discord, German Deadlock Community, Twitch raid network, chat protection",
    'meta[property="og:locale"]': "en_GB",
    'meta[property="og:url"]': canonical,
    'meta[property="og:title"]': title,
    'meta[property="og:description"]': description,
    'meta[property="og:image:alt"]':
      "Join the German Deadlock Community partner network",
    'meta[name="twitter:title"]': title,
    'meta[name="twitter:description"]': description,
  };
  for (const [selector, value] of Object.entries(values)) {
    document.querySelector(selector)?.setAttribute("content", value);
  }
  document
    .querySelector('link[rel="canonical"]')
    ?.setAttribute("href", canonical);
  const schema = document.querySelector('script[type="application/ld+json"]');
  if (!schema) return;
  const data = JSON.parse(schema.textContent);
  data["@graph"] = data["@graph"].filter(
    (entry) => entry["@type"] !== "FAQPage",
  );
  for (const entry of data["@graph"]) {
    if (entry["@type"] === "Organization")
      entry.description =
        "A partner network for German Deadlock streamers with automatic raids, Discord live announcements, chat protection and analytics.";
    if (entry["@type"] === "WebSite") {
      entry["@id"] = "https://dead-lock.de/#website";
      entry.url = "https://dead-lock.de/";
      entry.inLanguage = "en-GB";
    }
    if (entry["@type"] === "WebPage") {
      entry["@id"] = `${canonical}#webpage`;
      entry.url = canonical;
      entry.name = title;
      entry.description = description;
      entry.inLanguage = "en-GB";
      entry.isPartOf = { "@id": "https://dead-lock.de/#website" };
      entry.breadcrumb = { "@id": `${canonical}#breadcrumb` };
    }
    if (entry["@type"] === "BreadcrumbList") {
      entry["@id"] = `${canonical}#breadcrumb`;
      entry.itemListElement = [
        {
          "@type": "ListItem",
          position: 1,
          name: "Home",
          item: "https://dead-lock.de/",
        },
        {
          "@type": "ListItem",
          position: 2,
          name: "Streamers",
          item: canonical,
        },
      ];
    }
  }
  schema.textContent = JSON.stringify(data);
})();
