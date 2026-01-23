import path, { resolve } from 'node:path'
import { visualizer } from 'rollup-plugin-visualizer'
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
  if (!id.includes('node_modules')) {
return
}
  if (
    id.includes('@chakra-ui') ||
    id.includes('@emotion') ||
    id.includes('@ark-ui') ||
    id.includes('@zag-js') ||
    id.includes('framer-motion')
  ) {
    return 'chakra-ui'
  }

  if (id.includes('lucide-react')) {
    return 'icons'
  }

  if (id.includes('@tanstack/react-query')) {
    return 'react-query'
  }

  if (id.includes('react-select') || id.includes('next-themes')) {
    return 'utils'
  }

  if (
    id.includes('/react/') ||
    id.includes('/react-dom/') ||
    id.includes('/scheduler/') ||
    id.match(/\/react\/index\.js/) ||
    id.match(/\/react-dom\/index\.js/)
  ) {
    return 'react-vendor'
  }

  if (id.includes('@tauri-apps')) {
    return 'tauri'
  }

  if (id.includes('lodash')) {
    return 'utils'
  }

  return 'vendor'
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
    }),
    ...(process.env.ANALYZE ? [
      visualizer({
        open: true,
        gzipSize: true,
        brotliSize: true,
        filename: 'dist/stats.html'
      })
    ] : [])
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
	emptyOutDir: false,
    chunkSizeWarningLimit: 500,
    target: process.env.TAURI_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_DEBUG ? 'terser' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        logs: resolve(__dirname, 'logs.html'),
      },
      output: {
        manualChunks: createManualChunks,
        chunkFileNames: 'assets/[name]-[hash].js',
        entryFileNames: 'assets/[name]-[hash].js',
        assetFileNames: 'assets/[name]-[hash].[ext]'
      },
      treeshake: {
        moduleSideEffects: 'no-external',
        propertyReadSideEffects: false,
        tryCatchDeoptimization: false
      }
    }
  }
} as UserConfig)
