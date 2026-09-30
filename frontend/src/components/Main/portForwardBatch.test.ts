import type { RefObject } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { toaster } from '@/components/ui/toaster'

import {
  BATCH_DEADLINE_MS,
  type BatchDeps,
  DEADLINE_GRACE_MS,
  executeBatch,
} from './portForwardBatch'
import { createReservationRegistry } from './reservationRegistry'
import { flushPromises, makeConfig } from './testFixtures'
import type { RunForwardCommand } from './useForwardCommand'

vi.mock('@/components/ui/toaster', () => ({
  toaster: {
    info: vi.fn(),
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}))

const setup = () => {
  const registry = createReservationRegistry(vi.fn())
  const controllerRef: RefObject<AbortController | null> = { current: null }
  const gates = new Map<number, PromiseWithResolvers<void>>()
  const runForwardCommand = vi.fn<RunForwardCommand>(config => {
    const gate = Promise.withResolvers<void>()

    gates.set(config.id, gate)

    return gate.promise
  })
  const refreshConfigs = vi.fn(async () => undefined)
  const setBusy = vi.fn()
  const graceTimeouts: BatchDeps['graceTimeouts'] = new Set()
  const deps: BatchDeps = {
    pending: registry,
    runForwardCommand,
    refreshConfigs,
    controllerRef,
    graceTimeouts,
    setBusy,
  }
  const gate = (id: number) => {
    const found = gates.get(id)

    if (!found) {
      throw new Error(`config ${id} was never started`)
    }

    return found
  }
  const abort = () => {
    if (!controllerRef.current) {
      throw new Error('no batch is running')
    }
    controllerRef.current.abort()
  }
  const startedIds = () => runForwardCommand.mock.calls.map(([c]) => c.id)

  return {
    registry,
    controllerRef,
    graceTimeouts,
    setBusy,
    deps,
    gate,
    abort,
    startedIds,
    runForwardCommand,
  }
}

const configsWithIds = (ids: number[]) => ids.map(id => makeConfig(id))
const range = (from: number, to: number) =>
  Array.from({ length: to - from + 1 }, (_, index) => from + index)

describe('executeBatch', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
  })

  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('on abort releases only the queued reservations this batch created', async () => {
    const { deps, registry, abort, gate } = setup()
    const batch = executeBatch(deps, configsWithIds(range(1, 10)), 'starting')

    await flushPromises()
    const newer = registry.markPending(9, 'stopping')

    abort()

    const pending = registry.pendingConfigActionsRef.current

    expect(pending.has(10)).toBe(false)
    expect(pending.get(9)?.token).toBe(newer)
    expect(range(1, 8).every(id => pending.has(id))).toBe(true)
    for (const id of range(1, 8)) {
      gate(id).resolve()
    }
    await batch
  })

  it('counts only invocations still running in the timeout message after an abort', async () => {
    const { deps, abort } = setup()
    const batch = executeBatch(deps, configsWithIds(range(1, 10)), 'starting')

    await flushPromises()
    abort()
    await vi.advanceTimersByTimeAsync(BATCH_DEADLINE_MS)
    await batch

    expect(toaster.error).toHaveBeenCalledWith(
      expect.objectContaining({
        description: expect.stringContaining(
          '8 configuration(s) did not finish',
        ),
      }),
    )
  })

  it('leaves the work of a worker superseded by a newer reservation alone', async () => {
    const { deps, registry, gate, startedIds } = setup()
    const batch = executeBatch(deps, configsWithIds(range(1, 9)), 'starting')

    await flushPromises()
    const newer = registry.markPending(9, 'stopping')

    gate(1).resolve()
    await flushPromises()

    expect(startedIds()).not.toContain(9)
    expect(registry.pendingConfigActionsRef.current.get(9)?.token).toBe(newer)
    for (const id of range(2, 8)) {
      gate(id).resolve()
    }
    await batch
  })

  it('releases its own reservation when a worker dequeues an id after the abort', async () => {
    const { deps, registry, abort, gate, startedIds } = setup()
    const shared = makeConfig(100)
    const batch = executeBatch(
      deps,
      [shared, ...configsWithIds(range(1, 8)), shared],
      'starting',
    )

    await flushPromises()
    expect(startedIds()).toEqual(range(1, 8))
    abort()
    gate(1).resolve()
    await flushPromises()

    expect(startedIds()).not.toContain(100)
    expect(registry.pendingConfigActionsRef.current.has(100)).toBe(false)
    for (const id of range(2, 8)) {
      gate(id).resolve()
    }
    await batch
  })

  it('stops waiting at the deadline so a hung invoke cannot block later batches', async () => {
    const { deps, controllerRef, setBusy, startedIds, gate } = setup()
    const batch = executeBatch(deps, configsWithIds([1]), 'starting')

    await flushPromises()
    await vi.advanceTimersByTimeAsync(BATCH_DEADLINE_MS)
    await batch

    expect(controllerRef.current).toBeNull()
    expect(setBusy).toHaveBeenLastCalledWith(false)

    const next = executeBatch(deps, configsWithIds([2]), 'starting')

    await flushPromises()
    expect(startedIds()).toEqual([1, 2])
    expect(toaster.info).not.toHaveBeenCalled()
    gate(2).resolve()
    await next
  })

  it('still reports a failure that happened before the deadline won the race', async () => {
    const { deps, gate } = setup()
    const batch = executeBatch(deps, configsWithIds([1, 2]), 'starting')

    await flushPromises()
    gate(1).reject(new Error('boom'))
    await flushPromises()
    await vi.advanceTimersByTimeAsync(BATCH_DEADLINE_MS)
    await batch

    expect(toaster.error).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        title: 'Start Failed',
        description: 'Config 1: boom',
      }),
    )
    expect(toaster.error).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        description: expect.stringContaining(
          '1 configuration(s) did not finish',
        ),
      }),
    )
  })

  it('reports a failure that settles after the deadline on its own', async () => {
    const { deps, gate } = setup()
    const batch = executeBatch(deps, configsWithIds([1]), 'stopping')

    await flushPromises()
    await vi.advanceTimersByTimeAsync(BATCH_DEADLINE_MS)
    await batch
    vi.mocked(toaster.error).mockClear()

    gate(1).reject(new Error('late'))
    await flushPromises()

    expect(toaster.error).toHaveBeenCalledTimes(1)
    expect(toaster.error).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Stop Failed',
        description: 'Config 1: late',
      }),
    )
  })

  it('cancels the deadline timer when the batch finishes first', async () => {
    const { deps, gate } = setup()
    const batch = executeBatch(deps, configsWithIds([1]), 'starting')

    await flushPromises()
    gate(1).resolve()
    await batch

    expect(vi.getTimerCount()).toBe(0)
  })

  it('releases reservations of configurations that never started as soon as the deadline hits', async () => {
    const { deps, registry } = setup()
    const batch = executeBatch(deps, configsWithIds(range(1, 10)), 'starting')

    await flushPromises()
    await vi.advanceTimersByTimeAsync(BATCH_DEADLINE_MS)
    await batch

    const pending = registry.pendingConfigActionsRef.current

    expect(pending.has(9)).toBe(false)
    expect(pending.has(10)).toBe(false)
    expect(range(1, 8).every(id => pending.has(id))).toBe(true)
  })

  it('keeps reservations of running invokes after the deadline until each one settles', async () => {
    const { deps, registry, gate } = setup()
    const batch = executeBatch(deps, configsWithIds([1, 2]), 'starting')

    await flushPromises()
    await vi.advanceTimersByTimeAsync(BATCH_DEADLINE_MS)
    await batch

    expect(registry.isBusy(1)).toBe(true)
    expect(registry.isBusy(2)).toBe(true)
    gate(1).resolve()
    await flushPromises()
    expect(registry.isBusy(1)).toBe(false)
    expect(registry.isBusy(2)).toBe(true)
  })

  it('marks hung reservations timed out after the grace period and keeps them busy until they settle', async () => {
    const { deps, registry, graceTimeouts, gate } = setup()
    const batch = executeBatch(deps, configsWithIds([1]), 'starting')

    await flushPromises()
    await vi.advanceTimersByTimeAsync(BATCH_DEADLINE_MS)
    await batch
    const pending = registry.pendingConfigActionsRef.current

    await vi.advanceTimersByTimeAsync(DEADLINE_GRACE_MS - 1)
    expect(pending.get(1)?.timedOut).toBeUndefined()
    expect(graceTimeouts.size).toBe(1)

    await vi.advanceTimersByTimeAsync(1)
    expect(pending.get(1)?.timedOut).toBe(true)
    expect(registry.isBusy(1)).toBe(true)
    expect(graceTimeouts.size).toBe(0)

    gate(1).resolve()
    await flushPromises()
    expect(registry.isBusy(1)).toBe(false)
  })
})
