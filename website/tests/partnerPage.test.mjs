import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import test from "node:test";

const root = fileURLToPath(new URL("..", import.meta.url));
const pageFile = `${root}/src/pages/StreamerNetworkPage.tsx`;
const appFile = `${root}/src/App.tsx`;
const htmlFile = `${root}/index.html`;
const v1HtmlFile = `${root}/v1/index.html`;
const cleanDir = `${root}/src/components/partner-clean`;

const page = readFileSync(pageFile, "utf8");
const app = readFileSync(appFile, "utf8");
const html = readFileSync(htmlFile, "utf8");

const EMPTY_STATE_TEXT =
  "Die Partnerliste lädt gerade nicht. Schau auf Twitch oder im Discord vorbei.";

const FORBIDDEN_SUBSTRINGS = [
  "kostenlos verbinden",
  "leistungen",
  "wachstums-netzwerk",
  "kanal-report",
  "was du bekommst",
  "drei schritte",
  "module",
  "dashboard mit demo-daten",
  "demo-daten",
  "alle funktionen",
  "funktionen im vergleich",
  "jetzt testen",
  "pricing",
  "tarif",
  "saas",
  "software",
  "produkt",
  "preismodell",
  "preisliste",
];

const FORBIDDEN_WORDS = ["plan", "tool"];

function cleanFiles() {
  return readdirSync(cleanDir).filter((f) => f.endsWith(".tsx"));
}

test("die v1-Landing bleibt die bestehende Seite", () => {
  assert.match(app, /from '@\/components\/sections\/Hero'/);
  assert.doesNotMatch(app, /partner-clean/);
  assert.doesNotMatch(app, /StreamerNetworkPage/);
});

test("v2 rendert Partner-Block und Partner-Übersicht direkt unter dem Hero", () => {
  const order = [
    "<GlowOrb",
    "<Navbar",
    "<Hero",
    "<PartnerPitch",
    "<PartnerNetwork",
    "<RaidExplainer",
    "<BanFeed",
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
    assert.ok(at > previous, `Baustein steht nicht in Soll-Reihenfolge: ${marker}`);
    previous = at;
  }
});

test("die nummerierten Ablauf-Karten sind weg (StreamDay geloescht)", () => {
  assert.equal(
    existsSync(join(cleanDir, "StreamDay.tsx")),
    false,
    "StreamDay.tsx muss geloescht sein",
  );
  assert.doesNotMatch(page, /StreamDay/, "StreamDay wird noch importiert oder gerendert");
});

test("keine Sektionsnummern 01/02/03 in der Partner-Copy", () => {
  for (const f of cleanFiles()) {
    const src = readFileSync(join(cleanDir, f), "utf8");
    assert.doesNotMatch(
      src,
      /["'`]0[123]["'`]/,
      `Sektionsnummer als Literal in ${f}`,
    );
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

test("index.html ist die indexierbare Partner-Landing", () => {
  assert.match(html, /data-theme="v2"/);
  assert.match(html, /\/src\/streamer-v2\.tsx/);
  assert.match(html, /Deadlock Partner-Netzwerk/);
  assert.match(html, /name="robots" content="index, follow/);
  assert.doesNotMatch(html, /noindex/);
  assert.match(
    html,
    /rel="canonical" href="https:\/\/deutsche-deadlock-community\.de\/streamer\/"/,
  );
});

test("v1/index.html ist die alte Landing mit noindex", () => {
  const v1 = readFileSync(v1HtmlFile, "utf8");
  assert.match(v1, /\/src\/main\.tsx/);
  assert.match(v1, /noindex, nofollow/);
  assert.match(
    v1,
    /rel="canonical" href="https:\/\/deutsche-deadlock-community\.de\/streamer\/"/,
  );
});

test("v2/index.html existiert nicht mehr", () => {
  assert.equal(existsSync(join(root, "v2/index.html")), false);
});

test("kein SaaS-Vokabular in der sichtbaren Partner-Copy", () => {
  const files = cleanFiles();
  assert.ok(files.length >= 10, `partner-clean unerwartet leer: ${files.length}`);
  const combined = files
    .map((f) => readFileSync(join(cleanDir, f), "utf8"))
    .join("\n");
  const visible = combined.toLowerCase();
  for (const word of FORBIDDEN_SUBSTRINGS) {
    assert.equal(
      visible.includes(word),
      false,
      `verbotenes Muster in der Copy: ${word}`,
    );
  }
  for (const word of FORBIDDEN_WORDS) {
    assert.doesNotMatch(
      combined,
      new RegExp(`\\b${word}\\b`, "i"),
      `verbotenes Wort in der Copy: ${word}`,
    );
  }
});

test("PartnerNetwork zeigt echte Partner mit Twitch-Link und ehrlichem Leerzustand", () => {
  const netFile = join(cleanDir, "PartnerNetwork.tsx");
  assert.ok(existsSync(netFile), "PartnerNetwork.tsx fehlt");
  const net = readFileSync(netFile, "utf8");
  assert.match(net, /twitch\.tv\//, "kein Link auf das Twitch-Profil");
  assert.match(net, /player\.twitch\.tv/, "kein Twitch-Live-Embed");
  assert.match(net, /target="_blank"/, "Profil-Link oeffnet keinen neuen Tab");
  assert.match(net, /rel="noopener/, "Profil-Link ohne noopener");
  assert.ok(
    net.includes(EMPTY_STATE_TEXT),
    "ehrlicher Leerzustand-Text fehlt",
  );
  assert.doesNotMatch(
    net,
    /\blogin:\s*["'][A-Za-z0-9_]+["']/,
    "hart codierte Partner-Logins gefunden",
  );
});

test("PartnerPitch existiert als eigene Sektion", () => {
  assert.ok(
    existsSync(join(cleanDir, "PartnerPitch.tsx")),
    "PartnerPitch.tsx fehlt",
  );
});
