import { strict as assert } from "node:assert";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

const root = new URL("..", import.meta.url).pathname;
const pagePath = join(root, "src/pages/StreamerComparisonPage.tsx");
const entryPath = join(root, "src/streamer-comparison.tsx");
const htmlPath = join(root, "vergleich/index.html");

test("der öffentliche Streamer-Vergleich besitzt einen gebauten Einstieg", () => {
  assert.ok(existsSync(htmlPath), "vergleich/index.html fehlt");
  assert.ok(existsSync(entryPath), "src/streamer-comparison.tsx fehlt");

  const viteConfig = readFileSync(join(root, "vite.config.ts"), "utf8");
  assert.match(viteConfig, /vergleich\/index\.html/);
});

test("die Vergleichsseite nutzt nur den aggregierten Public-Endpoint", () => {
  assert.ok(existsSync(pagePath), "StreamerComparisonPage.tsx fehlt");
  const source = readFileSync(pagePath, "utf8");

  assert.match(source, /\/twitch\/api\/v2\/public\/streamer-comparison/);
  assert.match(source, /Methodik/);
  assert.match(source, /Datenqualität/);

  for (const privateMetric of ["revenue", "earnings", "subscriber", "discordUserId"]) {
    assert.ok(
      !source.toLowerCase().includes(privateMetric.toLowerCase()),
      `private Metrik darf nicht gerendert werden: ${privateMetric}`,
    );
  }
});

test("Sortierung stellt belastbar gerankte Werte vor kleine Stichproben", () => {
  const source = readFileSync(pagePath, "utf8");

  assert.match(source, /viewerHours:\s*"viewerHours"/);
  assert.match(source, /averageViewers:\s*"averageViewers"/);
  assert.match(source, /growth:\s*"momentum"/);
  assert.match(source, /raidImpact:\s*"raidImpact"/);
  assert.match(source, /leftRanked\s*!==\s*rightRanked/);
});

test("Suche und Berechnungszeit sind explizit zugänglich", () => {
  const source = readFileSync(pagePath, "utf8");

  assert.match(source, /aria-label="Streamer suchen"/);
  assert.match(source, /timeZone:\s*"Europe\/Berlin"/);
  assert.match(source, /aria-pressed=\{days === option\}/);
});

test("ein ungültiger Kanalparameter wird auf die sichtbare Auswahl normalisiert", () => {
  const source = readFileSync(pagePath, "utf8");

  assert.match(source, /url\.searchParams\.set\("streamer", nextLogin\)/);
  assert.match(source, /url\.searchParams\.delete\("streamer"\)/);
});
