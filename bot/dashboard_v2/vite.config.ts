import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig(({ mode }) => {
  const isPreviewMode = mode === 'preview'

  return {
    plugins: [react(), tailwindcss()].flat(),
    resolve: {
      dedupe: ['react', 'react-dom'],
      alias: {
        '@': path.resolve(__dirname, './src'),
      },
    },
    base: isPreviewMode ? '/' : '/twitch/dashboard-v2/',
    build: {
      outDir: isPreviewMode ? './dist-preview' : './dist',
      emptyOutDir: true,
      chunkSizeWarningLimit: 1000,
      rolldownOptions: {
        output: {
          codeSplitting: {
            groups: [
              { name: 'react', test: /\/node_modules\/(?:react(?:-dom|-is)?|scheduler)\//, priority: 30 },
              { name: 'query', test: /\/node_modules\/@tanstack\/(?:react-query|query-core)\//, priority: 20 },
              { name: 'charts', test: /\/node_modules\/recharts\//, priority: 10 },
            ],
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
