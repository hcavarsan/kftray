import { useCallback, useEffect, useRef, useState } from 'react'

import { useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import {
  configsQuery,
  fetchConfigsWithState,
  useConfigs,
} from '@/hooks/useConfigs'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { errorMessage } from '@/lib/errors'
import type {
  Config,
  PendingConfigAction,
  PortForwardAction,
  PortForwardResponse,
  PortForwardToggleAction,
} from '@/types'

const CONCURRENCY_LIMIT = 8
const BATCH_DEADLINE_MS = 180_000
const DEADLINE_GRACE_MS = 30_000

async function runWithLimit<T>(
  items: T[],
  limit: number,
  worker: (item: T) => Promise<void>,
) {
  let nextIndex = 0

  const runWorker = async () => {
    while (nextIndex < items.length) {
      const item = items[nextIndex]

      nextIndex += 1
      await worker(item)
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, runWorker),
  )
}

function usesTcpForward(config: Config) {
  switch (config.workload_type) {
    case 'expose':
      return true
    case 'proxy':
      return false
    case 'service':
    case 'pod':
      return config.protocol === 'tcp'
  }
}

export function usePortForwarding() {
  const queryClient = useQueryClient()
  const { data: configs = [] } = useConfigs()
  const configsRef = useRef(configs)
  const [isInitiating, setIsInitiating] = useState(false)
  const [isStopping, setIsStopping] = useState(false)
  const startAbortControllerRef = useRef<AbortController | null>(null)
  const stopAbortControllerRef = useRef<AbortController | null>(null)
  const pendingConfigActionsRef = useRef<Map<number, PendingConfigAction>>(
    new Map(),
  )
  const [pendingConfigActions, setPendingConfigActions] = useState<
    Map<number, PendingConfigAction>
  >(new Map())
  const pendingTokenCounterRef = useRef(0)
  const inFlightRef = useRef<Map<number, number>>(new Map())
  const graceTimeoutsRef = useRef<Set<ReturnType<typeof setTimeout>>>(new Set())

  useEffect(() => {
    configsRef.current = configs
  }, [configs])

  useEffect(() => {
    const graceTimeouts = graceTimeoutsRef.current

    return () => {
      for (const timeout of graceTimeouts) {
        clearTimeout(timeout)
      }
      graceTimeouts.clear()
    }
  }, [])

  const publishPending = useCallback(
    () => setPendingConfigActions(new Map(pendingConfigActionsRef.current)),
    [],
  )

  const markPending = useCallback(
    (id: number, action: PortForwardAction) => {
      const token = ++pendingTokenCounterRef.current

      pendingConfigActionsRef.current.set(id, { action, token })
      publishPending()

      return token
    },
    [publishPending],
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

  const refreshConfigs = useCallback(
    () => queryClient.invalidateQueries({ queryKey: configsQuery.queryKey }),
    [queryClient],
  )

  // Applies an authoritative local change and invalidates any refresh that is
  // still in flight, so a stale fetch cannot resurrect what this just removed
  // or replaced.
  const applyConfigs = useCallback(
    async (update: (current: Config[]) => Config[]) => {
      await queryClient.cancelQueries({
        queryKey: configsQuery.queryKey,
        exact: true,
      })
      queryClient.setQueryData(configsQuery.queryKey, current =>
        current ? update(current) : current,
      )
    },
    [queryClient],
  )

  useTauriEvent('config_state_changed', () => {
    void refreshConfigs()
  })

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

  const runForwardCommand = useCallback(
    async (config: Config, action: PortForwardToggleAction, token?: number) => {
      if (token !== undefined) {
        inFlightRef.current.set(config.id, token)
      }
      try {
        const tcp = usesTcpForward(config)

        if (action === 'starting') {
          const responses = await invoke<PortForwardResponse[]>(
            tcp ? 'start_port_forward_tcp_cmd' : 'deploy_and_forward_pod_cmd',
            { configs: [config] },
          )
          const failure = responses.find(response => response.status !== 0)

          if (failure) {
            throw new Error(
              failure.stderr || 'Failed to start port forwarding.',
            )
          }
        } else if (tcp) {
          await invoke('stop_port_forward_cmd', {
            serviceName: config.service,
            configId: config.id.toString(),
          })
        } else {
          await invoke('stop_proxy_forward_cmd', {
            configId: config.id.toString(),
            namespace: config.namespace,
            serviceName: config.service,
            localPort: config.local_port,
            remoteAddress: config.remote_address,
            protocol: 'tcp',
          })
        }
        // A later reservation already owns this id: apply nothing here and
        // let an authoritative refresh recover the state instead of racing
        // it.
        if (
          token === undefined ||
          pendingConfigActionsRef.current.get(config.id)?.token === token
        ) {
          const isRunning = action === 'starting'

          await applyConfigs(current =>
            current.map(item =>
              item.id === config.id ? { ...item, is_running: isRunning } : item,
            ),
          )
        } else {
          void refreshConfigs()
        }
      } finally {
        if (
          token !== undefined &&
          inFlightRef.current.get(config.id) === token
        ) {
          inFlightRef.current.delete(config.id)
        }
      }
    },
    [applyConfigs, refreshConfigs],
  )

  const toggleConfigForward = useCallback(
    async (config: Config, action: PortForwardToggleAction) => {
      if (isBusy(config.id)) {
        toaster.error({
          title: 'Error',
          description: 'This configuration is busy. Try again once it settles.',
          duration: 1000,
        })

        return
      }
      const token = markPending(config.id, action)

      try {
        await runForwardCommand(config, action, token)
      } catch (error) {
        await refreshConfigs()
        toaster.error({
          title:
            action === 'starting'
              ? 'Error starting port forwarding'
              : 'Error stopping port forwarding',
          description: errorMessage(error),
          duration: 1000,
        })
      } finally {
        clearPending(config.id, token)
        void refreshConfigs()
      }
    },
    [isBusy, markPending, clearPending, runForwardCommand, refreshConfigs],
  )

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
        token: ++pendingTokenCounterRef.current,
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
              const pending = pendingConfigActionsRef.current.get(id)

              if (
                pending &&
                pending.token === tokens.get(id) &&
                !pending.timedOut
              ) {
                pendingConfigActionsRef.current.set(id, {
                  ...pending,
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
    [isBusy, publishPending, clearPending, refreshConfigs, runForwardCommand],
  )

  const initiatePortForwarding = useCallback(
    (configsToStart: Config[]) =>
      runPortForwardBatch(configsToStart, 'starting'),
    [runPortForwardBatch],
  )

  const currentConfigs = (selected: Config[]) =>
    selected
      .map(({ id }) => configsRef.current.find(config => config.id === id))
      .filter((config): config is Config => config !== undefined)

  const startSelectedPortForwarding = async (selected: Config[]) => {
    const configsToStart = currentConfigs(selected).filter(
      config => !config.is_running,
    )

    if (configsToStart.length > 0) {
      await runPortForwardBatch(configsToStart, 'starting')
    }
  }

  const stopSelectedPortForwarding = async (selected: Config[]) => {
    const configsToStop = currentConfigs(selected).filter(
      config => config.is_running,
    )

    if (configsToStop.length > 0) {
      await runPortForwardBatch(
        configsToStop,
        'stopping',
        'Selected port forwards stopped successfully.',
      )
    }
  }

  const stopAllPortForwarding = async () => {
    const configsToStop = configsRef.current.filter(config => config.is_running)

    if (configsToStop.length > 0) {
      await runPortForwardBatch(
        configsToStop,
        'stopping',
        'Port forwarding stopped successfully for all configurations.',
      )
    }
  }

  const deleteConfigs = useCallback(
    async (ids: number[]): Promise<boolean> => {
      const busy = ids.filter(isBusy)

      if (busy.length) {
        toaster.error({
          title: 'Error',
          description: `${busy.length} selected configuration(s) are busy. Try again once they settle.`,
          duration: 2000,
        })

        return false
      }

      const tokens = new Map(ids.map(id => [id, markPending(id, 'deleting')]))

      try {
        // Revalidated while reserved: a selected start can settle between the
        // dialog opening and its confirmation, and deleting only removes the
        // database row, leaving the tunnel running with no way to stop it.
        const current = await fetchConfigsWithState()
        const currentIds = new Set(current.map(config => config.id))
        const missing = ids.filter(id => !currentIds.has(id))

        if (missing.length) {
          toaster.error({
            title: 'Error',
            description: `${missing.length} selected configuration(s) could not be verified. Try again.`,
            duration: 2000,
          })
          await refreshConfigs()

          return false
        }
        const running = current.filter(
          config => tokens.has(config.id) && config.is_running,
        )

        if (running.length) {
          toaster.error({
            title: 'Error',
            description: `${running.length} selected configuration(s) are running. Stop them before deleting.`,
            duration: 2000,
          })
          // Through the query refresh: another operation can have
          // settled while this snapshot was being fetched, and writing it
          // directly would resurrect what that operation just changed.
          await refreshConfigs()

          return false
        }
        await invoke('delete_configs_cmd', { ids })
        await applyConfigs(configs =>
          configs.filter(config => !tokens.has(config.id)),
        )
        toaster.success({
          title: 'Success',
          description: 'Configurations deleted successfully.',
          duration: 1000,
        })

        return true
      } catch {
        toaster.error({
          title: 'Error',
          description: 'Failed to delete configurations.',
          duration: 1000,
        })

        return false
      } finally {
        for (const [id, token] of tokens) {
          clearPending(id, token)
        }
      }
    },
    [isBusy, markPending, clearPending, applyConfigs, refreshConfigs],
  )

  const saveConfig = async (
    configToSave: Config,
    isEdit: boolean,
  ): Promise<boolean> => {
    const runningConfig = isEdit
      ? configsRef.current.find(conf => conf.id === configToSave.id)
      : undefined
    const wasRunning = Boolean(runningConfig?.is_running)
    const verb = isEdit ? 'update' : 'add'

    if (isEdit && isBusy(configToSave.id)) {
      toaster.error({
        title: 'Error',
        description: 'This configuration is busy. Try again once it settles.',
        duration: 1000,
      })

      return false
    }

    // Reserved for the whole transaction, not only when a restart is needed:
    // `update_config_cmd` does not share the backend lifecycle lock, so a start
    // accepted while it is in flight would use the pre-edit snapshot.
    let pendingToken = isEdit
      ? markPending(configToSave.id, wasRunning ? 'stopping' : 'saving')
      : undefined
    let configSaved = false
    let wasStopped = false

    try {
      const updatedConfigToSave: Config = {
        ...configToSave,
        id: isEdit ? configToSave.id : 0,
      }

      // Stopped by its pre-edit identity: `configToSave` carries the edited
      // service/namespace/port, and stopping with those would target the
      // wrong running resource.
      if (wasRunning && runningConfig) {
        await runForwardCommand(runningConfig, 'stopping', pendingToken)
        wasStopped = true
      }

      if (isEdit) {
        await invoke('update_config_cmd', { config: updatedConfigToSave })
        // Merged before the reservation is released: the refresh below may
        // fail, and a start in that window would otherwise send the pre-edit
        // snapshot to the backend.
        await applyConfigs(current =>
          current.map(config =>
            config.id === updatedConfigToSave.id
              ? { ...updatedConfigToSave, is_running: config.is_running }
              : config,
          ),
        )
      } else {
        await invoke('insert_config_cmd', { config: updatedConfigToSave })
      }
      configSaved = true
      if (wasRunning) {
        pendingToken = markPending(configToSave.id, 'starting')
        await runForwardCommand(updatedConfigToSave, 'starting', pendingToken)
      }

      toaster.success({
        title: 'Success',
        description: `Configuration ${isEdit ? 'updated' : 'added'} successfully.`,
        duration: 1000,
      })

      return true
    } catch (error) {
      const message = errorMessage(error)

      if (configSaved) {
        toaster.warning({
          title: 'Warning',
          description: `Configuration updated, but restarting the port forward failed: ${message}`,
          duration: 2000,
        })

        return true
      }
      if (!wasStopped) {
        toaster.error({
          title: 'Error',
          description: `Failed to ${verb} configuration.`,
          duration: 1000,
        })

        return false
      }
      if (!runningConfig) {
        toaster.error({
          title: 'Error',
          description: `The forward was stopped but the save failed. ${message}`,
          duration: 2000,
        })

        return false
      }
      try {
        const pending = pendingConfigActionsRef.current.get(configToSave.id)

        if (
          pendingToken !== undefined &&
          pending?.token === pendingToken &&
          pending.action !== 'starting'
        ) {
          pendingConfigActionsRef.current.set(configToSave.id, {
            ...pending,
            action: 'starting',
          })
          publishPending()
        }
        await runForwardCommand(runningConfig, 'starting', pendingToken)
        toaster.error({
          title: 'Error',
          description: `Failed to ${verb} configuration. The forward was restarted. ${message}`,
          duration: 2000,
        })
      } catch (restartError) {
        toaster.error({
          title: 'Error',
          description: `The forward was stopped, the save failed, and restarting it also failed: ${errorMessage(restartError)}`,
          duration: 3000,
        })
      }

      return false
    } finally {
      if (pendingToken !== undefined) {
        clearPending(configToSave.id, pendingToken)
      }
      void refreshConfigs()
    }
  }

  return {
    configs,
    pendingConfigActions,
    isInitiating,
    isStopping,
    toggleConfigForward,
    initiatePortForwarding,
    startSelectedPortForwarding,
    stopSelectedPortForwarding,
    stopAllPortForwarding,
    abortStartOperation,
    abortStopOperation,
    deleteConfigs,
    saveConfig,
  }
}
