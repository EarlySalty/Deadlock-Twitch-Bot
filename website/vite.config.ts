import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { vitePrerenderPlugin } from 'vite-prerender-plugin'
import path from 'path'

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    vitePrerenderPlugin({
      renderTarget: '#root',
      additionalPrerenderRoutes: ['/'],
    }),
    {
      name: 'geteilte-bausteine',
      enforce: 'post',
      configResolved(resolved: unknown) {
        const config = resolved as {
          build?: { rollupOptions?: { output?: { manualChunks?: unknown } } };
        };
        const output = config.build?.rollupOptions?.output;
        const vorher = output?.manualChunks;
        if (typeof vorher !== 'function') return;
        output.manualChunks = (id: string, ...rest: unknown[]) => {
          if (id.includes('vite/preload-helper') || id.includes('vite/modulepreload-polyfill')) {
            return 'gemeinsam';
          }
          const plugin = (vorher as (id: string, ...r: unknown[]) => string)(
            id,
            ...rest,
          );
          if (plugin) return plugin;
          if (
            (id.includes('/src/') || id.includes('/node_modules/')) &&
            !/\/src\/[^/]+\.(tsx|ts|mjs|jsx|js)$/.test(id)
          ) {
            return 'gemeinsam';
          }
          return undefined;
        };
      },
    },
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  base: '/streamer/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, 'index.html'),
        streamerV1: path.resolve(__dirname, 'v1/index.html'),
        streamerV3: path.resolve(__dirname, 'v3/index.html'),
        affiliateProgram: path.resolve(__dirname, 'vertriebler/index.html'),
        affiliatePortal: path.resolve(__dirname, 'affiliate-portal/index.html'),
        onboarding: path.resolve(__dirname, 'onboarding/index.html'),
        streamerComparison: path.resolve(__dirname, 'vergleich/index.html'),
        // Caddy serviert /twitch/faq* aus dist/faq — der Entry MUSS faq/index.html
        // heissen, sonst zeigt die Route weiter ins Leere (genau das war der 404).
        faq: path.resolve(__dirname, 'faq/index.html'),
      },
    },
  },
})
