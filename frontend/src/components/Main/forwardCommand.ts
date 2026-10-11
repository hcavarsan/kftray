import type { RefObject } from 'react'

import type { Config, PortForwardToggleAction, StoredConfig } from '@/types'

import { startForward, stopForward } from './portForwardCommands'
import { ownsReservation, type PendingMap } from './reservationRegistry'

export interface ForwardCommandDeps {
  pendingConfigActionsRef: RefObject<PendingMap>
  inFlightRef: RefObject<Map<number, number>>
  applyConfigs: (update: (current: Config[]) => Config[]) => Promise<void>
  refreshConfigs: () => Promise<void>
}

export async function executeForwardCommand(
  {
    pendingConfigActionsRef,
    inFlightRef,
    applyConfigs,
    refreshConfigs,
  }: ForwardCommandDeps,
  config: StoredConfig,
  action: PortForwardToggleAction,
  token?: number,
) {
  if (token !== undefined) {
    inFlightRef.current.set(config.id, token)
  }
  try {
    if (action === 'starting') {
      await startForward(config)
    } else {
      await stopForward(config)
    }
    if (
      token === undefined ||
      ownsReservation(pendingConfigActionsRef.current, config.id, token)
    ) {
      const isRunning = action === 'starting'

      // A start or stop writes a fresh state row, which drops the saved error
      // and any reconnect attempt.
      await applyConfigs(current =>
        current.map(item =>
          item.id === config.id
            ? {
                ...item,
                is_running: isRunning,
                is_retrying: false,
                retry_count: null,
                last_error: null,
              }
            : item,
        ),
      )
    } else {
      void refreshConfigs()
    }
  } finally {
    if (token !== undefined && inFlightRef.current.get(config.id) === token) {
      inFlightRef.current.delete(config.id)
    }
  }
}
