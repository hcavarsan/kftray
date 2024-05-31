import { defineConfig } from '@farmfe/core'
import terser from '@rollup/plugin-terser'


export default defineConfig({
  plugins: [
    '@farmfe/plugin-react',
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
  envPrefix: ['FARM_', 'TAURI_'],
  compilation: {
    minify: {
      compress: true,
      mangle: true
    },
    sourcemap: !!process.env.TAURI_DEBUG,
  }
})
