import path from 'path'
import { defineConfig } from 'vite'

import react from '@vitejs/plugin-react-swc'

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    open: process.env.TAURI_ARCH === undefined,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    chunkSizeWarningLimit: 600,
    target: process.env.TAURI_PLATFORM == 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
})
