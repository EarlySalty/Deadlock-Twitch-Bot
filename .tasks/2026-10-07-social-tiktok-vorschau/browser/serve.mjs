import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = path.dirname(fileURLToPath(import.meta.url));
const worktree = path.resolve(root, '../../..');
const frontend = path.join(worktree, 'bot/dashboard_v2');
const require = createRequire(path.join(frontend, 'package.json'));
const { createServer } = await import(require.resolve('vite'));
const { default: react } = await import(require.resolve('@vitejs/plugin-react'));
const { default: tailwind } = await import(require.resolve('@tailwindcss/vite'));
const server = await createServer({
  configFile: false,
  root,
  envDir: root,
  mode: 'isolated-evidence',
  plugins: [react(), tailwind(), {
    name: 'block-real-api',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (req.url.startsWith('/twitch/')) {
          res.statusCode = 410;
          res.setHeader('Content-Type', 'application/json');
          res.end(JSON.stringify({ error: 'isolated_evidence_no_real_api' }));
        } else next();
      });
    },
  }],
  resolve: {
    alias: {
      '@': path.join(frontend, 'src'),
      'react-dom/client': require.resolve('react-dom/client'),
      'react/jsx-dev-runtime': require.resolve('react/jsx-dev-runtime'),
      'react/jsx-runtime': require.resolve('react/jsx-runtime'),
      'react-dom': require.resolve('react-dom'),
      'react': require.resolve('react'),
      '@tanstack/react-query': path.join(frontend, 'node_modules/@tanstack/react-query/build/modern/index.js'),
    },
    dedupe: ['react', 'react-dom'],
  },
  server: { host: '127.0.0.1', port: 19327, strictPort: true, fs: { allow: [worktree] } },
});
await server.listen();
console.log('Isolierte Fixture: http://127.0.0.1:19327');
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, async () => { await server.close(); process.exit(0); });
