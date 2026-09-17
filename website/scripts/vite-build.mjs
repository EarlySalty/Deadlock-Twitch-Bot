import { build, createServer } from "vite";
import fs from "node:fs";

// ponytail: vite-prerender-plugin leaves an open handle after a successful Vite build.
await build();

const server = await createServer({
  server: { middlewareMode: true },
  appType: "custom",
  logLevel: "error",
});

try {
  const mod = await server.ssrLoadModule("/src/streamer-v3.tsx");
  const html = await mod.prerender();
  const file = "dist/v3/index.html";
  const template = fs.readFileSync(file, "utf-8");
  fs.writeFileSync(
    file,
    template.replace('<div id="root"></div>', `<div id="root">${html}</div>`),
  );
} finally {
  await server.close();
}

process.exit(0);
