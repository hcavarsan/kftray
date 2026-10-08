import { useCallback, useEffect, useRef } from 'react'

import { toaster } from '@/components/ui/toaster'
import { useConfigs } from '@/hooks/useConfigs'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { errorMessage } from '@/lib/errors'
import type { Config, PortForwardToggleAction } from '@/types'

import { PRIVILEGE_ERROR } from './privilegeGate'
import { useConfigCache } from './useConfigCache'
import { useConfigMutations } from './useConfigMutations'
import { useForwardCommand } from './useForwardCommand'
import { usePendingActions } from './usePendingActions'
import { usePortForwardBatch } from './usePortForwardBatch'
import { usePrivilegeGate } from './usePrivilegeGate'

export function usePortForwarding() {
  const { data: configs = [] } = useConfigs()
  const configsRef = useRef(configs)
  const pending = usePendingActions(configs)
  const { pendingConfigActions, isBusy, markPending, clearPending } = pending
  const { refreshConfigs, applyConfigs } = useConfigCache()
  const privileges = usePrivilegeGate()
  const { ensurePrivileges, askAfterRefusal } = privileges
  const runForwardCommand = useForwardCommand({
    pendingConfigActionsRef: pending.pendingConfigActionsRef,
    inFlightRef: pending.inFlightRef,
    applyConfigs,
    refreshConfigs,
  })
  const {
    isInitiating,
    isStopping,
    abortStartOperation,
    abortStopOperation,
    runPortForwardBatch,
  } = usePortForwardBatch({
    pending,
    runForwardCommand,
    refreshConfigs,
    // The retry batch reports a second refusal as a plain failure instead
    // of asking again.
    onPrivilegeRefused: (failed, message) =>
      void askAfterRefusal(failed, message).then(decision => {
        if (decision !== 'cancel') {
          void runPortForwardBatch(failed, 'starting', undefined, true)
        }
      }),
  })
  const { deleteConfigs, saveConfig } = useConfigMutations({
    configsRef,
    pending,
    runForwardCommand,
    applyConfigs,
    refreshConfigs,
  })

  useEffect(() => {
    configsRef.current = configs
  }, [configs])

  useTauriEvent('config_state_changed', () => {
    void refreshConfigs()
  })

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
      if (action === 'starting' && !(await ensurePrivileges([config]))) {
        return
      }
      // A start refused for lack of privileges is offered once more after
      // the user has had the chance to install the helper or accept the
      // prompts. The row is released between the two attempts so the retry
      // can reserve it again. A second refusal is reported as a plain
      // failure.
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const token = markPending(config.id, action)
        let refused: string | null = null

        try {
          await runForwardCommand(config, action, token)
        } catch (error) {
          await refreshConfigs()
          const message = errorMessage(error)

          if (
            action === 'starting' &&
            attempt === 0 &&
            PRIVILEGE_ERROR.test(message)
          ) {
            refused = message
          } else {
            toaster.error({
              title:
                action === 'starting'
                  ? 'Error starting port forwarding'
                  : 'Error stopping port forwarding',
              description: message,
              duration: 1000,
            })
          }
        } finally {
          clearPending(config.id, token)
          void refreshConfigs()
        }
        if (
          refused === null ||
          (await askAfterRefusal([config], refused)) === 'cancel'
        ) {
          return
        }
      }
    },
    [
      isBusy,
      ensurePrivileges,
      askAfterRefusal,
      markPending,
      clearPending,
      runForwardCommand,
      refreshConfigs,
    ],
  )

  const initiatePortForwarding = useCallback(
    async (configsToStart: Config[]) => {
      if (await ensurePrivileges(configsToStart)) {
        await runPortForwardBatch(configsToStart, 'starting')
      }
    },
    [ensurePrivileges, runPortForwardBatch],
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
      await initiatePortForwarding(configsToStart)
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
    privileges,
  }
}
