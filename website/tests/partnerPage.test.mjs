import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = fileURLToPath(new URL("..", import.meta.url));
const copyFile = `${root}/src/data/partnerPage.ts`;
const pageFile = `${root}/src/components/partner/PartnerPage.tsx`;
const appFile = `${root}/src/App.tsx`;
const htmlFile = `${root}/index.html`;

const copy = readFileSync(copyFile, "utf8");
const page = readFileSync(pageFile, "utf8");
const app = readFileSync(appFile, "utf8");
const html = readFileSync(htmlFile, "utf8");

function exportedList(name) {
  const match = copy.match(new RegExp(`export const ${name} = \\[([\\s\\S]*?)\\] as const;`));
  assert.ok(match, `${name} fehlt`);
  return [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]);
}

test("die Landing hängt die Partner-Seite ein", () => {
  assert.match(app, /PartnerPage/);
});

test("genau sechs Sektionen, in der Contract-Reihenfolge", () => {
  const sections = exportedList("PARTNER_SECTIONS");
  assert.deepEqual(sections, [
    "hero",
    "problem",
    "bedeutung",
    "partner",
    "sicherheit",
    "abschluss",
  ]);
  for (const id of sections) {
    assert.match(page, new RegExp(`id="${id}"`));
  }
});

test("Hero-CTA bleibt der bestehende OAuth-Start", () => {
  assert.match(page, /buildTwitchBotAuthUrl/);
  assert.match(page, /DISCORD_INVITE_URL/);
  assert.match(page, /TWITCH_SECURITY_URL/);
});

test("Title und Meta kommen aus der Copy-Datei", () => {
  assert.match(html, /Deadlock Partner Netzwerk - Deutsche Deadlock Community/);
  assert.match(
    html,
    /Werde Partner der deutschen Deadlock Community\. Automatische Raids/,
  );
  assert.doesNotMatch(html, /SoftwareApplication/);
  assert.doesNotMatch(html.toLowerCase(), /kostenlos verbinden/);
  assert.doesNotMatch(html.toLowerCase(), /wachstums-netzwerk/);
});

test("Verkaufsverbote stehen nicht in der sichtbaren Copy", () => {
  const forbidden = exportedList("PARTNER_FORBIDDEN");
  const block = copy.match(/export const PARTNER_COPY = \{([\s\S]*?)\} as const;/);
  assert.ok(block, "PARTNER_COPY fehlt");
  const visible = block[1].toLowerCase();

  for (const word of forbidden) {
    assert.equal(
      visible.includes(word),
      false,
      `verbotenes Muster in der Copy: ${word}`,
    );
  }
});
