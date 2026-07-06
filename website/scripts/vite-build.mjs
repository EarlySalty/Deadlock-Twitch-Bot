import { build } from "vite";

// ponytail: vite-prerender-plugin leaves an open handle after a successful Vite build.
await build();
process.exit(0);
