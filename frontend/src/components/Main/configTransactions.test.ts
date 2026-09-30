import type { RefObject } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import { fetchConfigsWithState } from '@/hooks/useConfigs'
import type { Config, StoredConfig } from '@/types'

import {
  deleteConfigsTransaction,
  saveConfigTransaction,
} from './configTransactions'
import { createReservationRegistry } from './reservationRegistry'
import { makeConfig } from './testFixtures'
import type { RunForwardCommand } from './useForwardCommand'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@/hooks/useConfigs', () => ({ fetchConfigsWithState: vi.fn() }))
vi.mock('@/components/ui/toaster', () => ({
  toaster: {
    info: vi.fn(),
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}))

const setup = (configs: Config[] = []) => {
  const registry = createReservationRegistry(vi.fn())
  const configsRef: RefObject<Config[]> = { current: configs }
  const applyConfigs = vi.fn(
    async (_update: (current: Config[]) => Config[]) => undefined,
  )
  const refreshConfigs = vi.fn(async () => undefined)
  const runForwardCommand = vi.fn<RunForwardCommand>(async () => undefined)

  return {
    registry,
    applyConfigs,
    refreshConfigs,
    runForwardCommand,
    deps: {
      ...registry,
      configsRef,
      applyConfigs,
      refreshConfigs,
      runForwardCommand,
    },
  }
}

const invokedCommands = () => vi.mocked(invoke).mock.calls.map(([cmd]) => cmd)

describe('deleteConfigsTransaction', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    vi.mocked(invoke).mockResolvedValue(undefined)
  })

  it('refuses to delete a config that started running after the dialog opened', async () => {
    const { deps } = setup()

    vi.mocked(fetchConfigsWithState).mockResolvedValue([
      makeConfig(1, true),
      makeConfig(2),
    ])

    await expect(deleteConfigsTransaction(deps, [1, 2])).resolves.toBe(false)

    expect(invokedCommands()).not.toContain('delete_configs_cmd')
    expect(toaster.error).toHaveBeenCalledWith(
      expect.objectContaining({
        description:
          '1 selected configuration(s) are running. Stop them before deleting.',
      }),
    )
  })

  it('keeps the ids reserved while it revalidates them', async () => {
    const { deps, registry } = setup()
    const busyWhileFetching: boolean[] = []

    vi.mocked(fetchConfigsWithState).mockImplementation(async () => {
      busyWhileFetching.push(registry.isBusy(1), registry.isBusy(2))

      return [makeConfig(1), makeConfig(2)]
    })

    await expect(deleteConfigsTransaction(deps, [1, 2])).resolves.toBe(true)

    expect(busyWhileFetching).toEqual([true, true])
    expect(registry.isBusy(1)).toBe(false)
    expect(registry.isBusy(2)).toBe(false)
  })

  it('recovers through a query refresh instead of writing the fetched snapshot', async () => {
    const { deps, applyConfigs, refreshConfigs } = setup()

    vi.mocked(fetchConfigsWithState).mockResolvedValue([makeConfig(1, true)])

    await deleteConfigsTransaction(deps, [1])

    expect(applyConfigs).not.toHaveBeenCalled()
    expect(refreshConfigs).toHaveBeenCalledTimes(1)
  })

  it('refreshes without deleting when a selected config no longer exists', async () => {
    const { deps, refreshConfigs } = setup()

    vi.mocked(fetchConfigsWithState).mockResolvedValue([makeConfig(1)])

    await expect(deleteConfigsTransaction(deps, [1, 2])).resolves.toBe(false)

    expect(invokedCommands()).not.toContain('delete_configs_cmd')
    expect(refreshConfigs).toHaveBeenCalledTimes(1)
  })

  it('removes exactly the deleted configs from the cache', async () => {
    const { deps, applyConfigs } = setup()

    vi.mocked(fetchConfigsWithState).mockResolvedValue([
      makeConfig(1),
      makeConfig(2),
    ])

    await expect(deleteConfigsTransaction(deps, [1])).resolves.toBe(true)

    expect(invoke).toHaveBeenCalledWith('delete_configs_cmd', { ids: [1] })
    const update = applyConfigs.mock.calls[0][0]

    expect(update([makeConfig(1), makeConfig(2)])).toEqual([makeConfig(2)])
  })
})

describe('saveConfigTransaction', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    vi.mocked(invoke).mockResolvedValue(undefined)
  })

  it('reserves an edited config for the whole save even when no restart is needed', async () => {
    const { deps, registry, runForwardCommand } = setup([makeConfig(1)])
    const reservationDuringUpdate: (string | undefined)[] = []

    vi.mocked(invoke).mockImplementation(async command => {
      reservationDuringUpdate.push(
        registry.pendingConfigActionsRef.current.get(1)?.action,
      )

      return command
    })

    await expect(
      saveConfigTransaction(deps, makeConfig(1), true),
    ).resolves.toBe(true)

    expect(reservationDuringUpdate).toEqual(['saving'])
    expect(runForwardCommand).not.toHaveBeenCalled()
    expect(registry.isBusy(1)).toBe(false)
  })

  it('stops a running config by its pre-edit identity and restarts it with the edit', async () => {
    const running = makeConfig(1, true)
    const { deps, runForwardCommand } = setup([running])
    const edited: StoredConfig = {
      ...running,
      service: 'edited-service',
      local_port: 9999,
    }

    await expect(saveConfigTransaction(deps, edited, true)).resolves.toBe(true)

    expect(runForwardCommand).toHaveBeenCalledTimes(2)
    expect(runForwardCommand.mock.calls[0].slice(0, 2)).toEqual([
      running,
      'stopping',
    ])
    expect(runForwardCommand.mock.calls[1].slice(0, 2)).toEqual([
      edited,
      'starting',
    ])
  })

  it('merges the edit into the cache before the reservation is released', async () => {
    const { deps, registry, applyConfigs } = setup([makeConfig(1)])
    const reservedWhenMerged: boolean[] = []
    const edited: StoredConfig = { ...makeConfig(1), service: 'edited-service' }

    applyConfigs.mockImplementation(async () => {
      reservedWhenMerged.push(registry.isBusy(1))
    })

    await saveConfigTransaction(deps, edited, true)

    expect(reservedWhenMerged).toEqual([true])
    const update = applyConfigs.mock.calls[0][0]

    expect(update([makeConfig(1, true), makeConfig(2)])).toEqual([
      { ...edited, is_running: true },
      makeConfig(2),
    ])
  })
})
