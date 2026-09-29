import { visualizer } from 'rollup-plugin-visualizer'
import { defineConfig } from 'vite'

import { codecovVitePlugin } from '@codecov/vite-plugin'
import react from '@vitejs/plugin-react'

import { resolve } from 'node:path'

const isDebug = process.env.TAURI_ENV_DEBUG === 'true'

export default defineConfig({
  resolve: {
    tsconfigPaths: true,
  },

  plugins: [
    react(),
    codecovVitePlugin({
      enableBundleAnalysis: process.env.CODECOV_TOKEN !== undefined,
      bundleName: 'kftray',
      uploadToken: process.env.CODECOV_TOKEN,
      gitService: 'github',
    }),
    process.env.ANALYZE &&
      visualizer({
        open: true,
        gzipSize: true,
        brotliSize: true,
        filename: 'dist/stats.html',
      }),
  ],

  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    open: process.env.TAURI_ENV_ARCH === undefined,
  },

  build: {
    outDir: 'dist',
    emptyOutDir: false,
    target:
      process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !isDebug,
    sourcemap: isDebug,
    rolldownOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        logs: resolve(import.meta.dirname, 'logs.html'),
      },
      output: {
        codeSplitting: {
          groups: [
            {
              name: 'chakra-ui',
              test: /node_modules[\\/](@chakra-ui|@emotion|@ark-ui|@zag-js)/,
            },
            {
              name: 'react-vendor',
              test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/,
            },
            { name: 'tauri', test: /node_modules[\\/]@tauri-apps/ },
            { name: 'vendor', test: /node_modules/ },
          ],
        },
      },
    },
  },
})
