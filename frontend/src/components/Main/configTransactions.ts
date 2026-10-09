import type { RefObject } from 'react'

import { toaster } from '@/components/ui/toaster'
import { fetchConfigsWithState } from '@/hooks/useConfigs'
import { errorMessage } from '@/lib/errors'
import { invoke } from '@/lib/tauri'
import type { Config, StoredConfig } from '@/types'

import type { RunForwardCommand } from './useForwardCommand'
import type { PendingActions } from './usePendingActions'

export interface DeleteDeps
  extends Pick<PendingActions, 'isBusy' | 'markPending' | 'clearPending'> {
  applyConfigs: (update: (current: Config[]) => Config[]) => Promise<void>
  refreshConfigs: () => Promise<void>
}

export interface SaveDeps
  extends DeleteDeps,
    Pick<PendingActions, 'pendingConfigActionsRef' | 'publishPending'> {
  configsRef: RefObject<Config[]>
  runForwardCommand: RunForwardCommand
}

export async function deleteConfigsTransaction(
  {
    isBusy,
    markPending,
    clearPending,
    applyConfigs,
    refreshConfigs,
  }: DeleteDeps,
  ids: number[],
): Promise<boolean> {
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
}

export async function saveConfigTransaction(
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
  }: SaveDeps,
  configToSave: StoredConfig,
  isEdit: boolean,
): Promise<boolean> {
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

  let pendingToken = isEdit
    ? markPending(configToSave.id, wasRunning ? 'stopping' : 'saving')
    : undefined
  let configSaved = false
  let wasStopped = false

  try {
    const updatedConfigToSave: StoredConfig = {
      ...configToSave,
      id: isEdit ? configToSave.id : 0,
    }
    let configToStart = updatedConfigToSave

    if (wasRunning && runningConfig) {
      await runForwardCommand(runningConfig, 'stopping', pendingToken)
      wasStopped = true
    }

    if (isEdit) {
      await invoke('update_config_cmd', { config: updatedConfigToSave })
      configSaved = true
      const savedConfig = await invoke<StoredConfig>('get_config_cmd', {
        id: updatedConfigToSave.id,
      })

      await applyConfigs(current =>
        current.map(config =>
          config.id === savedConfig.id
            ? { ...savedConfig, is_running: config.is_running }
            : config,
        ),
      )
      configToStart = savedConfig
    } else {
      await invoke('insert_config_cmd', { config: updatedConfigToSave })
      configSaved = true
    }
    if (wasRunning) {
      pendingToken = markPending(configToSave.id, 'starting')
      await runForwardCommand(configToStart, 'starting', pendingToken)
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
        description: wasRunning
          ? `Configuration updated, but restarting the port forward failed: ${message}`
          : `Configuration updated, but reloading it failed: ${message}`,
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
      const pendingAction = pendingConfigActionsRef.current.get(configToSave.id)

      if (
        pendingToken !== undefined &&
        pendingAction?.token === pendingToken &&
        pendingAction.action !== 'starting'
      ) {
        pendingConfigActionsRef.current.set(configToSave.id, {
          ...pendingAction,
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
