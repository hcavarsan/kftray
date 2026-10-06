import { describe, expect, it } from 'vitest'

import { makeConfig } from '@/components/Main/testFixtures'

import { forwardBatchState } from './forwardBatchState'

describe('forwardBatchState', () => {
  it('falls back to the all actions when the selection is hidden by a filter', () => {
    const hidden = makeConfig(1)
    const visible = [makeConfig(2), makeConfig(3, true)]

    const state = forwardBatchState(visible, [hidden], [hidden, ...visible])

    expect(state.selected).toEqual([])
    expect(state.startSelected).toBe(false)
    expect(state.stopSelected).toBe(false)
    expect(state.startDisabled).toBe(false)
    expect(state.stopDisabled).toBe(false)
  })

  it('targets only the visible part of a selection, with its current state', () => {
    const visible = [makeConfig(2, true), makeConfig(3)]
    const selected = [makeConfig(1), makeConfig(2, false)]

    const state = forwardBatchState(visible, selected, [
      selected[0],
      ...visible,
    ])

    expect(state.selected).toEqual([visible[0]])
    expect(state.startSelected).toBe(false)
    expect(state.stopSelected).toBe(true)
    expect(state.startDisabled).toBe(true)
    expect(state.stopDisabled).toBe(false)
  })

  it('uses the selected actions when a visible selection can start and stop', () => {
    const visible = [makeConfig(1), makeConfig(2, true), makeConfig(3)]

    const state = forwardBatchState(visible, [visible[0], visible[1]], visible)

    expect(state.selected).toEqual([visible[0], visible[1]])
    expect(state.startSelected).toBe(true)
    expect(state.stopSelected).toBe(true)
    expect(state.startDisabled).toBe(false)
    expect(state.stopDisabled).toBe(false)
  })

  it('keeps Stop All enabled for a running config hidden by a filter', () => {
    const hidden = makeConfig(1, true)
    const visible = [makeConfig(2)]

    const state = forwardBatchState(visible, [hidden], [hidden, ...visible])

    expect(state.stopSelected).toBe(false)
    expect(state.stopDisabled).toBe(false)
    expect(state.startDisabled).toBe(false)
  })

  it('disables the all actions when nothing can change', () => {
    const runningConfigs = [makeConfig(1, true)]
    const stoppedConfigs = [makeConfig(1)]
    const running = forwardBatchState(runningConfigs, [], runningConfigs)
    const stopped = forwardBatchState(stoppedConfigs, [], stoppedConfigs)

    expect(running.startDisabled).toBe(true)
    expect(running.stopDisabled).toBe(false)
    expect(stopped.startDisabled).toBe(false)
    expect(stopped.stopDisabled).toBe(true)
  })
})
