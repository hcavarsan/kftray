import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { runWithLimit } from './runWithLimit'
import { flushPromises } from './testFixtures'

describe('runWithLimit', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('never runs more than the limit at once and still processes every item', async () => {
    const gates = Array.from({ length: 5 }, () => Promise.withResolvers<void>())
    const started: number[] = []
    let active = 0
    let peak = 0

    const done = runWithLimit([0, 1, 2, 3, 4], 2, async item => {
      started.push(item)
      active += 1
      peak = Math.max(peak, active)
      await gates[item].promise
      active -= 1
    })

    await flushPromises()
    expect(started).toEqual([0, 1])

    gates[0].resolve()
    await flushPromises()
    expect(started).toEqual([0, 1, 2])

    for (const gate of gates) {
      gate.resolve()
    }
    await done

    expect(started).toEqual([0, 1, 2, 3, 4])
    expect(peak).toBe(2)
  })

  it('resolves immediately for an empty list', async () => {
    await expect(
      runWithLimit([], 8, async () => undefined),
    ).resolves.toBeUndefined()
  })
})
