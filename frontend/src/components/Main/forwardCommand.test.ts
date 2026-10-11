import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { Config } from '@/types'

import { executeForwardCommand } from './forwardCommand'
import { startForward, stopForward } from './portForwardCommands'
import { createReservationRegistry } from './reservationRegistry'
import { makeConfig } from './testFixtures'

vi.mock('./portForwardCommands', () => ({
  startForward: vi.fn(),
  stopForward: vi.fn(),
}))

const setup = () => {
  const registry = createReservationRegistry(vi.fn())
  const applied: Config[][] = []
  const applyConfigs = vi.fn(
    async (update: (current: Config[]) => Config[]) => {
      applied.push(update([makeConfig(1), makeConfig(2)]))
    },
  )
  const refreshConfigs = vi.fn(async () => undefined)

  return {
    registry,
    applied,
    applyConfigs,
    refreshConfigs,
    deps: {
      pendingConfigActionsRef: registry.pendingConfigActionsRef,
      inFlightRef: registry.inFlightRef,
      applyConfigs,
      refreshConfigs,
    },
  }
}

describe('executeForwardCommand', () => {
  beforeEach(() => {
    vi.mocked(startForward).mockReset()
    vi.mocked(stopForward).mockReset()
  })

  it('applies the new running state when its token still owns the reservation', async () => {
    const { registry, deps, applied, refreshConfigs } = setup()
    const token = registry.markPending(1, 'starting')

    await executeForwardCommand(deps, makeConfig(1), 'starting', token)

    expect(applied).toEqual([[makeConfig(1, true), makeConfig(2)]])
    expect(refreshConfigs).not.toHaveBeenCalled()
  })

  it('applies a stop the same way', async () => {
    const { registry, deps, applied } = setup()
    const token = registry.markPending(2, 'stopping')

    await executeForwardCommand(deps, makeConfig(2, true), 'stopping', token)

    expect(stopForward).toHaveBeenCalledTimes(1)
    expect(applied).toEqual([[makeConfig(1), makeConfig(2, false)]])
  })

  it('applies nothing and refreshes when a later reservation owns the id', async () => {
    const { registry, deps, applyConfigs, refreshConfigs } = setup()
    const superseded = registry.markPending(1, 'starting')

    registry.markPending(1, 'stopping')
    await executeForwardCommand(deps, makeConfig(1), 'starting', superseded)

    expect(applyConfigs).not.toHaveBeenCalled()
    expect(refreshConfigs).toHaveBeenCalledTimes(1)
  })

  it('applies the result when called without a token', async () => {
    const { deps, applied } = setup()

    await executeForwardCommand(deps, makeConfig(1), 'starting')

    expect(applied).toEqual([[makeConfig(1, true), makeConfig(2)]])
  })

  it('drops the saved connection error when a start succeeds', async () => {
    const { deps } = setup()
    const failing: Config = {
      ...makeConfig(1),
      is_retrying: true,
      retry_count: 2,
      last_error: 'pod web-0 is not ready',
    }
    let applied: Config[] = []

    await executeForwardCommand(
      {
        ...deps,
        applyConfigs: async update => {
          applied = update([failing])
        },
      },
      failing,
      'starting',
    )

    expect(applied).toEqual([makeConfig(1, true)])
  })

  it('releases its in-flight mark only when that mark is still its own', async () => {
    const { registry, deps } = setup()
    const gate = Promise.withResolvers<void>()

    vi.mocked(startForward).mockReturnValueOnce(gate.promise)
    const first = registry.markPending(1, 'starting')
    const running = executeForwardCommand(
      deps,
      makeConfig(1),
      'starting',
      first,
    )

    expect(registry.inFlightRef.current.get(1)).toBe(first)
    registry.inFlightRef.current.set(1, first + 100)
    gate.resolve()
    await running

    expect(registry.inFlightRef.current.get(1)).toBe(first + 100)
  })

  it('clears the in-flight mark and rethrows when the command fails', async () => {
    const { registry, deps, applyConfigs } = setup()
    const token = registry.markPending(1, 'starting')

    vi.mocked(startForward).mockRejectedValueOnce(new Error('boom'))

    await expect(
      executeForwardCommand(deps, makeConfig(1), 'starting', token),
    ).rejects.toThrow('boom')
    expect(registry.inFlightRef.current.has(1)).toBe(false)
    expect(applyConfigs).not.toHaveBeenCalled()
  })
})
