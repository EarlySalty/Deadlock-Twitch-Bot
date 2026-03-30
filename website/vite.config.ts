import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
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
        faq: path.resolve(__dirname, 'faq/index.html'),
      },
    },
  },
})
