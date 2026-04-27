import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  base: '/',
  build: {
    outDir: './dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      output: {
        manualChunks: {
          react: ['react', 'react/jsx-runtime', 'react-dom', 'react-dom/client'],
          charts: ['recharts'],
          query: ['@tanstack/react-query'],
        },
      },
    },
  },
  server: {
    host: 'localhost',
    strictPort: true,
    allowedHosts: ['localhost', '.localhost'],
    proxy: {
      '/twitch/demo/api': {
        target: 'http://localhost:8765',
        changeOrigin: true,
        secure: false,
      },
      '/twitch/api': {
        target: 'http://localhost:8765',
        changeOrigin: true,
        secure: false,
      },
    },
  },
  preview: {
    host: 'localhost',
    strictPort: true,
    allowedHosts: ['localhost', '.localhost'],
  },
})
