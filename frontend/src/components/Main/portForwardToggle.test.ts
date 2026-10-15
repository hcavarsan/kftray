import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { showErrorToast } from '@/components/ui/error-toast'
import { toaster } from '@/components/ui/toaster'

import { executeToggle, type ToggleDeps } from './portForwardToggle'
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

vi.mock('@/components/ui/error-toast', () => ({
  showErrorToast: vi.fn(),
}))

const setup = () => {
  const registry = createReservationRegistry(vi.fn())
  const preflights: PromiseWithResolvers<boolean>[] = []
  const runForwardCommand = vi.fn<RunForwardCommand>(async () => undefined)
  const deps: ToggleDeps = {
    pending: registry,
    ensurePrivileges: () => {
      const preflight = Promise.withResolvers<boolean>()

      preflights.push(preflight)

      return preflight.promise
    },
    askAfterRefusal: vi.fn(async () => 'cancel' as const),
    runForwardCommand,
    refreshConfigs: vi.fn(async () => undefined),
  }

  return { registry, preflights, runForwardCommand, deps }
}

describe('executeToggle', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
  })

  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('sends one start when the row is started again during the privilege preflight', async () => {
    const { deps, preflights, runForwardCommand } = setup()
    const config = makeConfig(1)

    const first = executeToggle(deps, config, 'starting')
    const second = executeToggle(deps, config, 'starting')

    for (const preflight of preflights) {
      preflight.resolve(true)
    }
    await flushPromises()
    await Promise.all([first, second])

    expect(runForwardCommand).toHaveBeenCalledTimes(1)
  })

  it('refreshes the row without a toast when the forward is already running', async () => {
    const { deps, preflights, registry, runForwardCommand } = setup()
    const config = makeConfig(1)
    runForwardCommand.mockRejectedValue(
      new Error('Port forwarding is already running for config 1'),
    )

    const toggle = executeToggle(deps, config, 'starting')
    for (const preflight of preflights) {
      preflight.resolve(true)
    }
    await flushPromises()
    await toggle
    await flushPromises()

    expect(toaster.info).not.toHaveBeenCalled()
    expect(toaster.error).not.toHaveBeenCalled()
    expect(showErrorToast).not.toHaveBeenCalled()
    expect(deps.refreshConfigs).toHaveBeenCalled()
    expect(registry.isBusy(1)).toBe(false)
    expect(runForwardCommand).toHaveBeenCalledTimes(1)
  })

  it.each([
    ['cancelled', (p: PromiseWithResolvers<boolean>) => p.resolve(false)],
    ['failed', (p: PromiseWithResolvers<boolean>) => p.reject(new Error('x'))],
  ])('releases the row when the privilege prompt is %s', async (_, settle) => {
    const { deps, preflights, registry, runForwardCommand } = setup()
    const toggle = executeToggle(deps, makeConfig(1), 'starting').catch(
      () => undefined,
    )

    await flushPromises()
    settle(preflights[0])
    await toggle

    expect(runForwardCommand).not.toHaveBeenCalled()
    expect(registry.isBusy(1)).toBe(false)
  })

  it('skips the retry after a privilege refusal when the row was started again during the prompt', async () => {
    const { deps, preflights, runForwardCommand } = setup()
    const config = makeConfig(1)
    const decision = Promise.withResolvers<'continue'>()
    const secondStart = Promise.withResolvers<undefined>()

    deps.askAfterRefusal = () => decision.promise
    runForwardCommand
      .mockRejectedValueOnce(
        new Error('loopback address configuration cancelled'),
      )
      .mockImplementationOnce(() => secondStart.promise)

    const first = executeToggle(deps, config, 'starting')

    preflights[0].resolve(true)
    await flushPromises()
    const second = executeToggle(deps, config, 'starting')

    preflights[1].resolve(true)
    await flushPromises()
    decision.resolve('continue')
    await flushPromises()
    secondStart.resolve(undefined)
    await Promise.all([first, second])

    expect(runForwardCommand).toHaveBeenCalledTimes(2)
  })
})
