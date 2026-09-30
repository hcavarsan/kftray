import { useCallback, useMemo, useRef } from 'react'

import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import type {
  Config,
  ConfigView,
  ConfigViewResult,
  Facet,
  PendingConfigAction,
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
    },
    placeholderData: keepPreviousData,
    meta: { errorToast: { title: 'Failed to load view' } },
  })

  const { mutate: saveView } = useMutation({
    mutationFn: (view: ConfigView) => invoke('set_config_view_cmd', { view }),
    meta: { errorToast: { title: 'Failed to save view' } },
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
      saveView(view)
    },
    [queryClient, queryKey, saveView],
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

type Pending = Map<number, PendingConfigAction>

export const NO_PENDING: Pending = new Map()

const samePending = (a: Pending, b: Pending) =>
  a.size === b.size && [...a].every(([id, action]) => b.get(id) === action)

export function useGroupPendingActions(
  groups: ResolvedGroup[],
  pendingConfigActions: Pending,
) {
  const previous = useRef(new Map<string, Pending>())

  return useMemo(() => {
    const next = new Map<string, Pending>()

    for (const group of groups) {
      const pending: Pending = new Map()

      for (const config of group.configs) {
        const action = pendingConfigActions.get(config.id)

        if (action) {
          pending.set(config.id, action)
        }
      }
      const earlier = previous.current.get(group.id)

      next.set(
        group.id,
        earlier && samePending(earlier, pending) ? earlier : pending,
      )
    }
    previous.current = next

    return next
  }, [groups, pendingConfigActions])
}
