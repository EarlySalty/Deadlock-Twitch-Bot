import { build, createServer } from "vite";
import fs from "node:fs";
import { createElement, StrictMode } from "react";
import { renderToString } from "react-dom/server";

// ponytail: vite-prerender-plugin leaves an open handle after a successful Vite build.
await build();

const server = await createServer({
  server: { middlewareMode: true },
  appType: "custom",
  logLevel: "error",
});

try {
  for (const [page, file] of [
    ["StreamerNetworkV3Page", "dist/v3/index.html"],
    ["StreamerNetworkV4Page", "dist/v4/index.html"],
  ]) {
    const mod = await server.ssrLoadModule(`/src/pages/${page}.tsx`);
    const html = renderToString(
      createElement(StrictMode, null, createElement(mod[page])),
    );
    const template = fs.readFileSync(file, "utf-8");
    fs.writeFileSync(
      file,
      template.replace('<div id="root"></div>', `<div id="root">${html}</div>`),
    );
  }
} finally {
  await server.close();
}

process.exit(0);
