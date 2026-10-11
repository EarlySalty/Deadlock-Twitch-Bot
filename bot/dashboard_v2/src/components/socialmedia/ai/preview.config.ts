import { readFileSync } from 'node:fs';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { defineConfig, mergeConfig, type ViteDevServer } from 'vite';
import baseConfig from '../../../../vite.config';

const source = readFileSync(new URL('./preview-source.mp4', import.meta.url));
const output = readFileSync(new URL('./preview-output.mp4', import.meta.url));

export default defineConfig(async (environment) => {
  const base = typeof baseConfig === 'function' ? await baseConfig(environment) : await baseConfig;
  return mergeConfig(base, {
    plugins: [{
      name: 'social-ai-local-media-fixtures',
      configureServer(server: ViteDevServer) {
        server.middlewares.use((request: IncomingMessage, response: ServerResponse, next: () => void) => {
          if (environment.mode !== 'preview') return next();
          const pathname = request.url?.split('?')[0];
          const data = pathname === '/twitch/social-media/api/ai/jobs/1101/original' ? source : pathname === '/twitch/social-media/api/ai/jobs/1101/versions/2201/file' ? output : null;
          if (!data) return next();
          const range = request.headers.range?.match(/^bytes=(\d+)-(\d*)$/);
          const start = range ? Number(range[1]) : 0;
          const end = range && range[2] ? Math.min(Number(range[2]), data.length - 1) : data.length - 1;
          if (start > end || start >= data.length) {
            response.writeHead(416, { 'Content-Range': `bytes */${data.length}` });
            response.end();
            return;
          }
          response.writeHead(range ? 206 : 200, {
            'Content-Type': 'video/mp4',
            'Accept-Ranges': 'bytes',
            'Content-Length': end - start + 1,
            ...(range ? { 'Content-Range': `bytes ${start}-${end}/${data.length}` } : {}),
          });
          response.end(data.subarray(start, end + 1));
        });
      },
    }],
  });
});
