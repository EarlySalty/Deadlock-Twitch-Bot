import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = fileURLToPath(new URL("..", import.meta.url));
const copyFile = `${root}/src/data/partnerPage.ts`;
const pageFile = `${root}/src/components/partner/PartnerPage.tsx`;
const cardFile = `${root}/src/components/partner/StreamCard.tsx`;
const v2Page = `${root}/src/pages/StreamerNetworkPage.tsx`;
const appFile = `${root}/src/App.tsx`;
const htmlFile = `${root}/v2/index.html`;

const networkCopyFile = `${root}/src/data/networkPage.ts`;

const copy = readFileSync(copyFile, "utf8");
const page = readFileSync(pageFile, "utf8");
const card = readFileSync(cardFile, "utf8");
const v2 = readFileSync(v2Page, "utf8");
const app = readFileSync(appFile, "utf8");
const html = readFileSync(htmlFile, "utf8");
const networkCopy = readFileSync(networkCopyFile, "utf8");

function exportedList(name) {
  const match = copy.match(new RegExp(`export const ${name} = \\[([\\s\\S]*?)\\] as const;`));
  assert.ok(match, `${name} fehlt`);
  return [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]);
}

test("die v1-Landing bleibt die bestehende Seite", () => {
  assert.match(app, /from '@\/components\/sections\/Hero'/);
  assert.doesNotMatch(app, /PartnerPage/);
});

test("v2 hängt die Partner-Seite ein", () => {
  assert.match(v2, /PartnerPage/);
});

test("die Partner-Netzwerk-Sektionen stehen in der Contract-Reihenfolge", () => {
  const sections = exportedList("PARTNER_SECTIONS");
  assert.deepEqual(sections, [
    "hero",
    "partner",
    "leere",
    "netzwerk",
    "spamschutz",
    "sicherheit",
    "abschluss",
  ]);

  const order = [
    "<NetworkHero",
    "<PartnersSection",
    "<VoidSection",
    "<PartnerValuesSection",
    "<PartnerBanFeedSection",
    "<NetworkSecuritySection",
    'id="abschluss"',
  ];
  let previous = -1;
  for (const marker of order) {
    const at = page.indexOf(marker);
    assert.ok(at !== -1, `Baustein fehlt in der Komposition: ${marker}`);
    assert.ok(
      at > previous,
      `Baustein steht nicht in Contract-Reihenfolge: ${marker}`,
    );
    previous = at;
  }
});

test("Hero-CTA bleibt der bestehende OAuth-Start", () => {
  assert.match(page, /buildTwitchBotAuthUrl/);
  assert.match(page, /DISCORD_INVITE_URL/);
  assert.match(page, /TWITCH_SECURITY_URL/);
});

test("Live-Karten nutzen Vorschaubilder, keinen Twitch-Player", () => {
  assert.match(card, /static-cdn\.jtvnw\.net\/previews-ttv/);
  assert.doesNotMatch(card, /player\.twitch\.tv/);
  assert.doesNotMatch(card, /<iframe/);
});

test("Title und Meta kommen aus der Copy-Datei", () => {
  assert.match(
    html,
    /Deadlock Partner Netzwerk - Auto-Raid & Streamer Community \(Deutsch\)/,
  );
  assert.match(
    html,
    /Werde Partner der deutschen Deadlock Community\. Automatische Raids/,
  );
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

test("Verkaufsverbote stehen nicht in der sichtbaren Netzwerk-Copy", () => {
  const forbidden = exportedList("PARTNER_FORBIDDEN");
  const blocks = [];
  for (const name of ["HERO_COPY", "VALUES_COPY", "SPAM_COPY"]) {
    const match = networkCopy.match(
      new RegExp(`export const ${name} = \\{([\\s\\S]*?)\\} as const;`),
    );
    assert.ok(match, `${name} fehlt`);
    blocks.push(match[1]);
  }
  const values = networkCopy.match(
    /export const networkValues[\s\S]*?\n\];/,
  );
  assert.ok(values, "networkValues fehlt");
  blocks.push(values[0]);

  const visible = blocks.join("\n").toLowerCase();
  for (const word of forbidden) {
    assert.equal(
      visible.includes(word),
      false,
      `verbotenes Muster in der Netzwerk-Copy: ${word}`,
    );
  }
});
