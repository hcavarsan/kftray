import { useCallback, useEffect, useMemo, useRef } from 'react'

import { useQueries, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { useTauriEvent } from '@/hooks/useTauriEvent'
import type { Config, ResolvedGroup } from '@/types'

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

type Pods = Map<number, string | null>

export const NO_PODS: Pods = new Map()

const samePods = (a: Pods, b: Pods) =>
  a.size === b.size && [...a].every(([id, pod]) => b.get(id) === pod)

export function useGroupActivePods(groups: ResolvedGroup[], activePods: Pods) {
  const previous = useRef(new Map<string, Pods>())

  return useMemo(() => {
    const next = new Map<string, Pods>()

    for (const group of groups) {
      const pods: Pods = new Map(
        group.configs.map(config => [
          config.id,
          activePods.get(config.id) ?? null,
        ]),
      )
      const earlier = previous.current.get(group.id)

      next.set(group.id, earlier && samePods(earlier, pods) ? earlier : pods)
    }
    previous.current = next

    return next
  }, [groups, activePods])
}
