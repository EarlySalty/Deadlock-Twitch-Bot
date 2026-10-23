import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const root = new URL("../", import.meta.url);
const source = readFileSync(
  new URL("public/subsite-metadata.js", root),
  "utf8",
);

function metadata(hostname, page, search = "") {
  const nodes = new Map();
  const document = {
    documentElement: { lang: "de" },
    title: "Original German title",
    querySelector(selector) {
      if (selector === 'script[type="application/ld+json"]') return null;
      const node = {
        setAttribute(_name, value) {
          nodes.set(selector, value);
        },
      };
      return node;
    },
  };
  runInNewContext(source, {
    URL,
    URLSearchParams,
    document,
    window: {
      location: {
        hostname,
        pathname: `/streamer/${page}/`,
        search,
        origin: `https://${hostname}`,
      },
    },
  });
  return { document, nodes };
}

for (const page of [
  "faq",
  "vergleich",
  "onboarding",
  "clips",
  "affiliate-portal",
  "vertriebler",
  "v1",
  "v3",
  "v4",
]) {
  test(`${page} metadata preserves German and selects English by host or query`, () => {
    const original = metadata("deutsche-deadlock-community.de", page);
    assert.equal(original.document.title, "Original German title");
    assert.equal(original.document.documentElement.lang, "de");
    assert.equal(original.nodes.size, 0);
    for (const [host, search] of [
      ["dead-lock.de", ""],
      ["www.dead-lock.de", ""],
      ["deutsche-deadlock-community.de", "?lang=en"],
    ]) {
      const result = metadata(host, page, search);
      assert.equal(result.document.documentElement.lang, "en");
      assert.notEqual(result.document.title, original.document.title);
      assert.ok(result.nodes.get('meta[name="description"]'));
      assert.equal(result.nodes.get('meta[property="og:locale"]'), "en_GB");
      assert.ok(
        result.nodes
          .get('link[rel="canonical"]')
          .includes(`/streamer/${page}/`),
      );
    }
    assert.equal(
      metadata("dead-lock.de", page, "?lang=de").document.title,
      "Original German title",
    );
    assert.match(
      readFileSync(new URL(`${page}/index.html`, root), "utf8"),
      /subsite-metadata\.js/,
    );
  });
}
