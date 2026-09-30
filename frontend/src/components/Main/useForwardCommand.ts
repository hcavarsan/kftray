import { useCallback } from 'react'

import type { Config, PortForwardToggleAction, StoredConfig } from '@/types'

import { startForward, stopForward } from './portForwardCommands'
import type { PendingActions } from './usePendingActions'

export type RunForwardCommand = (
  config: StoredConfig,
  action: PortForwardToggleAction,
  token?: number,
) => Promise<void>

interface UseForwardCommandOptions {
  pendingConfigActionsRef: PendingActions['pendingConfigActionsRef']
  inFlightRef: PendingActions['inFlightRef']
  applyConfigs: (update: (current: Config[]) => Config[]) => Promise<void>
  refreshConfigs: () => Promise<void>
}

export function useForwardCommand({
  pendingConfigActionsRef,
  inFlightRef,
  applyConfigs,
  refreshConfigs,
}: UseForwardCommandOptions): RunForwardCommand {
  return useCallback(
    async (
      config: StoredConfig,
      action: PortForwardToggleAction,
      token?: number,
    ) => {
      if (token !== undefined) {
        inFlightRef.current.set(config.id, token)
      }
      try {
        if (action === 'starting') {
          await startForward(config)
        } else {
          await stopForward(config)
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
    [pendingConfigActionsRef, inFlightRef, applyConfigs, refreshConfigs],
  )
}
