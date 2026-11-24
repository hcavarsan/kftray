import type * as React from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type * as ReactQuery from '@tanstack/react-query'

import {
  type ErrorToastMeta,
  showErrorToast,
} from '@/components/ui/error-toast'
import { toaster } from '@/components/ui/toaster'

import { UpdateSettings } from './UpdateSettings'

interface CapturedMutation {
  onSuccess?: (result: Record<string, string>) => void
  meta?: ErrorToastMeta
}

const mutations = vi.hoisted(() => [] as CapturedMutation[])

vi.mock('react', async importOriginal => ({
  ...(await importOriginal<typeof React>()),
  useState: <T>(initial: T) => [initial, vi.fn()],
}))
vi.mock('@tanstack/react-query', async importOriginal => ({
  ...(await importOriginal<typeof ReactQuery>()),
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
  useQuery: () => ({ data: '1.0.0' }),
  useMutation: (options: CapturedMutation) => {
    mutations.push(options)

    return { mutate: vi.fn(), isPending: false }
  },
}))
vi.mock('@tauri-apps/api', () => ({ app: { getVersion: vi.fn() } }))
vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))
vi.mock('@/lib/problemReports', () => ({
  getProblemReportContext: () => undefined,
  requestProblemReport: vi.fn(),
}))
vi.mock('@/components/ui/toaster', () => ({
  toaster: { success: vi.fn(), error: vi.fn(), create: vi.fn() },
}))

const renderMutations = () => {
  mutations.length = 0
  UpdateSettings({
    autoUpdateEnabled: true,
    onAutoUpdateEnabledChange: vi.fn(),
    lastUpdateCheck: undefined,
  })
  const [check, install] = mutations

  return { check, install }
}

describe('UpdateSettings', () => {
  beforeEach(() => {
    vi.mocked(toaster.error).mockClear()
  })

  it('shows why the install failed', () => {
    const { install } = renderMutations()

    showErrorToast('Failed to install update: permission denied', install.meta)

    expect(toaster.error).toHaveBeenCalledWith(
      expect.objectContaining({
        description: 'Failed to install update: permission denied',
      }),
    )
  })

  it('shows why the update check failed', () => {
    const { check } = renderMutations()

    check.onSuccess?.({
      current_version: '1.0.0',
      update_available: 'error',
      latest_version: 'unknown',
      error: 'Failed to check for updates: invalid signature',
    })

    expect(toaster.error).toHaveBeenCalledWith(
      expect.objectContaining({
        description: 'Failed to check for updates: invalid signature',
      }),
    )
  })
})
