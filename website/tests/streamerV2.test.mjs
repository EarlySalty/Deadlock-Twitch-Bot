import { strict as assert } from "node:assert";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const websiteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const demoSrc = readFileSync(
  join(websiteRoot, "src/components/sections/RaidDemo.tsx"),
  "utf8",
);
const page = readFileSync(
  join(websiteRoot, "src/pages/StreamerNetworkPage.tsx"),
  "utf8",
);

function clipLogins() {
  return [...new Set([...demoSrc.matchAll(/\/clips\/([\w-]+)\.mp4/g)].map((m) => m[1]))];
}

test("v2 baut die Raid-Bühne auf dem echten v1-Clip-Pool auf", () => {
  assert.ok(
    page.includes("@/components/partner-clean/Hero"),
    "v2 rendert nicht den sauberen partner-clean Hero",
  );
  const hero = readFileSync(
    join(websiteRoot, "src/components/partner-clean/Hero.tsx"),
    "utf8",
  );
  assert.ok(
    hero.includes("@/components/sections/RaidDemo"),
    "der Hero bindet die bestehende RaidDemo nicht ein",
  );
});

test("Clip-Pool: für jeden Clip liegen mp4 und Profilbild in public/", () => {
  const logins = clipLogins();
  assert.ok(logins.length >= 6, `Clip-Pool zu klein: ${logins.length} Clips`);
  for (const login of logins) {
    for (const rel of [`public/clips/${login}.mp4`, `public/clips/pfp/${login}.png`]) {
      assert.ok(existsSync(join(websiteRoot, rel)), `${rel} fehlt für Clip ${login}`);
    }
  }
});

test("die Streamer-Landing v2 trägt den Community-Markennamen", () => {
  const nav = readFileSync(
    join(websiteRoot, "src/components/layout/Navbar.tsx"),
    "utf8",
  );
  const html = readFileSync(join(websiteRoot, "v2/index.html"), "utf8");
  assert.ok(nav.includes("Deutsche Deadlock Community"), "Nav nennt die Community nicht");
  assert.match(
    html,
    /<title>[^<]*Deadlock Partner Netzwerk[^<]*<\/title>/,
    "der Seitentitel nennt das Partner-Netzwerk nicht",
  );
});
