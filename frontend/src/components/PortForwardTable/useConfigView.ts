import { useCallback, useMemo, useRef } from 'react'

import { useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'
import type {
  Config,
  ConfigView,
  ConfigViewResult,
  Facet,
  ResolvedGroup,
} from '@/types'

const groupId = (key: string | null) => JSON.stringify(key)

export const ALL_GROUP_ID = groupId(null)

const NO_FACETS: Facet[] = []

const viewSignature = (configs: Config[]) =>
  JSON.stringify(
    configs.map(c => [
      c.id,
      c.alias,
      c.context,
      c.namespace,
      c.kubeconfig,
      c.workload_type,
      c.protocol,
      c.tags ?? {},
    ]),
  )

export const useConfigView = (configs: Config[], visibleConfigs: Config[]) => {
  const viewRef = useRef<ConfigView | null>(null)
  const viewVersionRef = useRef(0)
  const signature = useMemo(() => viewSignature(configs), [configs])
  const queryKey = useMemo(
    () => ['config-view', signature] as const,
    [signature],
  )
  const queryClient = useQueryClient()

  const { data } = useQuery({
    queryKey,
    queryFn: async () => {
      try {
        let version: number
        let result: ConfigViewResult

        do {
          version = viewVersionRef.current
          result = await invoke<ConfigViewResult>('query_config_view_cmd', {
            view: viewRef.current,
          })
        } while (version !== viewVersionRef.current)

        viewRef.current = result.view

        return result
      } catch (error) {
        toaster.error({
          title: 'Failed to load view',
          description: errorMessage(error),
        })
        throw error
      }
    },
  })

  const setView = useCallback(
    (view: ConfigView) => {
      viewRef.current = view
      viewVersionRef.current += 1
      queryClient.setQueryData(
        queryKey,
        (prev: ConfigViewResult | undefined) =>
          prev ? { ...prev, view } : prev,
      )
      void queryClient.invalidateQueries({ queryKey })
      invoke('set_config_view_cmd', { view }).catch(error =>
        toaster.error({
          title: 'Failed to save view',
          description: errorMessage(error),
        }),
      )
    },
    [queryClient, queryKey],
  )

  const groups = useMemo((): ResolvedGroup[] => {
    if (!data) {
      return visibleConfigs.length
        ? [{ id: ALL_GROUP_ID, label: 'All', configs: visibleConfigs }]
        : []
    }
    const byId = new Map(visibleConfigs.map(c => [c.id, c]))

    return data.groups
      .map(group => ({
        id: groupId(group.key),
        label: group.label,
        configs: group.config_ids
          .map(id => byId.get(id))
          .filter((c): c is Config => c !== undefined),
      }))
      .filter(group => group.configs.length > 0)
  }, [data, visibleConfigs])

  return {
    view: data?.view ?? null,
    facets: data?.facets ?? NO_FACETS,
    groups,
    setView,
  }
}
