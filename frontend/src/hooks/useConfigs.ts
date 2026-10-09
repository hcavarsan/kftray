import { queryOptions, useQuery } from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'
import type { Config } from '@/types'

export async function fetchConfigsWithState(): Promise<Config[]> {
  const [configs, states] = await Promise.all([
    invoke<Config[]>('get_configs_cmd'),
    invoke<{ config_id: number; is_running: boolean }[]>('get_config_states'),
  ])
  const runningById = new Map(states.map(s => [s.config_id, s.is_running]))

  return configs.map(config => ({
    ...config,
    is_running: runningById.get(config.id) ?? false,
  }))
}

export const configsQuery = queryOptions({
  queryKey: ['configs'],
  queryFn: fetchConfigsWithState,
})

export const useConfigs = () => useQuery(configsQuery)
