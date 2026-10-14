import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";

const root = new URL("../", import.meta.url);
function loadLocale(hostname, pathname = "/streamer/") {
  const dictionary = ts.transpileModule(
    readFileSync(new URL("src/data/landingEnglish.ts", root), "utf8"),
    { compilerOptions: { module: ts.ModuleKind.CommonJS } },
  ).outputText;
  const english = { exports: {} };
  runInNewContext(dictionary, english);
  const source = ts.transpileModule(
    readFileSync(new URL("src/lib/landingLocale.ts", root), "utf8"),
    { compilerOptions: { module: ts.ModuleKind.CommonJS } },
  ).outputText;
  const context = {
    exports: {},
    require: () => english.exports,
    URL,
    window: { location: { hostname, pathname } },
  };
  runInNewContext(source, context);
  return context.exports;
}

test("only the two English hosts select English on the main landing page", () => {
  for (const host of ["dead-lock.de", "www.dead-lock.de"]) {
    const locale = loadLocale(host);
    assert.equal(locale.isEnglishLanding(), true);
    assert.equal(
      locale.landingText("Jetzt Partner werden"),
      "Become a partner",
    );
    assert.equal(locale.landingLocale(), "en-GB");
    assert.equal(loadLocale(host, "/").isEnglishLanding(), true);
    for (const path of [
      "/streamer/v1/",
      "/streamer/v3/",
      "/streamer/clips/",
      "/streamer/vergleich/",
      "/streamer/vertriebler/",
      "/twitch/onboarding",
      "/twitch/faq",
    ]) {
      assert.equal(loadLocale(host, path).isEnglishLanding(), false);
    }
  }
  for (const host of [
    "deutsche-deadlock-community.de",
    "www.deutsche-deadlock-community.de",
    "dead-lock.de.example.org",
    "localhost",
  ]) {
    const locale = loadLocale(host);
    assert.equal(locale.isEnglishLanding(), false);
    assert.equal(
      locale.landingText("Jetzt Partner werden"),
      "Jetzt Partner werden",
    );
    assert.equal(locale.landingLocale(), "de-DE");
  }
});

test("English login and dashboard links select English on the main domain, including next", () => {
  const locale = loadLocale("dead-lock.de");
  const login = new URL(
    locale.landingLink(
      "/twitch/auth/login?next=%2Ftwitch%2Fdashboard%3Ftab%3Draids",
    ),
  );
  assert.equal(login.origin, "https://deutsche-deadlock-community.de");
  assert.equal(login.searchParams.get("lang"), "en");
  assert.equal(
    login.searchParams.get("next"),
    "/twitch/dashboard?tab=raids&lang=en",
  );
  assert.equal(
    new URL(
      locale.landingLink("/twitch/raid/auth?scope_profile=base"),
    ).searchParams.get("lang"),
    "en",
  );
  assert.equal(
    locale.landingLink("/twitch/dashboard"),
    "https://deutsche-deadlock-community.de/twitch/dashboard?lang=en",
  );
  assert.equal(
    locale.landingLink("https://discord.gg/example"),
    "https://discord.gg/example",
  );
  assert.equal(
    locale.landingLink("/twitch/impressum"),
    "https://deutsche-deadlock-community.de/twitch/impressum",
  );
  assert.equal(
    locale.landingLink("/twitch/datenschutz"),
    "https://deutsche-deadlock-community.de/twitch/datenschutz",
  );
  assert.equal(
    locale.landingLink("/streamer/vergleich/"),
    "https://deutsche-deadlock-community.de/streamer/vergleich/",
  );
  assert.equal(
    loadLocale("deutsche-deadlock-community.de").landingLink(
      "/twitch/dashboard",
    ),
    "/twitch/dashboard",
  );
  assert.equal(
    new URL(
      locale.landingLink("/twitch/auth/login?next=https://example.org"),
    ).searchParams.get("next"),
    "/twitch/dashboard?lang=en",
  );
});

test("metadata uses matching English language, canonical and social descriptions", () => {
  const source = readFileSync(
    new URL("public/landing-metadata.js", root),
    "utf8",
  );
  const html = readFileSync(new URL("index.html", root), "utf8");
  assert.match(html, /hreflang="en" href="https:\/\/dead-lock.de\/streamer\/"/);
  assert.match(
    html,
    /hreflang="de" href="https:\/\/deutsche-deadlock-community.de\/streamer\/"/,
  );
  for (const hostname of [
    "dead-lock.de",
    "www.dead-lock.de",
    "deutsche-deadlock-community.de",
  ]) {
    const nodes = new Map();
    const schema = {
      textContent: JSON.stringify({
        "@graph": [
          { "@type": "WebPage" },
          { "@type": "WebSite" },
          { "@type": "FAQPage" },
        ],
      }),
    };
    const document = {
      title: "German title",
      documentElement: { lang: "de" },
      querySelector(selector) {
        if (selector === 'script[type="application/ld+json"]') return schema;
        const node = {
          value: null,
          setAttribute(_name, value) {
            this.value = value;
          },
        };
        nodes.set(selector, node);
        return node;
      },
    };
    runInNewContext(source, { document, window: { location: { hostname } } });
    if (hostname === "deutsche-deadlock-community.de") {
      assert.equal(document.documentElement.lang, "de");
      assert.equal(document.title, "German title");
      assert.equal(nodes.size, 0);
    } else {
      assert.equal(document.documentElement.lang, "en");
      assert.equal(
        nodes.get('link[rel="canonical"]').value,
        "https://dead-lock.de/streamer/",
      );
      assert.match(document.title, /Deadlock Partner Network/);
      assert.match(
        nodes.get('meta[name="description"]').value,
        /Join the German Deadlock Community/,
      );
      assert.equal(nodes.get('meta[property="og:locale"]').value, "en_GB");
      const graph = JSON.parse(schema.textContent)["@graph"];
      assert.equal(
        graph.some((entry) => entry["@type"] === "FAQPage"),
        false,
      );
      assert.equal(
        graph.find((entry) => entry["@type"] === "WebPage").inLanguage,
        "en-GB",
      );
    }
  }
});
