import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { PrivilegeNeed } from '@/components/PrivilegeDialog/types'
import { invoke } from '@/lib/tauri'

import {
  gateStart,
  PRIVILEGE_ERROR,
  type PrivilegeDecision,
} from './privilegeGate'
import { makeConfig } from './testFixtures'

vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))

const hostsNeed: PrivilegeNeed = {
  config_id: 1,
  resource: 'hosts_file',
  detail: 'svc.local',
}

describe('gateStart', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
  })

  it('starts without asking when nothing needs privileges', async () => {
    vi.mocked(invoke).mockResolvedValue([])
    const prompt = vi.fn()

    await expect(
      gateStart([makeConfig(1)], { prompt, isSuppressed: async () => false }),
    ).resolves.toBe(true)
    expect(prompt).not.toHaveBeenCalled()
  })

  it('passes the reported needs to the prompt and honours its answer', async () => {
    vi.mocked(invoke).mockResolvedValue([hostsNeed])
    const prompt = vi.fn<
      (needs: PrivilegeNeed[]) => Promise<PrivilegeDecision>
    >(async () => 'cancel')

    await expect(
      gateStart([makeConfig(1)], { prompt, isSuppressed: async () => false }),
    ).resolves.toBe(false)
    expect(prompt).toHaveBeenCalledWith([hostsNeed])

    prompt.mockResolvedValueOnce('continue')
    await expect(
      gateStart([makeConfig(1)], { prompt, isSuppressed: async () => false }),
    ).resolves.toBe(true)
  })

  it('skips the prompt once the user asked not to be asked', async () => {
    vi.mocked(invoke).mockResolvedValue([hostsNeed])
    const prompt = vi.fn()

    await expect(
      gateStart([makeConfig(1)], { prompt, isSuppressed: async () => true }),
    ).resolves.toBe(true)
    expect(prompt).not.toHaveBeenCalled()
  })

  it('does not block a start when the preflight itself fails', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('no backend'))
    const prompt = vi.fn()

    await expect(
      gateStart([makeConfig(1)], { prompt, isSuppressed: async () => false }),
    ).resolves.toBe(true)
    expect(prompt).not.toHaveBeenCalled()
  })
})

describe('PRIVILEGE_ERROR', () => {
  it('matches the backend messages for refused local changes', () => {
    expect(
      PRIVILEGE_ERROR.test(
        'Failed to write to the hostfile for svc: Permission denied (os error 13). Domain alias feature requires hostfile access.',
      ),
    ).toBe(true)
    expect(
      PRIVILEGE_ERROR.test(
        'Failed to write to the hostfile for svc: Access is denied. (os error 5). Domain alias feature requires hostfile access.',
      ),
    ).toBe(true)
    expect(
      PRIVILEGE_ERROR.test(
        'Custom loopback address configuration cancelled: User cancelled loopback address configuration',
      ),
    ).toBe(true)
    expect(
      PRIVILEGE_ERROR.test(
        'Address allocation cancelled by user: Failed to configure loopback address: User canceled',
      ),
    ).toBe(true)
  })

  it('leaves other permission, hosts and network failures alone', () => {
    expect(PRIVILEGE_ERROR.test('pod not found')).toBe(false)
    expect(
      PRIVILEGE_ERROR.test(
        'Failed to load kubeconfig: Permission denied (os error 13)',
      ),
    ).toBe(false)
    // A hosts write refused for the alias itself; elevation would not help.
    expect(
      PRIVILEGE_ERROR.test(
        'Failed to write to the hostfile for svc: Invalid data: Invalid hostname "bad alias". Domain alias feature requires hostfile access.',
      ),
    ).toBe(false)
    expect(
      PRIVILEGE_ERROR.test(
        'Failed to add HTTPS hosts entries: IO error: No space left on device',
      ),
    ).toBe(false)
    expect(
      PRIVILEGE_ERROR.test(
        'Failed to bind listener on loopback address 127.0.0.1:8080',
      ),
    ).toBe(false)
  })
})
