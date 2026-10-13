import { beforeEach, describe, expect, it, vi } from 'vitest'

import { makeConfig } from '@/components/Main/testFixtures'
import { invoke } from '@/lib/tauri'

import { fetchConfigsWithState } from './useConfigs'

vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))

const state = (configId: number, ownedElsewhere: boolean) => ({
  config_id: configId,
  is_running: true,
  is_retrying: false,
  retry_count: null,
  last_error: null,
  owned_elsewhere: ownedElsewhere,
})

describe('fetchConfigsWithState', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
  })

  it('keeps which running forwards another kftray process owns', async () => {
    vi.mocked(invoke).mockImplementation(async command =>
      command === 'get_configs_cmd'
        ? [makeConfig(1), makeConfig(2), makeConfig(3)]
        : [state(1, true), state(2, false)],
    )

    const configs = await fetchConfigsWithState()

    expect(configs.map(config => config.owned_elsewhere)).toEqual([
      true,
      false,
      false,
    ])
  })
})
