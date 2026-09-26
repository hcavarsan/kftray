import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { invoke } from '@tauri-apps/api/core'

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
  const [result, setResult] = useState<ConfigViewResult | null>(null)
  const requestRef = useRef(0)
  const viewRef = useRef<ConfigView | null>(null)
  const signature = useMemo(() => viewSignature(configs), [configs])

  const query = useCallback(async (view: ConfigView | null) => {
    const request = ++requestRef.current

    try {
      const next = await invoke<ConfigViewResult>('query_config_view_cmd', {
        view,
      })

      if (request === requestRef.current) {
        viewRef.current = next.view
        setResult(next)
      }
    } catch (error) {
      console.error('Failed to query config view:', error)
    }
  }, [])

  // biome-ignore lint/correctness/useExhaustiveDependencies: refetch when grouping-relevant config fields change
  useEffect(() => {
    void query(viewRef.current)
  }, [signature, query])

  const setView = useCallback(
    (view: ConfigView) => {
      viewRef.current = view
      setResult(prev => (prev ? { ...prev, view } : prev))
      void query(view)
      invoke('set_config_view_cmd', { view }).catch(error =>
        console.error('Failed to save config view:', error),
      )
    },
    [query],
  )

  const groups = useMemo((): ResolvedGroup[] => {
    if (!result) {
      return visibleConfigs.length
        ? [{ id: ALL_GROUP_ID, label: 'All', configs: visibleConfigs }]
        : []
    }
    const byId = new Map(visibleConfigs.map(c => [c.id, c]))

    return result.groups
      .map(group => ({
        id: groupId(group.key),
        label: group.label,
        configs: group.config_ids
          .map(id => byId.get(id))
          .filter((c): c is Config => c !== undefined),
      }))
      .filter(group => group.configs.length > 0)
  }, [result, visibleConfigs])

  return {
    view: result?.view ?? null,
    facets: result?.facets ?? NO_FACETS,
    groups,
    setView,
  }
}
