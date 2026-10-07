import { beforeEach, describe, expect, it, vi } from 'vitest'

import { invoke } from '@tauri-apps/api/core'

import type { PrivilegeNeed } from '@/components/PrivilegeDialog/types'

import {
  gateStart,
  PRIVILEGE_ERROR,
  type PrivilegeDecision,
} from './privilegeGate'
import { makeConfig } from './testFixtures'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

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
        'Failed to write to the hostfile for svc: Permission denied (os error 13)',
      ),
    ).toBe(true)
    expect(
      PRIVILEGE_ERROR.test(
        'Custom loopback address configuration cancelled: User cancelled',
      ),
    ).toBe(true)
    expect(PRIVILEGE_ERROR.test('pod not found')).toBe(false)
  })
})
