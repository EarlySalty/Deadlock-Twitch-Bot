import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import test from "node:test";

const root = fileURLToPath(new URL("..", import.meta.url));
const pageFile = `${root}/src/pages/StreamerNetworkPage.tsx`;
const appFile = `${root}/src/App.tsx`;
const htmlFile = `${root}/v2/index.html`;
const cleanDir = `${root}/src/components/partner-clean`;

const page = readFileSync(pageFile, "utf8");
const app = readFileSync(appFile, "utf8");
const html = readFileSync(htmlFile, "utf8");

const FORBIDDEN = [
  "kostenlos verbinden",
  "leistungen",
  "wachstums-netzwerk",
  "kanal-report",
  "was du bekommst",
  "drei schritte",
  "module",
];

test("die v1-Landing bleibt die bestehende Seite", () => {
  assert.match(app, /from '@\/components\/sections\/Hero'/);
  assert.doesNotMatch(app, /partner-clean/);
  assert.doesNotMatch(app, /StreamerNetworkPage/);
});

test("v2 rendert die saubere Partner-Komposition in v1-Reihenfolge", () => {
  const order = [
    "<GlowOrb",
    "<Navbar",
    "<Hero",
    "<StreamDay",
    "<RaidExplainer",
    "<BanFeed",
    "<Stats",
    "<Features",
    "<ClipManager",
    "<Community",
    "<Security",
    "<CTA",
    "<Footer",
  ];
  let previous = -1;
  for (const marker of order) {
    const at = page.indexOf(marker);
    assert.ok(at !== -1, `Baustein fehlt in der Komposition: ${marker}`);
    assert.ok(at > previous, `Baustein steht nicht in v1-Reihenfolge: ${marker}`);
    previous = at;
  }
});

test("v2 nutzt keine Network*- oder alten partner/-Bausteine mehr", () => {
  assert.doesNotMatch(page, /components\/v2\//);
  assert.doesNotMatch(page, /components\/partner\//);
  assert.match(page, /components\/partner-clean\//);
});

test("Hero-CTA bleibt der bestehende OAuth-Start plus Discord", () => {
  const hero = readFileSync(`${cleanDir}/Hero.tsx`, "utf8");
  assert.match(hero, /buildTwitchBotAuthUrl/);
  assert.match(hero, /DISCORD_INVITE_URL/);
  assert.match(hero, /Jetzt Partner werden/);
});

test("Title und Meta bleiben Partner-Netzwerk mit noindex", () => {
  assert.match(
    html,
    /Deadlock Partner Netzwerk - Auto-Raid & Streamer Community \(Deutsch\)/,
  );
  assert.match(html, /noindex, nofollow/);
});

test("Verkaufsverbote stehen nicht in der sichtbaren Partner-Copy", () => {
  const files = readdirSync(cleanDir).filter((f) => f.endsWith(".tsx"));
  assert.ok(files.length >= 10, `partner-clean unerwartet leer: ${files.length}`);
  const visible = files
    .map((f) => readFileSync(join(cleanDir, f), "utf8"))
    .join("\n")
    .toLowerCase();
  for (const word of FORBIDDEN) {
    assert.equal(
      visible.includes(word),
      false,
      `verbotenes Muster in der Copy: ${word}`,
    );
  }
});
