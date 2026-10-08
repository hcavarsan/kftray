import { describe, expect, it } from 'vitest'

import { errorReport } from './telemetry'

const MAIN = 'tauri://localhost/assets/main-BkQTPf31.js'

describe('errorReport', () => {
  it('drops a multiline message whose lines look like frames', () => {
    const message =
      'Failed to forward\n' +
      '    at prod-cluster svc/postgres (10.0.0.5:6443:1)\n' +
      'dial svc@prod-host:6443:1'
    const error = new Error(message)
    error.stack =
      `Error: ${message}\n` +
      `    at forward (${MAIN}:1:20)\n` +
      '    at https://tauri.localhost/assets/main-BkQTPf31.js:3:4'

    expect(errorReport(error)).toEqual({
      name: 'Error',
      stack: `${MAIN}:1:20\nhttps://tauri.localhost/assets/main-BkQTPf31.js:3:4`,
    })
  })

  it('drops message lines even when the stack header no longer matches', () => {
    const error = new Error('changed after the stack was taken')
    error.stack =
      'Error: Failed to forward\n' +
      '    at prod-cluster svc/postgres\n' +
      'dial svc@prod-host:6443:1\n' +
      `    at forward (${MAIN}:1:20)`

    expect(errorReport(error)?.stack).toBe(`${MAIN}:1:20`)
  })

  it('keeps only bundle locations from a WebKit stack', () => {
    const error = new TypeError('x is undefined')
    error.stack =
      `forward@${MAIN}:1:20\n` +
      'forEach@[native code]\n' +
      'load@https://prod-host.example.com/x.js:1:2\n' +
      `global code@${MAIN}:3:4`

    expect(errorReport(error)).toEqual({
      name: 'TypeError',
      stack: `${MAIN}:1:20\n${MAIN}:3:4`,
    })
  })

  it('sends no stack when no frame is from the bundle', () => {
    const error = new Error('Failed to forward svc/postgres in prod-cluster')

    expect(errorReport(error)).toEqual({ name: 'Error', stack: null })
  })

  it('ignores values that are not errors', () => {
    expect(errorReport('Failed to forward context prod')).toBeNull()
    expect(errorReport(undefined)).toBeNull()
  })
})
