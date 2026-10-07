import { useCallback, useEffect, useRef, useState } from 'react'

import { toaster } from '@/components/ui/toaster'
import type { Config, PortForwardToggleAction } from '@/types'

import { executeBatch } from './portForwardBatch'
import type { RunForwardCommand } from './useForwardCommand'
import type { PendingActions } from './usePendingActions'

interface UsePortForwardBatchOptions {
  pending: PendingActions
  runForwardCommand: RunForwardCommand
  refreshConfigs: () => Promise<void>
  onPrivilegeRefused: (configs: Config[], message: string) => void
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
    privilegeRetry?: boolean,
  ) => Promise<void>
}

export function usePortForwardBatch({
  pending,
  runForwardCommand,
  refreshConfigs,
  onPrivilegeRefused,
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
    (
      candidates: Config[],
      action: PortForwardToggleAction,
      successMessage?: string,
      privilegeRetry = false,
    ) =>
      executeBatch(
        {
          pending: {
            pendingConfigActionsRef,
            nextToken,
            publishPending,
            clearPending,
            isBusy,
          },
          runForwardCommand,
          refreshConfigs,
          controllerRef:
            action === 'starting'
              ? startAbortControllerRef
              : stopAbortControllerRef,
          graceTimeouts: graceTimeoutsRef.current,
          setBusy: action === 'starting' ? setIsInitiating : setIsStopping,
          onPrivilegeRefused,
        },
        candidates,
        action,
        successMessage,
        privilegeRetry,
      ),
    [
      pendingConfigActionsRef,
      nextToken,
      isBusy,
      publishPending,
      clearPending,
      refreshConfigs,
      runForwardCommand,
      onPrivilegeRefused,
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
