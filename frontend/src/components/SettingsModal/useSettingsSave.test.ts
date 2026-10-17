import { beforeEach, describe, expect, it, vi } from 'vitest'

import type * as ReactQuery from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'

import type { SettingsDraft } from './types'
import { useSettingsSave } from './useSettingsSave'

interface CapturedMutation {
  mutationFn: () => Promise<unknown>
  onSuccess: (result: unknown) => void
}

const hooks = vi.hoisted(() => ({
  states: [] as unknown[],
  cursor: 0,
  mutation: undefined as CapturedMutation | undefined,
}))

vi.mock('react', () => ({
  useState: <T>(initial: T) => {
    const index = hooks.cursor++

    if (!(index in hooks.states)) {
      hooks.states[index] = initial
    }

    const setState = (next: T | ((prev: T) => T)) => {
      hooks.states[index] =
        typeof next === 'function'
          ? (next as (prev: T) => T)(hooks.states[index] as T)
          : next
    }

    return [hooks.states[index], setState]
  },
}))
vi.mock('@tanstack/react-query', async importOriginal => ({
  ...(await importOriginal<typeof ReactQuery>()),
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
  useMutation: (options: CapturedMutation) => {
    hooks.mutation = options

    return { mutate: vi.fn(), isPending: false }
  },
}))
vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))
vi.mock('@/components/ui/toaster', () => ({
  toaster: { success: vi.fn(), error: vi.fn() },
}))
vi.mock('@/components/ui/error-toast', () => ({ showErrorToast: vi.fn() }))

const initial: SettingsDraft = {
  disconnectTimeout: '0',
  networkMonitor: true,
  autoUpdateEnabled: true,
  telemetryEnabled: false,
  performanceEnabled: false,
  sslEnabled: false,
  sslCertValidityDays: '365',
  logRetentionCount: '10',
  logRetentionDays: '7',
  appMode: 'tray',
  mcpEnabled: false,
  mcpPort: '3000',
}

function propsFor(draft: SettingsDraft, onClose: () => void) {
  hooks.cursor = 0

  return { draft, initial, loaded: { ssl: true, log: true }, onClose }
}

async function submit() {
  const mutation = hooks.mutation as CapturedMutation
  const failures = await mutation.mutationFn()

  mutation.onSuccess(failures)

  return failures
}

describe('useSettingsSave', () => {
  beforeEach(() => {
    hooks.states = []
    hooks.mutation = undefined
    vi.mocked(invoke).mockReset()
  })

  it('sends the mcp switch again after the save that turned it on failed', async () => {
    let savedEnabled = false

    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'update_mcp_server_enabled') {
        const { enabled } = args as { enabled: boolean }

        savedEnabled = enabled
        if (enabled) {
          throw new Error('Failed to start MCP server: address in use')
        }
      }

      return undefined
    })
    const onClose = vi.fn()

    useSettingsSave(propsFor({ ...initial, mcpEnabled: true }, onClose))
    const first = await submit()

    expect(first).toHaveLength(1)
    expect(savedEnabled).toBe(true)
    expect(onClose).not.toHaveBeenCalled()

    useSettingsSave(propsFor({ ...initial, mcpEnabled: false }, onClose))
    const second = await submit()

    expect(second).toEqual([])
    expect(savedEnabled).toBe(false)
    expect(onClose).toHaveBeenCalledOnce()
  })

  it('skips a setting that did not change since its last successful save', async () => {
    vi.mocked(invoke).mockResolvedValue(undefined)

    useSettingsSave(propsFor({ ...initial, mcpEnabled: true }, vi.fn()))
    await submit()
    vi.mocked(invoke).mockClear()
    useSettingsSave(propsFor({ ...initial, mcpEnabled: true }, vi.fn()))
    await submit()

    expect(invoke).not.toHaveBeenCalled()
  })
})
