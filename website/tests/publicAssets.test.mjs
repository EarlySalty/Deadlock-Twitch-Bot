import { strict as assert } from "node:assert";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

/*
 * Jedes im Code referenzierte /streamer/-Asset muss in public/ liegen.
 *
 * Vite kopiert public/ unveraendert nach dist/ — ein vertippter Pfad in
 * src/ faellt weder Build noch Typecheck auf und wird erst live zum 404
 * (das Bild fehlt einfach). Gleiche Bug-Klasse wie der FAQ-Entry-404:
 * verdrahtet, aber nie gebaut. /streamer/assets/ ist ausgenommen, das
 * erzeugt der Build selbst.
 */

const websiteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const publicDir = join(websiteRoot, "public");

const ASSET_REF = /\/streamer\/((?!assets\/)[\w./-]+\.(?:svg|png|webp|jpe?g|gif|ico|woff2?))/g;

function* sourceFiles(dir) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (["node_modules", "dist", "public"].includes(entry) || entry.startsWith(".")) continue;
      yield* sourceFiles(full);
    } else if (/\.(tsx?|css|html)$/.test(entry)) {
      yield full;
    }
  }
}

test("jedes referenzierte /streamer/-Asset existiert in public/", () => {
  let found = 0;
  for (const file of sourceFiles(websiteRoot)) {
    const content = readFileSync(file, "utf8");
    for (const match of content.matchAll(ASSET_REF)) {
      found += 1;
      const assetPath = join(publicDir, match[1]);
      assert.ok(
        existsSync(assetPath),
        `${file} referenziert /streamer/${match[1]}, aber public/${match[1]} fehlt`,
      );
    }
  }
  // Selbsttest des Waechters: findet er nichts, ist die Regex kaputt,
  // nicht der Baum sauber.
  assert.ok(found > 0, "keine einzige Asset-Referenz gefunden — Regex pruefen");
});
