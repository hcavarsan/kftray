import { useCallback, useEffect, useRef } from 'react'

import { toaster } from '@/components/ui/toaster'
import { useConfigs } from '@/hooks/useConfigs'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { errorMessage } from '@/lib/errors'
import type { Config, PortForwardToggleAction } from '@/types'

import { useConfigCache } from './useConfigCache'
import { useConfigMutations } from './useConfigMutations'
import { useForwardCommand } from './useForwardCommand'
import { usePendingActions } from './usePendingActions'
import { usePortForwardBatch } from './usePortForwardBatch'

export function usePortForwarding() {
  const { data: configs = [] } = useConfigs()
  const configsRef = useRef(configs)
  const pending = usePendingActions(configs)
  const { pendingConfigActions, isBusy, markPending, clearPending } = pending
  const { refreshConfigs, applyConfigs } = useConfigCache()
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
  } = usePortForwardBatch({ pending, runForwardCommand, refreshConfigs })
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
