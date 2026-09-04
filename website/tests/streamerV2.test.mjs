import { strict as assert } from "node:assert";
import { existsSync, readFileSync } from "node:fs";
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

test("die Streamer-Landing v2 trägt den Community-Markennamen", () => {
  const nav = readFileSync(
    join(websiteRoot, "src/components/v2/NetworkChrome.tsx"),
    "utf8",
  );
  const partnerNav = readFileSync(
    join(websiteRoot, "src/components/partner/PartnerNav.tsx"),
    "utf8",
  );
  const html = readFileSync(join(websiteRoot, "v2/index.html"), "utf8");
  assert.ok(nav.includes("Deutsche Deadlock Community"), "Nav nennt die Community nicht");
  assert.ok(partnerNav.includes("PARTNER_COPY.brand"), "Partner-Nav hängt die Marke nicht ein");
  assert.match(
    html,
    /<title>[^<]*Deadlock Partner Netzwerk[^<]*<\/title>/,
    "der Seitentitel nennt das Partner-Netzwerk nicht",
  );
});
