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
      // V2-Routen bewusst nicht prerendern: das Plugin rendert nur den
      // main-Entry in fremde HTML-Dateien (leere Roots). V2 ist CSR + noindex.
      additionalPrerenderRoutes: ['/'],
    }),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  base: '/streamer/',
  server: {
    fs: {
      // V2 liest die Bot-Wissensbasis (rust/knowledge/bot/*.md) zur Build-Zeit ein.
      allow: [path.resolve(__dirname, '..')],
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, 'index.html'),
        affiliateProgram: path.resolve(__dirname, 'vertriebler/index.html'),
        affiliatePortal: path.resolve(__dirname, 'affiliate-portal/index.html'),
        onboarding: path.resolve(__dirname, 'onboarding/index.html'),
        v2: path.resolve(__dirname, 'v2/index.html'),
        v2Features: path.resolve(__dirname, 'v2/features/index.html'),
        v2Faq: path.resolve(__dirname, 'v2/faq/index.html'),
      },
    },
  },
})
