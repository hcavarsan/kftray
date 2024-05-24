import path from 'node:path'
import { defineConfig } from 'vite'

import terser from '@rollup/plugin-terser'
import react from '@vitejs/plugin-react-swc'

console.log('TAURI_DEBUG:', process.env.TAURI_DEBUG)

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  plugins: [
    react(),
    !process.env.TAURI_DEBUG && terser({
      compress: {
        drop_console: true,
        drop_debugger: true,
        module: true,
        passes: 2,
        pure_funcs: ['console.info', 'console.debug', 'console.warn'],
      },
      format: {
        comments: false,
      },
    }),
  ].filter(Boolean),
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    open: process.env.TAURI_ARCH === undefined,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    chunkSizeWarningLimit: 600,
    target: process.env.TAURI_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_DEBUG ? 'terser' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules')) {
            const chunks = id.toString().split('node_modules/')[1].split('/')

            if (chunks.length > 1 && chunks[0] !== '') {
              return chunks[0]
            }
          }
        },
      },
    },
  },
})
