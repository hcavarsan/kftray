import { queryOptions, useQuery } from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'
import type { Config, ForwardState } from '@/types'

type ConfigStateRow = Required<ForwardState> & { config_id: number }

export async function fetchConfigsWithState(): Promise<Config[]> {
  const [configs, states] = await Promise.all([
    invoke<Config[]>('get_configs_cmd'),
    invoke<ConfigStateRow[]>('get_config_states'),
  ])
  const stateById = new Map(states.map(s => [s.config_id, s]))

  return configs.map(config => {
    const state = stateById.get(config.id)

    return {
      ...config,
      is_running: state?.is_running ?? false,
      is_retrying: state?.is_retrying ?? false,
      retry_count: state?.retry_count ?? null,
      last_error: state?.last_error ?? null,
    }
  })
}

export const configsQuery = queryOptions({
  queryKey: ['configs'],
  queryFn: fetchConfigsWithState,
})

export const useConfigs = () => useQuery(configsQuery)
