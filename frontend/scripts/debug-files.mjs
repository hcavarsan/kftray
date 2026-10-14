import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import { resolve } from 'node:path'

const require = createRequire(import.meta.url)
const sentryCli = require('@sentry/cli').SentryCli.getPath()
const root = resolve(import.meta.dirname, '..', '..')

const [binary, triple = process.env.TAURI_ENV_TARGET_TRIPLE] =
  process.argv.slice(2)
const pdbName = process.env.KFTRAY_PDB_NAME ?? binary
if (!binary || !triple) {
  throw new Error('usage: debug-files.mjs <binary> [target-triple]')
}

const run = (file, args) => {
  console.log(
    `[debug-files] ${file === sentryCli ? 'sentry-cli' : file} ${args.join(' ')}`,
  )
  execFileSync(file, args, { cwd: root, stdio: 'inherit' })
}

if (!process.env.SENTRY_AUTH_TOKEN) {
  console.log('[debug-files] SENTRY_AUTH_TOKEN unset; not uploading')
  process.exit(0)
}

const targets =
  triple === 'universal-apple-darwin'
    ? ['x86_64-apple-darwin', 'aarch64-apple-darwin']
    : [triple]

const files = targets.map(target => {
  const dir = resolve(root, 'target', target, 'release')
  if (process.platform === 'win32') {
    const pdb = resolve(dir, `${pdbName}.pdb`)
    return { check: pdb, upload: pdb }
  }
  if (process.platform === 'darwin') {
    const bundle = resolve(dir, `${binary}.dSYM`)
    const dwarf = resolve(bundle, 'Contents', 'Resources', 'DWARF')
    const [file] = existsSync(dwarf) ? readdirSync(dwarf) : []
    return { check: file ? resolve(dwarf, file) : dwarf, upload: bundle }
  }
  const executable = resolve(dir, binary)
  const debug = `${executable}.debug`
  run('objcopy', ['--only-keep-debug', executable, debug])
  run('objcopy', ['--strip-debug', executable])
  return { check: debug, upload: debug }
})

for (const { check } of files) {
  if (!existsSync(check)) {
    throw new Error(
      `[debug-files] ${check} was not produced; check the CARGO_PROFILE_RELEASE_* settings`,
    )
  }
  run(sentryCli, ['debug-files', 'check', check])
}
run(sentryCli, ['debug-files', 'upload', ...files.map(({ upload }) => upload)])
