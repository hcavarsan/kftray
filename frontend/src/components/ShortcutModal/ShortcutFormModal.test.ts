import type * as ReactModule from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type * as ReactQueryModule from '@tanstack/react-query'

import { showErrorToast } from '@/components/ui/error-toast'
import { toaster } from '@/components/ui/toaster'
import { useGlobalShortcuts } from '@/hooks/useGlobalShortcuts'

import { ShortcutFormModal } from './ShortcutFormModal'

type MutationOptions = {
  meta?: Parameters<typeof showErrorToast>[1]
  onError?: (error: unknown) => void
}
const mutations = vi.hoisted(() => [] as MutationOptions[])

vi.mock('react', async importOriginal => ({
  ...(await importOriginal<typeof ReactModule>()),
  useState: (init: unknown) => [
    typeof init === 'function' ? (init as () => unknown)() : init,
    vi.fn(),
  ],
}))
vi.mock('@tanstack/react-query', async importOriginal => ({
  ...(await importOriginal<typeof ReactQueryModule>()),
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
  useQuery: () => ({ data: undefined }),
  useMutation: (options: MutationOptions) => {
    mutations.push(options)

    return { mutate: vi.fn(), isPending: false }
  },
}))
vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))
vi.mock('@/components/ui/toaster', () => ({
  toaster: { success: vi.fn(), error: vi.fn() },
}))

describe('shortcut error toasts', () => {
  beforeEach(() => {
    mutations.length = 0
    vi.mocked(toaster.error).mockClear()
  })

  it('shows why saving a shortcut failed', () => {
    ShortcutFormModal({ shortcut: null, configs: [], onClose: vi.fn() })

    mutations[0].onError?.(
      'Platform error: Register failed: HotKey already registered: HotKey { mods: CONTROL | SHIFT, key: KeyK }',
    )

    expect(
      vi.mocked(toaster.error).mock.calls.at(-1)?.[0].description,
    ).toContain('HotKey already registered')
  })

  it.each([
    ['delete', 0, 'Shortcut not found: 7'],
    [
      'fix permissions',
      1,
      'Platform error: Failed to execute usermod command: No such file or directory (os error 2)',
    ],
  ])('shows why %s failed', (_, index, reason) => {
    useGlobalShortcuts()

    showErrorToast(reason, mutations[index].meta)

    expect(vi.mocked(toaster.error).mock.calls.at(-1)?.[0].description).toBe(
      reason,
    )
  })
})
