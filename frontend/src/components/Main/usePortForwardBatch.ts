import { useCallback, useEffect, useRef, useState } from 'react'

import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'
import type { Config, PortForwardToggleAction } from '@/types'

import { runWithLimit } from './runWithLimit'
import type { RunForwardCommand } from './useForwardCommand'
import type { PendingActions } from './usePendingActions'

const CONCURRENCY_LIMIT = 8
const BATCH_DEADLINE_MS = 180_000
const DEADLINE_GRACE_MS = 30_000

interface UsePortForwardBatchOptions {
  pending: PendingActions
  runForwardCommand: RunForwardCommand
  refreshConfigs: () => Promise<void>
}

interface PortForwardBatch {
  isInitiating: boolean
  isStopping: boolean
  abortStartOperation: () => void
  abortStopOperation: () => void
  runPortForwardBatch: (
    candidates: Config[],
    action: PortForwardToggleAction,
    successMessage?: string,
  ) => Promise<void>
}

export function usePortForwardBatch({
  pending,
  runForwardCommand,
  refreshConfigs,
}: UsePortForwardBatchOptions): PortForwardBatch {
  const {
    pendingConfigActionsRef,
    nextToken,
    publishPending,
    clearPending,
    isBusy,
  } = pending
  const [isInitiating, setIsInitiating] = useState(false)
  const [isStopping, setIsStopping] = useState(false)
  const startAbortControllerRef = useRef<AbortController | null>(null)
  const stopAbortControllerRef = useRef<AbortController | null>(null)
  const graceTimeoutsRef = useRef<Set<ReturnType<typeof setTimeout>>>(new Set())

  useEffect(() => {
    const graceTimeouts = graceTimeoutsRef.current

    return () => {
      for (const timeout of graceTimeouts) {
        clearTimeout(timeout)
      }
      graceTimeouts.clear()
    }
  }, [])

  const abortOperation = useCallback(
    (action: PortForwardToggleAction) => {
      const controllerRef =
        action === 'starting' ? startAbortControllerRef : stopAbortControllerRef
      const noun = action === 'starting' ? 'starts' : 'stops'

      controllerRef.current?.abort()
      controllerRef.current = null
      if (action === 'starting') {
        setIsInitiating(false)
      } else {
        setIsStopping(false)
      }
      toaster.info({
        title: 'Aborted',
        description: `Queued ${noun} cancelled. Active ${noun} will finish.`,
        duration: 2000,
      })
      void refreshConfigs()
    },
    [refreshConfigs],
  )

  const abortStartOperation = useCallback(
    () => abortOperation('starting'),
    [abortOperation],
  )

  const abortStopOperation = useCallback(
    () => abortOperation('stopping'),
    [abortOperation],
  )

  const runPortForwardBatch = useCallback(
    async (
      candidates: Config[],
      action: PortForwardToggleAction,
      successMessage?: string,
    ) => {
      const controllerRef =
        action === 'starting' ? startAbortControllerRef : stopAbortControllerRef
      const verb = action === 'starting' ? 'start' : 'stop'
      const failureTitle =
        action === 'starting' ? 'Start Failed' : 'Stop Failed'

      if (controllerRef.current) {
        toaster.info({
          title: 'Busy',
          description: `A ${verb} batch is already running. Try again once it finishes.`,
          duration: 2000,
        })

        return
      }
      const targets = candidates.filter(config => !isBusy(config.id))

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
      const setBusy = action === 'starting' ? setIsInitiating : setIsStopping
      const jobs = targets.map(config => ({
        config,
        token: nextToken(),
      }))
      const tokens = new Map(
        jobs.map(({ config, token }) => [config.id, token]),
      )
      const queued = new Set(tokens.keys())
      const unresolved = new Set(tokens.keys())

      for (const { config, token } of jobs) {
        pendingConfigActionsRef.current.set(config.id, { action, token })
      }
      publishPending()
      setBusy(true)

      const cancelQueued = () => {
        for (const id of queued) {
          // Only the reservation this batch created: a worker that already
          // settled and started a fresh operation on the same id must keep
          // that newer reservation intact.
          if (
            pendingConfigActionsRef.current.get(id)?.token === tokens.get(id)
          ) {
            pendingConfigActionsRef.current.delete(id)
          }
          // Released here, so the deadline message counts only the invocations
          // that are genuinely still running.
          unresolved.delete(id)
        }
        queued.clear()
        publishPending()
      }

      controller.signal.addEventListener('abort', cancelQueued, { once: true })
      // The batch is bounded so one hung invoke cannot hold the controller guard
      // forever and reject every later batch of the same action.
      let timedOut = false

      // Collected as workers settle, so a failure that happened before the
      // deadline is still reported when the deadline wins the race.
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
            if (pendingConfigActionsRef.current.get(id)?.token !== token) {
              // Superseded by a newer reservation for this id: this worker has
              // nothing left to hold.
              unresolved.delete(id)

              return
            }
            if (controller.signal.aborted) {
              // Dequeued before cancelQueued could release it: this worker
              // still owns the reservation, so release it here.
              unresolved.delete(id)
              clearPending(id, token)

              return
            }
            try {
              await runForwardCommand(config, action, token)
            } catch (error) {
              const failure = { id, error }

              failures.push(failure)
              // After the deadline nobody is aggregating any more, so each failure
              // reports itself instead of waiting for unrelated invocations.
              if (timedOut) {
                reportFailures([failure])
              }
            } finally {
              unresolved.delete(id)
              clearPending(id, token)
            }
          },
        )
        // The handle is kept so the loser of the race can be cancelled: an
        // uncleared timeout keeps the timer, and everything it closes over,
        // alive for the full deadline after a batch that finished immediately.
        let deadline: ReturnType<typeof setTimeout> | undefined
        const settled = await Promise.race([
          batch.then(() => true),
          new Promise<false>(resolve => {
            deadline = setTimeout(() => resolve(false), BATCH_DEADLINE_MS)
          }),
        ]).finally(() => clearTimeout(deadline))

        if (!settled) {
          timedOut = true
          // Aborted while `cancelQueued` is still registered: configurations
          // that never started release their reservation immediately.
          controller.abort()
          // The remaining ids are genuinely in flight: their reservation is
          // kept (rows stay busy) instead of being cleared out from under a
          // running invoke(); each worker's own finally releases its token
          // once it actually settles. Releasing the batch controller below
          // is what lets a new batch start in the meantime. A worker that
          // never settles (a truly hung invoke) would otherwise keep its row
          // looking like it is still actively starting/stopping forever, so a
          // grace timeout instead marks whatever is still unresolved as
          // timed out; the row stays busy/disabled and only flips once the
          // invoke actually settles.
          const stillUnresolved = unresolved.size

          reportFailures()
          toaster.error({
            title: failureTitle,
            description: `${stillUnresolved} configuration(s) did not finish within the timeout. Their status will refresh shortly.`,
            duration: 3000,
          })

          const graceTimeout = setTimeout(() => {
            graceTimeoutsRef.current.delete(graceTimeout)

            let markedTimedOut = false

            for (const id of unresolved) {
              const pendingAction = pendingConfigActionsRef.current.get(id)

              if (
                pendingAction &&
                pendingAction.token === tokens.get(id) &&
                !pendingAction.timedOut
              ) {
                pendingConfigActionsRef.current.set(id, {
                  ...pendingAction,
                  timedOut: true,
                })
                markedTimedOut = true
              }
            }
            if (markedTimedOut) {
              publishPending()
            }
          }, DEADLINE_GRACE_MS)

          graceTimeoutsRef.current.add(graceTimeout)

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
    },
    [
      pendingConfigActionsRef,
      nextToken,
      isBusy,
      publishPending,
      clearPending,
      refreshConfigs,
      runForwardCommand,
    ],
  )

  return {
    isInitiating,
    isStopping,
    abortStartOperation,
    abortStopOperation,
    runPortForwardBatch,
  }
}
