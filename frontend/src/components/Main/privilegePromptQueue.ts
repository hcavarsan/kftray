import type { PrivilegeNeed } from '@/components/PrivilegeDialog/types'

import type { PrivilegeDecision } from './privilegeGate'

export interface PrivilegePrompt {
  needs: PrivilegeNeed[]
  /** Set when a start already failed for lack of privileges. */
  error: string | null
  resolve: (decision: PrivilegeDecision) => void
}

export interface PrivilegePromptQueue {
  ask: (
    needs: PrivilegeNeed[],
    error: string | null,
  ) => Promise<PrivilegeDecision>
  resolve: (decision: PrivilegeDecision) => void
  /**
   * Answers every waiting prompt with `continue`: the user has just asked
   * not to be prompted, so the starts queued behind that dialog proceed
   * without one.
   */
  drain: () => void
}

/**
 * One prompt at a time. A start reaching the gate while a dialog is open
 * waits for that dialog to close, then gets its own; replacing the visible
 * prompt would drop the first start's resolver and leave it waiting
 * forever. `onShow` is told which prompt is on screen, or `null`.
 */
export function createPrivilegePromptQueue(
  onShow: (prompt: PrivilegePrompt | null) => void,
): PrivilegePromptQueue {
  let active: PrivilegePrompt | null = null
  const waiting: PrivilegePrompt[] = []
  const show = (next: PrivilegePrompt | null) => {
    active = next
    onShow(next)
  }

  return {
    ask: (needs, error) => {
      const { promise, resolve } = Promise.withResolvers<PrivilegeDecision>()
      const next = { needs, error, resolve }

      if (active) {
        waiting.push(next)
      } else {
        show(next)
      }

      return promise
    },
    resolve: decision => {
      active?.resolve(decision)
      show(waiting.shift() ?? null)
    },
    drain: () => {
      for (const queued of waiting.splice(0)) {
        queued.resolve('continue')
      }
    },
  }
}
