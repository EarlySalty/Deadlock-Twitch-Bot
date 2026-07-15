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
        affiliateProgram: path.resolve(__dirname, 'vertriebler/index.html'),
        affiliatePortal: path.resolve(__dirname, 'affiliate-portal/index.html'),
        onboarding: path.resolve(__dirname, 'onboarding/index.html'),
        // Caddy serviert /twitch/faq* aus dist/faq — der Entry MUSS faq/index.html
        // heissen, sonst zeigt die Route weiter ins Leere (genau das war der 404).
        faq: path.resolve(__dirname, 'faq/index.html'),
      },
    },
  },
})
