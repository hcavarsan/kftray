import type { QueryClient } from '@tanstack/react-query'

import { configsQuery } from '@/hooks/useConfigs'
import type { Config } from '@/types'

export async function applyConfigsToCache(
  queryClient: QueryClient,
  update: (current: Config[]) => Config[],
) {
  await queryClient.cancelQueries({
    queryKey: configsQuery.queryKey,
    exact: true,
  })
  queryClient.setQueryData(configsQuery.queryKey, current =>
    current ? update(current) : current,
  )
}
