import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const isPreviewMode = mode === 'preview'

  return {
    plugins: [react(), tailwindcss()].flat(),
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
          manualChunks(id: string) {
            if (id.includes('/node_modules/react') || id.includes('/node_modules/react-dom')) {
              return 'react'
            }
            if (id.includes('/node_modules/recharts')) {
              return 'charts'
            }
            if (id.includes('/node_modules/@tanstack/react-query')) {
              return 'query'
            }
            return undefined
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
