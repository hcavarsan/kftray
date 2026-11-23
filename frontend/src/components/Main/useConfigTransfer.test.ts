import { beforeEach, describe, expect, it, vi } from 'vitest'

import { QueryClient, QueryObserver } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { readTextFile } from '@tauri-apps/plugin-fs'

import { httpLogsEnabledQueryKey } from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/useHttpLogsEnabled'
import {
  type ErrorToastMeta,
  showErrorToast,
} from '@/components/ui/error-toast'
import { toaster } from '@/components/ui/toaster'
import { invoke } from '@/lib/tauri'

import { useConfigTransfer } from './useConfigTransfer'

const mutations = vi.hoisted(
  () => [] as { mutationFn: () => Promise<unknown>; meta?: ErrorToastMeta }[],
)
const client = vi.hoisted(() => ({ current: undefined as unknown }))

vi.mock('react', () => ({ useCallback: <T>(fn: T) => fn }))
vi.mock('@tanstack/react-query', async importOriginal => ({
  ...(await importOriginal<typeof import('@tanstack/react-query')>()),
  useQueryClient: () => client.current,
  useMutation: (options: {
    mutationFn: () => Promise<unknown>
    meta?: ErrorToastMeta
  }) => {
    mutations.push(options)

    return { mutate: vi.fn() }
  },
}))
vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))
vi.mock('@/lib/nativeDialog', () => ({
  withNativeDialog: <T>(run: () => Promise<T>) => run(),
}))
vi.mock('@/components/ui/toaster', () => ({
  toaster: { error: vi.fn(), success: vi.fn() },
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }))
vi.mock('@tauri-apps/plugin-fs', () => ({
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
}))

describe('useConfigTransfer import', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
    vi.mocked(toaster.error).mockReset()
    mutations.length = 0
  })

  it('refreshes the http logs flag of rows already in the list', async () => {
    let httpLogsEnabled = false

    vi.mocked(invoke).mockImplementation(async command => {
      if (command === 'import_configs_cmd') {
        httpLogsEnabled = true

        return undefined
      }

      return command === 'get_http_logs_cmd' ? httpLogsEnabled : []
    })
    vi.mocked(open).mockResolvedValue('/tmp/configs.json')
    vi.mocked(readTextFile).mockResolvedValue('[]')

    const queryClient = new QueryClient()

    client.current = queryClient

    const observer = new QueryObserver(queryClient, {
      queryKey: httpLogsEnabledQueryKey(7),
      queryFn: () => invoke<boolean>('get_http_logs_cmd', { configId: 7 }),
    })
    const unsubscribe = observer.subscribe(() => undefined)

    await vi.waitFor(() =>
      expect(queryClient.getQueryData(httpLogsEnabledQueryKey(7))).toBe(false),
    )

    useConfigTransfer()

    const importMutation = mutations[1]

    await importMutation.mutationFn()

    await vi.waitFor(() =>
      expect(queryClient.getQueryData(httpLogsEnabledQueryKey(7))).toBe(true),
    )
    unsubscribe()
  })

  it.each([
    [
      'empty namespace',
      'Error migrating configs: Invalid config: Namespace is required and cannot be empty',
      'Namespace is required and cannot be empty',
    ],
    [
      'malformed JSON',
      'Error migrating configs: Failed to parse config: expected value at line 1 column 1',
      'Failed to parse config: expected value at line 1 column 1',
    ],
    ['oversized rejection', 'x'.repeat(10000), 'x'.repeat(511) + '…'],
    [
      'blank visible prefix',
      ' '.repeat(10000) + 'late detail',
      'Failed to import configurations.',
    ],
    ['empty message', '', 'Failed to import configurations.'],
    [
      'whitespace-only Error',
      new Error('   '),
      'Failed to import configurations.',
    ],
  ])(
    'shows the %s reason in the import error toast',
    async (_scenario, rejection, expectedReason) => {
      vi.mocked(open).mockResolvedValue('/tmp/configs.json')
      vi.mocked(readTextFile).mockResolvedValue('[]')
      vi.mocked(invoke).mockRejectedValue(rejection)
      useConfigTransfer()

      const importMutation = mutations[1]

      await importMutation
        .mutationFn()
        .catch(error => showErrorToast(error, importMutation.meta))

      expect(toaster.error).toHaveBeenCalledOnce()
      const [toast] = vi.mocked(toaster.error).mock.calls[0]

      expect(toast?.description).toContain(expectedReason)
      expect(toast?.description?.toString().length).toBeLessThanOrEqual(512)
      expect(toast?.duration).toBeGreaterThanOrEqual(8000)
    },
  )
})
