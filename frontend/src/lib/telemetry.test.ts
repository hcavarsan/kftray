import { describe, expect, it } from 'vitest'

import { errorReport } from './telemetry'

describe('errorReport', () => {
  it('describes an error by name, message and stack', () => {
    const error = new TypeError('x is undefined')

    expect(errorReport(error)).toEqual({
      name: 'TypeError',
      message: 'x is undefined',
      stack: error.stack ?? null,
    })
  })

  it('ignores values that are not errors', () => {
    expect(errorReport('Failed to forward context prod')).toBeNull()
    expect(errorReport(undefined)).toBeNull()
  })
})
