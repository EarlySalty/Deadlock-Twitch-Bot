import { strict as assert } from "node:assert";
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { parseFragment } from "parse5";

/*
 * Jede Unterseite braucht einen Vite-Entry — sonst wird sie nie gebaut.
 *
 * Warum es diesen Test gibt: /twitch/faq war in Caddy sauber verdrahtet
 * (handle /twitch/faq* -> root website/dist/faq) und aus der Dashboard-Sidebar
 * verlinkt — aber es gab weder faq/index.html noch einen rollup-Input dafuer.
 * Das Verzeichnis dist/faq entstand nie, Caddy fand nichts, der Link lieferte
 * monatelang 404. Kein Build, kein Typecheck und kein Linter schlaegt dabei an:
 * eine Seite, die niemand baut, kann auch nicht fehlschlagen.
 *
 * Der Test schliesst die Luecke von beiden Seiten:
 *   - HTML ohne Entry  -> wird nicht gebaut  (der Bug von oben)
 *   - Entry ohne HTML  -> Build bricht ab
 */

const websiteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

/** Verzeichnisse im Website-Root, die eine eigene Einstiegs-HTML tragen. */
function pageDirsWithHtml() {
  const skip = new Set(["node_modules", "dist", "src", "public", "tests", "scripts"]);
  return readdirSync(websiteRoot).filter((entry) => {
    if (skip.has(entry) || entry.startsWith(".")) return false;
    const full = join(websiteRoot, entry);
    return statSync(full).isDirectory() && existsSync(join(full, "index.html"));
  });
}

/*
 * Die tatsaechlich deklarierten Entries — NICHT per `config.includes("faq/index.html")`.
 * Diese Datei enthaelt Kommentare, in denen Pfade woertlich vorkommen; eine reine
 * Substring-Suche findet den Pfad dann auch dann noch, wenn der Entry laengst
 * geloescht ist, und der Test meldet froehlich gruen. (Genau so verhielt er sich
 * im ersten Entwurf — aufgefallen erst, als der Negativ-Beweis ausblieb.)
 * `path.resolve(...)` steht nur in echtem Code.
 */
function declaredEntries() {
  const config = readFileSync(join(websiteRoot, "vite.config.ts"), "utf8");
  return [...config.matchAll(/path\.resolve\(\s*__dirname\s*,\s*['"]([^'"]+)['"]\s*\)/g)]
    .map((match) => match[1])
    .filter((rel) => rel.endsWith("index.html"));
}

test("jede Einstiegs-HTML ist als Vite-Entry registriert (sonst wird sie nie gebaut)", () => {
  const entries = declaredEntries();
  assert.ok(entries.length > 0, "vite.config.ts deklariert keine Entries — Regex kaputt?");

  const missing = pageDirsWithHtml().filter(
    (dir) => !entries.includes(`${dir}/index.html`),
  );

  assert.deepEqual(
    missing,
    [],
    `Diese Seiten haben eine index.html, aber keinen rollup-Input in vite.config.ts.\n` +
      `Sie landen nicht in dist/ und liefern im Betrieb 404:\n  ${missing.join("\n  ")}`,
  );
});

test("jeder Vite-Entry zeigt auf eine existierende index.html", () => {
  const broken = declaredEntries().filter((rel) => !existsSync(join(websiteRoot, rel)));
  assert.deepEqual(broken, [], `Vite-Entry zeigt ins Leere:\n  ${broken.join("\n  ")}`);
});

test("die Empfangsseite /twitch/faq wird gebaut (Caddy serviert dist/faq)", () => {
  // Namentlich festgenagelt: Caddy sucht genau dieses Verzeichnis. Wird der
  // Entry umbenannt oder entfernt, faellt der FAQ-Link zurueck in den 404.
  assert.ok(
    existsSync(join(websiteRoot, "faq/index.html")),
    "faq/index.html fehlt — /twitch/faq liefert wieder 404",
  );
  assert.ok(
    declaredEntries().includes("faq/index.html"),
    "vite.config.ts hat keinen faq-Entry — dist/faq entsteht nicht",
  );
});

function beaconScripts(html) {
  const pending = [parseFragment(html)];
  const sources = [];
  while (pending.length > 0) {
    const node = pending.pop();
    if (node.tagName === "script") {
      const source = node.attrs.find((attribute) => attribute.name === "src")?.value;
      if (source) {
        const url = new URL(source, "https://website.example");
        if (url.hostname === "static.cloudflareinsights.com"
          || url.hostname.endsWith(".cloudflareinsights.com")
          || url.pathname.split("/").at(-1) === "beacon.min.js") sources.push(source);
      }
    }
    pending.push(...(node.childNodes ?? []));
    if (node.content) pending.push(node.content);
  }
  return sources;
}

test("Beacon-Prüfung erkennt echte Skriptquellen statt Kommentare oder Teilzeichenfolgen", () => {
  for (const html of [
    '<script src="https://static.cloudflareinsights.com/anything.js"></script>',
    '<SCRIPT SRC="//STATIC.CLOUDFLAREINSIGHTS.COM/anything.js"></SCRIPT>',
    '<script src="https:&#47;&#47;static.cloudflareinsights.com/anything.js"></script>',
    '<script src="/assets/beacon.min.js?version=1"></script>',
    '<template><script src="/beacon.min.js"></script></template>',
  ]) assert.equal(beaconScripts(html).length, 1);
  for (const html of [
    '<script src="/assets/main.js"></script>',
    '<!-- <script src="/beacon.min.js"></script> -->',
    '<p>static.cloudflareinsights.com</p>',
    '<script src="/main.js?note=static.cloudflareinsights.com"></script>',
    '<script src="https://static.cloudflareinsights.com.example/main.js"></script>',
  ]) assert.deepEqual(beaconScripts(html), []);
});

test("keine Streamer-Seite bindet den manuellen Cloudflare-Beacon ein", () => {
  const htmlFiles = ["index.html", ...pageDirsWithHtml().map((dir) => `${dir}/index.html`)];
  const offenders = htmlFiles.filter((rel) => {
    const html = readFileSync(join(websiteRoot, rel), "utf8");
    return beaconScripts(html).length > 0;
  });
  assert.deepEqual(
    offenders,
    [],
    `Externe Beacon-Skripte ohne SRI/CSP-Freigabe gefunden:\n  ${offenders.join("\n  ")}`,
  );
});
