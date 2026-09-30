import type { RefObject } from 'react'

import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'
import type { Config, PortForwardToggleAction } from '@/types'

import {
  markTimedOut,
  ownsReservation,
  releaseReservation,
} from './reservationRegistry'
import { runWithLimit } from './runWithLimit'
import type { RunForwardCommand } from './useForwardCommand'
import type { PendingActions } from './usePendingActions'

const CONCURRENCY_LIMIT = 8
export const BATCH_DEADLINE_MS = 180_000
export const DEADLINE_GRACE_MS = 30_000

export interface BatchDeps {
  pending: Pick<
    PendingActions,
    | 'pendingConfigActionsRef'
    | 'nextToken'
    | 'publishPending'
    | 'clearPending'
    | 'isBusy'
  >
  runForwardCommand: RunForwardCommand
  refreshConfigs: () => Promise<void>
  controllerRef: RefObject<AbortController | null>
  graceTimeouts: Set<ReturnType<typeof setTimeout>>
  setBusy: (busy: boolean) => void
}

export async function executeBatch(
  {
    pending,
    runForwardCommand,
    refreshConfigs,
    controllerRef,
    graceTimeouts,
    setBusy,
  }: BatchDeps,
  candidates: Config[],
  action: PortForwardToggleAction,
  successMessage?: string,
) {
  const { pendingConfigActionsRef, nextToken, publishPending, clearPending } =
    pending
  const verb = action === 'starting' ? 'start' : 'stop'
  const failureTitle = action === 'starting' ? 'Start Failed' : 'Stop Failed'

  if (controllerRef.current) {
    toaster.info({
      title: 'Busy',
      description: `A ${verb} batch is already running. Try again once it finishes.`,
      duration: 2000,
    })

    return
  }
  const targets = candidates.filter(config => !pending.isBusy(config.id))

  if (targets.length === 0) {
    toaster.info({
      title: 'Nothing to do',
      description: 'The selected configuration(s) are already busy.',
      duration: 2000,
    })

    return
  }
  const controller = new AbortController()

  controllerRef.current = controller
  const jobs = targets.map(config => ({
    config,
    token: nextToken(),
  }))
  const tokens = new Map(jobs.map(({ config, token }) => [config.id, token]))
  const queued = new Map(tokens)
  const unresolved = new Set(tokens.keys())

  for (const { config, token } of jobs) {
    pendingConfigActionsRef.current.set(config.id, { action, token })
  }
  publishPending()
  setBusy(true)

  const cancelQueued = () => {
    for (const [id, token] of queued) {
      releaseReservation(pendingConfigActionsRef.current, id, token)
      unresolved.delete(id)
    }
    queued.clear()
    publishPending()
  }

  controller.signal.addEventListener('abort', cancelQueued, { once: true })
  let timedOut = false

  const failures: { id: number; error: unknown }[] = []
  const reportFailures = (reporting = failures) => {
    if (!reporting.length) {
      return
    }
    const first = reporting[0]

    toaster.error({
      title: failureTitle,
      description:
        reporting.length === 1
          ? `Config ${first.id}: ${errorMessage(first.error)}`
          : `${reporting.length} configs failed to ${verb}`,
      duration: 3000,
    })
  }

  try {
    const batch = runWithLimit(
      jobs,
      CONCURRENCY_LIMIT,
      async ({ config, token }) => {
        const { id } = config

        queued.delete(id)
        if (!ownsReservation(pendingConfigActionsRef.current, id, token)) {
          unresolved.delete(id)

          return
        }
        if (controller.signal.aborted) {
          unresolved.delete(id)
          clearPending(id, token)

          return
        }
        try {
          await runForwardCommand(config, action, token)
        } catch (error) {
          const failure = { id, error }

          failures.push(failure)
          if (timedOut) {
            reportFailures([failure])
          }
        } finally {
          unresolved.delete(id)
          clearPending(id, token)
        }
      },
    )
    let deadline: ReturnType<typeof setTimeout> | undefined
    const settled = await Promise.race([
      batch.then(() => true),
      new Promise<false>(resolve => {
        deadline = setTimeout(() => resolve(false), BATCH_DEADLINE_MS)
      }),
    ]).finally(() => clearTimeout(deadline))

    if (!settled) {
      timedOut = true
      controller.abort()
      const stillUnresolved = unresolved.size

      reportFailures()
      toaster.error({
        title: failureTitle,
        description: `${stillUnresolved} configuration(s) did not finish within the timeout. Their status will refresh shortly.`,
        duration: 3000,
      })

      const graceTimeout = setTimeout(() => {
        graceTimeouts.delete(graceTimeout)
        if (markTimedOut(pendingConfigActionsRef.current, unresolved, tokens)) {
          publishPending()
        }
      }, DEADLINE_GRACE_MS)

      graceTimeouts.add(graceTimeout)

      return
    }
    if (failures.length) {
      reportFailures()
    } else if (successMessage && !controller.signal.aborted) {
      toaster.success({
        title: 'Success',
        description: successMessage,
        duration: 1000,
      })
    }
  } catch (error) {
    toaster.error({
      title: failureTitle,
      description: errorMessage(error),
      duration: 3000,
    })
  } finally {
    controller.signal.removeEventListener('abort', cancelQueued)
    if (controllerRef.current === controller) {
      controllerRef.current = null
      setBusy(false)
    }
    await refreshConfigs()
  }
}
