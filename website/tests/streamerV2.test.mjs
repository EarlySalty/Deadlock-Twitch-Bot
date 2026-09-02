import { strict as assert } from "node:assert";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const websiteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const demoSrc = readFileSync(
  join(websiteRoot, "src/components/v2/NetworkRaidDemo.tsx"),
  "utf8",
);

function clipLogins() {
  return [...new Set([...demoSrc.matchAll(/\/clips\/([\w-]+)\.mp4/g)].map((m) => m[1]))];
}

test("Clip-Pool: für jeden Clip liegen mp4, Poster und Profilbild in public/", () => {
  const logins = clipLogins();
  assert.ok(logins.length >= 6, `Clip-Pool zu klein: ${logins.length} Clips`);
  for (const login of logins) {
    for (const rel of [
      `public/clips/${login}.mp4`,
      `public/clips/poster/${login}.jpg`,
      `public/clips/pfp/${login}.png`,
    ]) {
      assert.ok(existsSync(join(websiteRoot, rel)), `${rel} fehlt für Clip ${login}`);
    }
  }
});

test("die Bühne fällt auf den echten Clip-Pool zurück, nicht auf ausgedachte Namen", () => {
  assert.ok(!/FALLBACK_CHANNELS/.test(demoSrc), "FALLBACK_CHANNELS ist wieder im Code");
  for (const name of ["dein_kanal", "ein_anderer_stream", "partnerkanal", "naechster_partner"]) {
    assert.ok(!demoSrc.includes(name), `ausgedachter Kanalname noch im Code: ${name}`);
  }
  assert.ok(/CLIP_POOL/.test(demoSrc), "CLIP_POOL als Rückfallebene fehlt");
});

function v2Files() {
  const root = join(websiteRoot, "src/components/v2");
  const walk = (dir) =>
    readdirSync(dir).flatMap((entry) => {
      const path = join(dir, entry);
      if (statSync(path).isDirectory()) return walk(path);
      return /\.tsx?$/.test(entry) ? [path] : [];
    });
  return walk(root);
}

test("der Clip-Pool trägt keine erfundenen Zuschauerzahlen und der Zähler hängt an sample", () => {
  const block = demoSrc.match(/const CLIP_POOL[^=]*=\s*\[([\s\S]*?)\];/);
  assert.ok(block, "CLIP_POOL-Block nicht gefunden");
  const viewers = [...block[1].matchAll(/viewers:\s*(\d+)/g)].map((m) => Number(m[1]));
  assert.ok(viewers.length >= 6, `zu wenige Clip-Eintraege: ${viewers.length}`);
  for (const v of viewers) {
    assert.equal(v, 0, `Clip-Karte trägt eine erfundene Zuschauerzahl: ${v}`);
  }
  assert.match(
    demoSrc,
    /if\s*\(!\w+\.sample\)\s*\{[\s\S]*?animateCounter\([\s\S]*?counterNumRef/,
    "der Zähler animiert auch für Clip-Karten (nicht an sample gebunden)",
  );
});

test("die Nav führt in der Partner-Reihenfolge", () => {
  const nav = readFileSync(
    join(websiteRoot, "src/components/v2/NetworkChrome.tsx"),
    "utf8",
  );
  const block = nav.match(/const NAV_ITEMS\s*=\s*\[([\s\S]*?)\];/);
  assert.ok(block, "NAV_ITEMS nicht gefunden");
  const labels = [...block[1].matchAll(/label:\s*"([^"]*)"/g)].map((m) => m[1]);
  assert.deepEqual(labels, [
    "Partner",
    "So funktioniert's",
    "Zahlen",
    "Sicherheit",
    "FAQ",
  ]);
});

test("kein Verbinden- oder Report-Knopftext mehr in v2 und den Plandaten", () => {
  const scanned = [...v2Files(), join(websiteRoot, "src/data/networkPage.ts")];
  for (const file of scanned) {
    const text = readFileSync(file, "utf8");
    for (const banned of ["Kostenlos verbinden", "Jetzt kostenlos verbinden", "Kanal-Report holen"]) {
      assert.ok(!text.includes(banned), `${banned} steht noch in ${file}`);
    }
  }
});

test("die Hero-Headline macht aus dem Besucher einen Partner", () => {
  const hero = readFileSync(
    join(websiteRoot, "src/components/v2/NetworkHero.tsx"),
    "utf8",
  );
  assert.ok(
    hero.includes("Werde Partner der deutschen"),
    "Hero-Headline nennt die Partnerschaft nicht",
  );
  assert.ok(
    hero.includes("Deadlock-Community."),
    "Hero-Headline nennt die Community nicht",
  );
});

test("die Streamer-Landing v2 trägt den Community-Markennamen", () => {
  const nav = readFileSync(
    join(websiteRoot, "src/components/v2/NetworkChrome.tsx"),
    "utf8",
  );
  const html = readFileSync(join(websiteRoot, "v2/index.html"), "utf8");
  assert.ok(nav.includes("Deutsche Deadlock Community"), "Nav nennt die Community nicht");
  assert.match(
    html,
    /<title>[^<]*Deutsche Deadlock Community[^<]*<\/title>/,
    "der Seitentitel nennt die Community nicht",
  );
});
