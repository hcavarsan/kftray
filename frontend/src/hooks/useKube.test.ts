import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { QueryClient, QueryObserver } from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'

import { kubeContextsQuery } from './useKube'

vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))

beforeEach(() => {
  vi.useFakeTimers()
  vi.mocked(invoke).mockReset()
})

afterEach(() => {
  vi.useRealTimers()
})

it('lists contexts once when reading the kubeconfig fails', async () => {
  vi.mocked(invoke).mockRejectedValue(new Error('unreadable kubeconfig'))
  // The app's client keeps the library default, which is 3 retries in a
  // browser but none under node.
  const client = new QueryClient({
    defaultOptions: { queries: { retry: 3 } },
  })
  const unsubscribe = new QueryObserver(
    client,
    kubeContextsQuery('/home/me/.kube/config'),
  ).subscribe(() => undefined)

  // Past the longest default retry backoff.
  await vi.advanceTimersByTimeAsync(60_000)

  expect(invoke).toHaveBeenCalledTimes(1)
  unsubscribe()
})
