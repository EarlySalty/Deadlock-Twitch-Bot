import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const isPreviewMode = mode === 'preview'

  return {
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, './src'),
      },
    },
    base: isPreviewMode ? '/' : '/twitch/dashboard-v2/',
    build: {
      outDir: isPreviewMode ? './dist-preview' : '../analytics/dashboard_v2/dist',
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
      ...(isPreviewMode ? { port: 4174 } : {}),
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
      ...(isPreviewMode ? { port: 4175 } : {}),
      strictPort: true,
      allowedHosts: ['localhost', '.localhost'],
    },
  }
})
