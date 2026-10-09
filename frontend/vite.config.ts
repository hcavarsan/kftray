import { visualizer } from 'rollup-plugin-visualizer'
import { defineConfig } from 'vitest/config'

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
    !!process.env.ANALYZE &&
      visualizer({
        open: true,
        gzipSize: true,
        brotliSize: true,
        filename: 'dist/stats.html',
      }),
    codecovVitePlugin({
      enableBundleAnalysis: process.env.CODECOV_BUNDLE_ANALYSIS === 'true',
      bundleName: 'kftray',
      gitService: 'github',
      oidc: { useGitHubOIDC: process.env.CODECOV_OIDC === 'true' },
      telemetry: false,
    }),
  ],

  clearScreen: false,

  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },

  server: {
    port: 1420,
    strictPort: true,
    open: process.env.TAURI_ENV_ARCH === undefined,
  },

  build: {
    outDir: 'dist',
    target:
      process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !isDebug,
    // Maps are always written: scripts/sourcemaps.mjs injects debug ids,
    // uploads them to GlitchTip when a token is present, and removes them
    // from dist in release builds so they are never shipped.
    sourcemap: isDebug ? true : 'hidden',
    rolldownOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        logs: resolve(import.meta.dirname, 'logs.html'),
      },
    },
  },
})
