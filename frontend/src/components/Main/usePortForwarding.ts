import { useCallback, useEffect, useRef } from 'react'

import { useConfigs } from '@/hooks/useConfigs'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import type { Config, PortForwardToggleAction } from '@/types'

import { executeToggle } from './portForwardToggle'
import { useConfigCache } from './useConfigCache'
import { useConfigMutations } from './useConfigMutations'
import { useForwardCommand } from './useForwardCommand'
import { usePendingActions } from './usePendingActions'
import { usePortForwardBatch } from './usePortForwardBatch'
import { usePrivilegeGate } from './usePrivilegeGate'

const NO_CONFIGS: Config[] = []

export function usePortForwarding() {
  const { data: configs = NO_CONFIGS } = useConfigs()
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
    (config: Config, action: PortForwardToggleAction) =>
      executeToggle(
        {
          pending: { isBusy, markPending, clearPending },
          ensurePrivileges,
          askAfterRefusal,
          runForwardCommand,
          refreshConfigs,
        },
        config,
        action,
      ),
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
      config => config.is_running && !config.owned_elsewhere,
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
    const configsToStop = configsRef.current.filter(
      config => config.is_running && !config.owned_elsewhere,
    )

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
