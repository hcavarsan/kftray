// Runs after `vite build`. Every bundle file gets a debug id (derived from
// its content, so identical bundles built on different runners get the same
// id), the bundle and its maps are uploaded to GlitchTip when a token is
// present, and the maps are then removed so they are never shipped inside
// the app. GlitchTip matches a frame to its map by that debug id, so the
// upload does not depend on the URL the webview serves the file from.
//
// sentry-cli reads SENTRY_URL, SENTRY_ORG, SENTRY_PROJECT and
// SENTRY_AUTH_TOKEN from the environment. The release is `kftray@<version>`,
// the same string the Rust side and the webview report with.
import { execFileSync } from 'node:child_process'
import { globSync, readFileSync, rmSync } from 'node:fs'
import { createRequire } from 'node:module'
import { resolve } from 'node:path'

const require = createRequire(import.meta.url)
const sentryCli = require('@sentry/cli').SentryCli.getPath()
const root = resolve(import.meta.dirname, '..')
const dist = resolve(root, 'dist')
const debug = process.env.TAURI_ENV_DEBUG === 'true'

const run = args => {
  console.log(`[sourcemaps] sentry-cli ${args.join(' ')}`)
  execFileSync(sentryCli, args, { cwd: root, stdio: 'inherit' })
}

run(['sourcemaps', 'inject', dist])

if (process.env.SENTRY_AUTH_TOKEN) {
  const { version } = JSON.parse(
    readFileSync(resolve(root, 'package.json'), 'utf8'),
  )
  const release = `kftray@${version}`
  run(['releases', 'new', release])
  run(['sourcemaps', 'upload', '--release', release, dist])
  run(['releases', 'finalize', release])
} else {
  console.log('[sourcemaps] SENTRY_AUTH_TOKEN unset; not uploading')
}

// Debug builds reference their maps and are never distributed.
if (!debug) {
  const maps = globSync('**/*.map', { cwd: dist })
  for (const map of maps) {
    rmSync(resolve(dist, map))
  }
  console.log(`[sourcemaps] removed ${maps.length} map file(s) from dist`)
}
