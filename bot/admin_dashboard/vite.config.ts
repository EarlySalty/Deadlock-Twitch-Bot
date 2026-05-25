import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  base: '/twitch/admin/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/recharts')) {
            return 'charts';
          }

          if (id.includes('node_modules/@tanstack/react-query')) {
            return 'query';
          }

          if (
            id.includes('node_modules/react/') ||
            id.includes('node_modules/react-dom/') ||
            id.includes('node_modules/scheduler/')
          ) {
            return 'react';
          }

          return undefined;
        },
      },
    },
  },
  server: {
    host: 'localhost',
    strictPort: true,
    allowedHosts: ['localhost', '.localhost'],
    proxy: {
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
});
