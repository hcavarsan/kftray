import { useCallback } from 'react'

import { useQueryClient } from '@tanstack/react-query'

import { configsQuery } from '@/hooks/useConfigs'
import type { Config } from '@/types'

import { applyConfigsToCache } from './configCache'

export function useConfigCache() {
  const queryClient = useQueryClient()

  const refreshConfigs = useCallback(
    () => queryClient.invalidateQueries({ queryKey: configsQuery.queryKey }),
    [queryClient],
  )

  const applyConfigs = useCallback(
    (update: (current: Config[]) => Config[]) =>
      applyConfigsToCache(queryClient, update),
    [queryClient],
  )

  return { refreshConfigs, applyConfigs }
}
