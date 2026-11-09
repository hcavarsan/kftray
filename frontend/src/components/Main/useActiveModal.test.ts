import { beforeEach, describe, expect, it, vi } from 'vitest'

import type * as ReactQuery from '@tanstack/react-query'
import { QueryClient } from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'
import type { StoredConfig } from '@/types'

import { makeConfig } from './testFixtures'
import { useActiveModal } from './useActiveModal'

const state = vi.hoisted(() => ({
  value: { activeModal: null as unknown, modalKey: 0 },
  ref: { current: 0 },
  client: undefined as unknown,
}))

vi.mock('react', () => ({
  useCallback: <T>(callback: T) => callback,
  useRef: () => state.ref,
  useState: () => [
    state.value,
    (value: unknown) => {
      state.value = value as typeof state.value
    },
  ],
}))
vi.mock('@tanstack/react-query', async importOriginal => ({
  ...(await importOriginal<typeof ReactQuery>()),
  useQueryClient: () => state.client,
}))
vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))

const first = { ...makeConfig(1), alias: 'api-a', local_port: 18080 }
const second = {
  ...makeConfig(2),
  alias: 'database-b',
  local_port: 15432,
  remote_port: 5432,
}

function usePendingLoads() {
  const loads = [
    Promise.withResolvers<StoredConfig>(),
    Promise.withResolvers<StoredConfig>(),
  ]
  vi.mocked(invoke)
    .mockReturnValueOnce(loads[0].promise)
    .mockReturnValueOnce(loads[1].promise)
  const modal = useActiveModal()
  const requests = [modal.openConfig(1, true), modal.openConfig(2, true)]
  return { loads, modal, requests }
}

describe('config editor requests', () => {
  beforeEach(() => {
    state.ref.current = 0
    state.value = { activeModal: null, modalKey: 0 }
    state.client = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    })
    vi.mocked(invoke).mockReset()
  })

  it.each([
    [0, 1],
    [1, 0],
  ])(
    'keeps the last requested editor when response %i finishes first',
    async (a, b) => {
      const { loads, requests } = usePendingLoads()
      const configs = [first, second]

      loads[a].resolve(configs[a])
      await requests[a]
      const afterFirstResponse = state.value.activeModal
      loads[b].resolve(configs[b])
      await requests[b]

      expect({ afterFirstResponse, final: state.value.activeModal }).toEqual({
        afterFirstResponse:
          a === 0
            ? null
            : expect.objectContaining({ initialConfig: second, isEdit: true }),
        final: expect.objectContaining({ initialConfig: second, isEdit: true }),
      })
    },
  )

  it.each([null, { type: 'settings' } as const])(
    'does not reopen an editor after selecting %j',
    async next => {
      const { loads, modal, requests } = usePendingLoads()
      modal.setActiveModal(next)
      loads[0].resolve(first)
      loads[1].resolve(second)
      await Promise.all(requests)

      expect(state.value.activeModal).toEqual(next)
    },
  )

  it('does not restore an obsolete editor when the newer load fails', async () => {
    const { loads, requests } = usePendingLoads()
    loads[1].reject(new Error('Configuration no longer exists'))
    await requests[1]
    loads[0].resolve(first)
    await requests[0]

    expect(state.value.activeModal).toBeNull()
  })

  it('keeps a new config open when an older edit finishes', async () => {
    const { loads, modal, requests } = usePendingLoads()
    const next = { type: 'config', initialConfig: null, isEdit: false } as const
    modal.setActiveModal(next)
    loads[0].resolve(first)
    loads[1].resolve(second)
    await Promise.all(requests)

    expect(state.value.activeModal).toEqual(next)
  })

  it('keeps duplicate mode when an older edit finishes', async () => {
    const loads = [
      Promise.withResolvers<StoredConfig>(),
      Promise.withResolvers<StoredConfig>(),
    ]
    vi.mocked(invoke)
      .mockReturnValueOnce(loads[0].promise)
      .mockReturnValueOnce(loads[1].promise)
    const modal = useActiveModal()
    const edit = modal.openConfig(1, true)
    const duplicate = modal.openConfig(2, false)
    loads[1].resolve(second)
    await duplicate
    loads[0].resolve(first)
    await edit

    expect(state.value.activeModal).toEqual({
      type: 'config',
      isEdit: false,
      initialConfig: { ...second, id: 0, alias: 'database-b-copy' },
    })
  })
})
