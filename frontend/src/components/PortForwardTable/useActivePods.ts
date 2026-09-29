import { useEffect, useMemo, useState } from 'react'

import { useQueries } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { useTauriEvent } from '@/hooks/useTauriEvent'
import type { Config } from '@/types'

export interface ActivePodChangedPayload {
  configId: string
  podName: string | null
}

export function useActivePods(configs: Config[]) {
  const runningIds = useMemo(
    () => configs.filter(config => config.is_running).map(config => config.id),
    [configs],
  )
  const [overrides, setOverrides] = useState<Map<number, string | null>>(
    new Map(),
  )

  const initialResults = useQueries({
    queries: runningIds.map(id => ({
      queryKey: ['active-pod', id],
      queryFn: () =>
        invoke<string | null>('get_active_pod_cmd', {
          configId: id.toString(),
        }),
    })),
  })

  useTauriEvent<ActivePodChangedPayload>('active_pod_changed', event => {
    const id = Number(event.payload.configId)

    setOverrides(prev => {
      const next = new Map(prev)

      next.set(id, event.payload.podName)

      return next
    })
  })

  useEffect(() => {
    const runningSet = new Set(runningIds)

    setOverrides(prev => {
      const next = new Map(prev)
      let changed = false

      for (const id of next.keys()) {
        if (!runningSet.has(id)) {
          next.delete(id)
          changed = true
        }
      }

      return changed ? next : prev
    })
  }, [runningIds])

  return useMemo(() => {
    const map = new Map<number, string | null>()

    runningIds.forEach((id, index) => {
      map.set(id, overrides.get(id) ?? initialResults[index]?.data ?? null)
    })

    return map
  }, [runningIds, overrides, initialResults])
}
