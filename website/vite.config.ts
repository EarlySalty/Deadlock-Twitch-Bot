import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { vitePrerenderPlugin } from 'vite-prerender-plugin'
import path from 'path'
import fs from 'fs'

// `VITE_SITE_LANG=en` baut die englische Fassung fuer dead-lock.de/streamer
// (Plan 4.10). Gleicher Code, andere Sprache, eigener Ausgabeordner.
const siteLang = process.env.VITE_SITE_LANG === 'en' ? 'en' : 'de'

const pageInputs: Record<string, string> = {
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
  clips: path.resolve(__dirname, 'clips/index.html'),
}

// Die englische Fassung umfasst nur die uebersetzten Seiten: Landing,
// Onboarding und FAQ. Alles andere bleibt deutsch auf der Hauptdomain.
const EN_PAGES = ['main', 'onboarding', 'faq'] as const

/*
 * Tauscht im EN-Build den Seitenkopf zwischen `<!-- i18n:head -->` und
 * `<!-- /i18n:head -->` gegen `i18n-en/<seite>.head.html` und setzt
 * `lang="en"`. Fehlt Marker oder Datei, bricht der Build ab, statt still
 * deutsche Meta-Daten unter dead-lock.de auszuliefern.
 */
function englischerSeitenkopf() {
  return {
    name: 'englischer-seitenkopf',
    transformIndexHtml: {
      order: 'pre' as const,
      handler(html: string, ctx: { filename: string }) {
        if (siteLang !== 'en') return html
        const rel = path.relative(__dirname, ctx.filename).replace(/\\/g, '/')
        const seite = rel === 'index.html' ? 'index' : rel.replace(/\/index\.html$/, '')
        const kopfDatei = path.resolve(__dirname, 'i18n-en', `${seite}.head.html`)
        if (!fs.existsSync(kopfDatei)) {
          throw new Error(`EN-Build: ${kopfDatei} fehlt fuer ${rel}`)
        }
        const muster = /<!-- i18n:head -->[\s\S]*?<!-- \/i18n:head -->/
        if (!muster.test(html)) {
          throw new Error(`EN-Build: ${rel} hat keinen i18n:head-Block`)
        }
        const kopf = fs.readFileSync(kopfDatei, 'utf8').trim()
        return html
          .replace(muster, kopf)
          .replace(/<html lang="de"/, '<html lang="en"')
      },
    },
  }
}

export default defineConfig({
  plugins: [
    englischerSeitenkopf(),
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
    outDir: siteLang === 'en' ? 'dist-en' : 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input:
        siteLang === 'en'
          ? Object.fromEntries(EN_PAGES.map((name) => [name, pageInputs[name]]))
          : pageInputs,
    },
  },
})
