import { useCallback } from 'react'

import type { PortForwardToggleAction, StoredConfig } from '@/types'

import {
  executeForwardCommand,
  type ForwardCommandDeps,
} from './forwardCommand'

export type RunForwardCommand = (
  config: StoredConfig,
  action: PortForwardToggleAction,
  token?: number,
) => Promise<void>

export function useForwardCommand({
  pendingConfigActionsRef,
  inFlightRef,
  applyConfigs,
  refreshConfigs,
}: ForwardCommandDeps): RunForwardCommand {
  return useCallback(
    (config: StoredConfig, action: PortForwardToggleAction, token?: number) =>
      executeForwardCommand(
        { pendingConfigActionsRef, inFlightRef, applyConfigs, refreshConfigs },
        config,
        action,
        token,
      ),
    [pendingConfigActionsRef, inFlightRef, applyConfigs, refreshConfigs],
  )
}
