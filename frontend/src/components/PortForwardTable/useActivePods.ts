import { useCallback, useEffect, useMemo, useRef } from 'react'

import { useQueries, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { useTauriEvent } from '@/hooks/useTauriEvent'
import type { Config } from '@/types'

interface ActivePodChangedPayload {
  configId: string
  podName: string | null
}

const activePodKey = (id: number) => ['active-pod', id]

export function useActivePods(configs: Config[]) {
  const queryClient = useQueryClient()
  const runningIds = useMemo(
    () => configs.filter(config => config.is_running).map(config => config.id),
    [configs],
  )
  const previousRunningIds = useRef<number[]>([])

  useEffect(() => {
    const running = new Set(runningIds)

    for (const id of previousRunningIds.current) {
      if (!running.has(id)) {
        queryClient.removeQueries({ queryKey: activePodKey(id) })
      }
    }
    previousRunningIds.current = runningIds
  }, [runningIds, queryClient])

  useTauriEvent<ActivePodChangedPayload>('active_pod_changed', event => {
    queryClient.setQueryData(
      activePodKey(Number(event.payload.configId)),
      event.payload.podName,
    )
  })

  const combine = useCallback(
    (results: { data?: string | null }[]) =>
      new Map(
        runningIds.map((id, index) => [id, results[index]?.data ?? null]),
      ),
    [runningIds],
  )

  return useQueries({
    queries: runningIds.map(id => ({
      queryKey: activePodKey(id),
      queryFn: () =>
        invoke<string | null>('get_active_pod_cmd', {
          configId: id.toString(),
        }),
    })),
    combine,
  })
}
