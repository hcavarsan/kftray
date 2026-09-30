import { type RefObject, useEffect, useState } from 'react'

import type { Config, PendingConfigAction, PortForwardAction } from '@/types'

import {
  createReservationRegistry,
  type PendingMap,
  settledTimedOutReservations,
} from './reservationRegistry'

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
  const [pendingConfigActions, setPendingConfigActions] = useState<PendingMap>(
    new Map(),
  )
  const [registry] = useState(() =>
    createReservationRegistry(setPendingConfigActions),
  )
  const { clearPending } = registry

  useEffect(() => {
    for (const { id, token } of settledTimedOutReservations(
      pendingConfigActions,
      configs,
    )) {
      clearPending(id, token)
    }
  }, [configs, pendingConfigActions, clearPending])

  return { pendingConfigActions, ...registry }
}
