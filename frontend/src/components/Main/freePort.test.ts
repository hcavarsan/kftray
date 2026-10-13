import { beforeEach, describe, expect, it, vi } from 'vitest'

import { invoke } from '@/lib/tauri'

import { freePortFromError, moveToFreePort } from './freePort'
import { makeConfig } from './testFixtures'

vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))

describe('freePortFromError', () => {
  it('reads the free port from the start error the backend returns', () => {
    expect(
      freePortFromError(
        'Failed to start TCP port forwarding for service web: Local TCP port 8080 on 127.0.0.1 is already in use. Port 8081 is free.; cleanup incomplete: x',
      ),
    ).toBe(8081)
  })

  it('finds no port in other start errors', () => {
    expect(
      freePortFromError('Local TCP port 8080 on 127.0.0.1 is already in use.'),
    ).toBeUndefined()
    expect(
      freePortFromError(
        'Failed to bind TCP listener to 127.0.0.1:8080: Address already in use',
      ),
    ).toBeUndefined()
  })
})

describe('moveToFreePort', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
  })

  it('saves the stored config with only the local port changed', async () => {
    const stored = { ...makeConfig(3), local_port: 8080, alias: 'web' }

    vi.mocked(invoke).mockImplementation(async command =>
      command === 'get_config_cmd' ? stored : undefined,
    )

    const moved = await moveToFreePort(
      { ...stored, alias: 'stale', is_running: false },
      8081,
    )

    expect(invoke).toHaveBeenCalledWith('update_config_cmd', {
      config: { ...stored, local_port: 8081 },
    })
    expect(moved).toEqual({ ...stored, is_running: false, local_port: 8081 })
  })
})
