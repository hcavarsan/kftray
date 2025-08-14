import path from 'node:path'
import { defineConfig, type Plugin, type UserConfig } from 'vite'
import tsconfigPaths from 'vite-tsconfig-paths'

import { codecovVitePlugin } from '@codecov/vite-plugin'
import terser from '@rollup/plugin-terser'
import react from '@vitejs/plugin-react-swc'

const asPlugin = (p: any) => p as Plugin

const terserConfig = {
  mangle: true,
  output: { comments: false },
  compress: {
    drop_console: true,
    drop_debugger: true,
    pure_funcs: ['console.info', 'console.debug', 'console.warn'],
    passes: 2
  }
}

const createManualChunks = (id: string) => {
  if (id.includes('node_modules')) {
    const chunks = id.toString().split('node_modules/')[1].split('/')


    if (chunks.length > 1 && chunks[0] !== '') {
      return chunks[0]
    }
  }
}

export default defineConfig({
  resolve: {
    alias: { '@': path.resolve(__dirname, 'src') }
  },

  plugins: [
    asPlugin(react()),
    asPlugin(tsconfigPaths()),
    ...(!process.env.TAURI_DEBUG ? [asPlugin(terser(terserConfig))] : []),
    codecovVitePlugin({
      enableBundleAnalysis: process.env.CODECOV_TOKEN !== undefined,
      bundleName: 'kftray',
      uploadToken: process.env.CODECOV_TOKEN,
      gitService: 'github',
    })
  ],

  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    open: process.env.TAURI_ARCH === undefined
  },

  envPrefix: ['VITE_', 'TAURI_'],

  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 600,
    target: process.env.TAURI_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_DEBUG ? 'terser' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      output: { manualChunks: createManualChunks }
    }
  }
} as UserConfig)
