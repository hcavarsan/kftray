import { type RefObject, useCallback, useEffect, useRef, useState } from 'react'

import type { Config, PendingConfigAction, PortForwardAction } from '@/types'

export interface PendingActions {
  pendingConfigActions: Map<number, PendingConfigAction>
  pendingConfigActionsRef: RefObject<Map<number, PendingConfigAction>>
  inFlightRef: RefObject<Map<number, number>>
  nextToken: () => number
  publishPending: () => void
  markPending: (id: number, action: PortForwardAction) => number
  clearPending: (id: number, token: number) => void
  isBusy: (id: number) => boolean
}

export function usePendingActions(configs: Config[]): PendingActions {
  const pendingConfigActionsRef = useRef<Map<number, PendingConfigAction>>(
    new Map(),
  )
  const [pendingConfigActions, setPendingConfigActions] = useState<
    Map<number, PendingConfigAction>
  >(new Map())
  const pendingTokenCounterRef = useRef(0)
  const inFlightRef = useRef<Map<number, number>>(new Map())

  const publishPending = useCallback(
    () => setPendingConfigActions(new Map(pendingConfigActionsRef.current)),
    [],
  )

  const nextToken = useCallback(() => ++pendingTokenCounterRef.current, [])

  const markPending = useCallback(
    (id: number, action: PortForwardAction) => {
      const token = nextToken()

      pendingConfigActionsRef.current.set(id, { action, token })
      publishPending()

      return token
    },
    [nextToken, publishPending],
  )

  // Only releases the reservation it created: a caller holding a stale token
  // (superseded by a later markPending for the same id) must not clear
  // someone else's in-flight reservation.
  const clearPending = useCallback(
    (id: number, token: number) => {
      if (pendingConfigActionsRef.current.get(id)?.token !== token) {
        return
      }
      pendingConfigActionsRef.current.delete(id)
      publishPending()
    },
    [publishPending],
  )

  const isBusy = useCallback(
    (id: number) =>
      pendingConfigActionsRef.current.has(id) || inFlightRef.current.has(id),
    [],
  )

  useEffect(() => {
    // A grace-timeout can mark a reservation `timedOut` when its invoke
    // never settles; once the backend state catches up with the action
    // that reservation was waiting for, release it instead of leaving
    // the row disabled for the lifetime of the app.
    const runningById = new Map(
      configs.map(config => [config.id, config.is_running]),
    )

    for (const [id, pending] of pendingConfigActionsRef.current) {
      if (!pending.timedOut) {
        continue
      }
      const isRunning = runningById.get(id)

      if (
        (pending.action === 'starting' && isRunning === true) ||
        (pending.action === 'stopping' && isRunning === false)
      ) {
        clearPending(id, pending.token)
      }
    }
  }, [configs, clearPending])

  return {
    pendingConfigActions,
    pendingConfigActionsRef,
    inFlightRef,
    nextToken,
    publishPending,
    markPending,
    clearPending,
    isBusy,
  }
}
