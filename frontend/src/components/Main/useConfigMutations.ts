import { type RefObject, useCallback } from 'react'

import type { Config, StoredConfig } from '@/types'

import {
  deleteConfigsTransaction,
  saveConfigTransaction,
} from './configTransactions'
import type { RunForwardCommand } from './useForwardCommand'
import type { PendingActions } from './usePendingActions'

interface UseConfigMutationsOptions {
  configsRef: RefObject<Config[]>
  pending: PendingActions
  runForwardCommand: RunForwardCommand
  applyConfigs: (update: (current: Config[]) => Config[]) => Promise<void>
  refreshConfigs: () => Promise<void>
}

interface ConfigMutations {
  deleteConfigs: (ids: number[]) => Promise<boolean>
  saveConfig: (configToSave: StoredConfig, isEdit: boolean) => Promise<boolean>
}

export function useConfigMutations({
  configsRef,
  pending,
  runForwardCommand,
  applyConfigs,
  refreshConfigs,
}: UseConfigMutationsOptions): ConfigMutations {
  const {
    pendingConfigActionsRef,
    publishPending,
    markPending,
    clearPending,
    isBusy,
  } = pending

  const deleteConfigs = useCallback(
    (ids: number[]) =>
      deleteConfigsTransaction(
        { isBusy, markPending, clearPending, applyConfigs, refreshConfigs },
        ids,
      ),
    [isBusy, markPending, clearPending, applyConfigs, refreshConfigs],
  )

  const saveConfig = (configToSave: StoredConfig, isEdit: boolean) =>
    saveConfigTransaction(
      {
        configsRef,
        pendingConfigActionsRef,
        publishPending,
        isBusy,
        markPending,
        clearPending,
        runForwardCommand,
        applyConfigs,
        refreshConfigs,
      },
      configToSave,
      isEdit,
    )

  return { deleteConfigs, saveConfig }
}
