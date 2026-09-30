import { describe, expect, it, vi } from 'vitest'

import { QueryClient } from '@tanstack/react-query'

import { configsQuery } from '@/hooks/useConfigs'
import type { Config } from '@/types'

import { applyConfigsToCache } from './configCache'
import { makeConfig } from './testFixtures'

describe('applyConfigsToCache', () => {
  it('invalidates an in-flight refresh so a stale fetch cannot resurrect a removed config', async () => {
    const client = new QueryClient()
    const stale = [makeConfig(1), makeConfig(2)]
    const refresh = Promise.withResolvers<Config[]>()

    client.setQueryData(configsQuery.queryKey, stale)
    const fetching = Promise.allSettled([
      client.fetchQuery({
        queryKey: configsQuery.queryKey,
        queryFn: () => refresh.promise,
      }),
    ])

    await applyConfigsToCache(client, current =>
      current.filter(config => config.id !== 2),
    )
    refresh.resolve(stale)
    await fetching

    expect(client.getQueryData(configsQuery.queryKey)).toEqual([makeConfig(1)])
  })

  it('leaves an absent cache untouched instead of running the update', async () => {
    const client = new QueryClient()
    const update = vi.fn((current: Config[]) => current)

    await applyConfigsToCache(client, update)

    expect(update).not.toHaveBeenCalled()
    expect(client.getQueryData(configsQuery.queryKey)).toBeUndefined()
  })
})
