import { strict as assert } from "node:assert";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

/*
 * Englische Fassung fuer dead-lock.de (Plan 4.10, Paket W).
 *
 * Der EN-Build tauscht den Block zwischen `<!-- i18n:head -->` und
 * `<!-- /i18n:head -->` gegen `i18n-en/<seite>.head.html`. Diese Tests halten
 * beide Seiten des Vertrags zusammen, ohne einen Build zu brauchen.
 */

const websiteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => readFileSync(join(websiteRoot, rel), "utf8");

const PAGES = [
  { html: "index.html", head: "i18n-en/index.head.html", en: "https://dead-lock.de/streamer/", de: "https://deutsche-deadlock-community.de/streamer/" },
  { html: "onboarding/index.html", head: "i18n-en/onboarding.head.html", en: "https://dead-lock.de/twitch/onboarding", de: "https://deutsche-deadlock-community.de/twitch/onboarding" },
  { html: "faq/index.html", head: "i18n-en/faq.head.html", en: "https://dead-lock.de/twitch/faq", de: "https://deutsche-deadlock-community.de/twitch/faq" },
];

test("vite.config.ts baut im EN-Modus genau Landing, Onboarding und FAQ", () => {
  const config = read("vite.config.ts");
  assert.match(config, /EN_PAGES = \['main', 'onboarding', 'faq'\]/);
  assert.match(config, /siteLang === 'en' \? 'dist-en' : 'dist'/);
});

for (const page of PAGES) {
  test(`${page.html}: deutsche Fassung verweist per hreflang auf die englische`, () => {
    const html = read(page.html);
    assert.match(html, /<html lang="de"/);
    assert.ok(html.includes("<!-- i18n:head -->"), "Startmarker fehlt");
    assert.ok(html.includes("<!-- /i18n:head -->"), "Endmarker fehlt");
    assert.ok(html.includes(`hreflang="en" href="${page.en}"`), "hreflang en fehlt");
    assert.ok(html.includes(`hreflang="de" href="${page.de}"`), "hreflang de fehlt");
  });

  test(`${page.head}: englischer Kopf ist vollstaendig und wechselseitig verknuepft`, () => {
    assert.ok(existsSync(join(websiteRoot, page.head)), `${page.head} fehlt`);
    const head = read(page.head);
    assert.ok(head.includes(`<link rel="canonical" href="${page.en}" />`), "canonical muss auf dead-lock.de zeigen");
    assert.ok(head.includes(`hreflang="en" href="${page.en}"`));
    assert.ok(head.includes(`hreflang="de" href="${page.de}"`));
    assert.match(head, /<title>[^<]+<\/title>/);
    assert.match(head, /name="description"/);
    assert.doesNotMatch(head, /[äöüÄÖÜ]|Deutsche Deadlock Community|Partner werden/, "deutscher Text im EN-Kopf");
  });
}
