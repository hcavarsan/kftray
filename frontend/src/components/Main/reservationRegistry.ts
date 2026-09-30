import type { RefObject } from 'react'

import type { Config, PendingConfigAction, PortForwardAction } from '@/types'

export type PendingMap = Map<number, PendingConfigAction>

export function ownsReservation(
  pending: PendingMap,
  id: number,
  token: number,
) {
  return pending.get(id)?.token === token
}

export function releaseReservation(
  pending: PendingMap,
  id: number,
  token: number,
) {
  if (!ownsReservation(pending, id, token)) {
    return false
  }
  pending.delete(id)

  return true
}

export function settledTimedOutReservations(
  pending: PendingMap,
  configs: Config[],
) {
  const runningById = new Map(
    configs.map(config => [config.id, config.is_running]),
  )
  const settled: { id: number; token: number }[] = []

  for (const [id, reservation] of pending) {
    if (!reservation.timedOut) {
      continue
    }
    const isRunning = runningById.get(id)

    if (
      (reservation.action === 'starting' && isRunning === true) ||
      (reservation.action === 'stopping' && isRunning === false)
    ) {
      settled.push({ id, token: reservation.token })
    }
  }

  return settled
}

export function markTimedOut(
  pending: PendingMap,
  ids: Iterable<number>,
  tokens: Map<number, number>,
) {
  let marked = false

  for (const id of ids) {
    const reservation = pending.get(id)

    if (
      reservation &&
      reservation.token === tokens.get(id) &&
      !reservation.timedOut
    ) {
      pending.set(id, { ...reservation, timedOut: true })
      marked = true
    }
  }

  return marked
}

export function createReservationRegistry(
  publish: (snapshot: PendingMap) => void,
) {
  const pendingConfigActionsRef: RefObject<PendingMap> = { current: new Map() }
  const inFlightRef: RefObject<Map<number, number>> = { current: new Map() }
  let tokenCounter = 0

  const publishPending = () => publish(new Map(pendingConfigActionsRef.current))

  const nextToken = () => ++tokenCounter

  const markPending = (id: number, action: PortForwardAction) => {
    const token = nextToken()

    pendingConfigActionsRef.current.set(id, { action, token })
    publishPending()

    return token
  }

  const clearPending = (id: number, token: number) => {
    if (releaseReservation(pendingConfigActionsRef.current, id, token)) {
      publishPending()
    }
  }

  const isBusy = (id: number) =>
    pendingConfigActionsRef.current.has(id) || inFlightRef.current.has(id)

  return {
    pendingConfigActionsRef,
    inFlightRef,
    nextToken,
    publishPending,
    markPending,
    clearPending,
    isBusy,
  }
}
