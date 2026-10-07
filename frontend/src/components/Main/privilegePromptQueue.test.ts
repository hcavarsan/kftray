import { describe, expect, it, vi } from 'vitest'

import type { PrivilegeNeed } from '@/components/PrivilegeDialog/types'

import { createPrivilegePromptQueue } from './privilegePromptQueue'

const need = (id: number): PrivilegeNeed => ({
  config_id: id,
  resource: 'hosts_file',
  detail: `svc-${id}.local`,
})

describe('createPrivilegePromptQueue', () => {
  it('shows one prompt at a time and answers each start with its own decision', async () => {
    const onShow = vi.fn()
    const queue = createPrivilegePromptQueue(onShow)
    const first = queue.ask([need(1)], null)
    const second = queue.ask([need(2)], null)

    expect(onShow).toHaveBeenCalledTimes(1)
    expect(onShow.mock.calls[0][0].needs).toEqual([need(1)])

    queue.resolve('cancel')
    await expect(first).resolves.toBe('cancel')
    expect(onShow).toHaveBeenCalledTimes(2)
    expect(onShow.mock.calls[1][0].needs).toEqual([need(2)])

    queue.resolve('continue')
    await expect(second).resolves.toBe('continue')
    expect(onShow).toHaveBeenLastCalledWith(null)
  })

  it('lets queued starts continue once the user asked not to be prompted', async () => {
    const onShow = vi.fn()
    const queue = createPrivilegePromptQueue(onShow)
    const first = queue.ask([need(1)], null)
    const second = queue.ask([need(2)], null)

    queue.drain()
    queue.resolve('continue')

    await expect(first).resolves.toBe('continue')
    await expect(second).resolves.toBe('continue')
    expect(onShow).toHaveBeenCalledTimes(2)
    expect(onShow).toHaveBeenLastCalledWith(null)
  })
})
