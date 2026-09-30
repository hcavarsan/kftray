import { useCallback } from 'react'

import { useQueryClient } from '@tanstack/react-query'

import { configsQuery } from '@/hooks/useConfigs'
import type { Config } from '@/types'

export function useConfigCache() {
  const queryClient = useQueryClient()

  const refreshConfigs = useCallback(
    () => queryClient.invalidateQueries({ queryKey: configsQuery.queryKey }),
    [queryClient],
  )

  // Applies an authoritative local change and invalidates any refresh that is
  // still in flight, so a stale fetch cannot resurrect what this just removed
  // or replaced.
  const applyConfigs = useCallback(
    async (update: (current: Config[]) => Config[]) => {
      await queryClient.cancelQueries({
        queryKey: configsQuery.queryKey,
        exact: true,
      })
      queryClient.setQueryData(configsQuery.queryKey, current =>
        current ? update(current) : current,
      )
    },
    [queryClient],
  )

  return { refreshConfigs, applyConfigs }
}
