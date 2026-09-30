import { describe, expect, it, vi } from 'vitest'

import {
  createReservationRegistry,
  markTimedOut,
  type PendingMap,
  settledTimedOutReservations,
} from './reservationRegistry'
import { makeConfig } from './testFixtures'

describe('reservation registry', () => {
  it('hands out a distinct token per reservation', () => {
    const registry = createReservationRegistry(vi.fn())

    const first = registry.markPending(1, 'starting')
    const second = registry.markPending(2, 'starting')

    expect(second).not.toBe(first)
  })

  it('does not let a stale token clear a newer reservation for the same id', () => {
    const publish = vi.fn()
    const registry = createReservationRegistry(publish)
    const stale = registry.markPending(1, 'starting')
    const current = registry.markPending(1, 'stopping')

    publish.mockClear()
    registry.clearPending(1, stale)

    expect(registry.pendingConfigActionsRef.current.get(1)).toEqual({
      action: 'stopping',
      token: current,
    })
    expect(publish).not.toHaveBeenCalled()
  })

  it('clears and publishes when the token still owns the reservation', () => {
    const publish = vi.fn()
    const registry = createReservationRegistry(publish)
    const token = registry.markPending(1, 'starting')

    registry.clearPending(1, token)

    expect(registry.pendingConfigActionsRef.current.has(1)).toBe(false)
    expect(publish).toHaveBeenLastCalledWith(new Map())
  })

  it('reports an id as busy while reserved or in flight', () => {
    const registry = createReservationRegistry(vi.fn())
    const token = registry.markPending(1, 'starting')

    registry.inFlightRef.current.set(2, 7)

    expect(registry.isBusy(1)).toBe(true)
    expect(registry.isBusy(2)).toBe(true)
    expect(registry.isBusy(3)).toBe(false)
    registry.clearPending(1, token)
    expect(registry.isBusy(1)).toBe(false)
  })
})

describe('settledTimedOutReservations', () => {
  const pendingOf = (
    action: 'starting' | 'stopping',
    timedOut: boolean,
  ): PendingMap => new Map([[1, { action, token: 4, timedOut }]])

  it('releases a timed-out start once the config is running', () => {
    expect(
      settledTimedOutReservations(pendingOf('starting', true), [
        makeConfig(1, true),
      ]),
    ).toEqual([{ id: 1, token: 4 }])
  })

  it('releases a timed-out stop once the config is stopped', () => {
    expect(
      settledTimedOutReservations(pendingOf('stopping', true), [
        makeConfig(1, false),
      ]),
    ).toEqual([{ id: 1, token: 4 }])
  })

  it('keeps a timed-out reservation while the backend has not caught up', () => {
    expect(
      settledTimedOutReservations(pendingOf('starting', true), [
        makeConfig(1, false),
      ]),
    ).toEqual([])
    expect(
      settledTimedOutReservations(pendingOf('stopping', true), [
        makeConfig(1, true),
      ]),
    ).toEqual([])
    expect(
      settledTimedOutReservations(pendingOf('starting', true), []),
    ).toEqual([])
  })

  it('never releases a reservation that has not timed out', () => {
    expect(
      settledTimedOutReservations(pendingOf('starting', false), [
        makeConfig(1, true),
      ]),
    ).toEqual([])
  })
})

describe('markTimedOut', () => {
  it('skips a reservation that a newer token has replaced', () => {
    const pending: PendingMap = new Map([
      [1, { action: 'starting', token: 10 }],
      [2, { action: 'starting', token: 11 }],
    ])
    const batchTokens = new Map([
      [1, 5],
      [2, 11],
    ])

    expect(markTimedOut(pending, [1, 2], batchTokens)).toBe(true)
    expect(pending.get(1)).toEqual({ action: 'starting', token: 10 })
    expect(pending.get(2)).toEqual({
      action: 'starting',
      token: 11,
      timedOut: true,
    })
  })

  it('reports nothing marked when every reservation is already timed out', () => {
    const pending: PendingMap = new Map([
      [1, { action: 'starting', token: 5, timedOut: true }],
    ])

    expect(markTimedOut(pending, [1], new Map([[1, 5]]))).toBe(false)
  })
})
