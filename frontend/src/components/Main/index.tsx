import React, {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react'

import { Box, VStack } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs'

import Footer from '@/components/Footer'
import PortForwardTable from '@/components/PortForwardTable'
import { toaster } from '@/components/ui/toaster'
import { useSyncManager } from '@/hooks/useSyncManager'
import type { Config, PortForwardAction, PortForwardResponse } from '@/types'

const AddConfigModal = lazy(() => import('@/components/AddConfigModal'))
const AutoImportModal = lazy(() => import('@/components/AutoImportModal'))
const GitSyncModal = lazy(() => import('@/components/GitSyncModal'))
const ServerResourcesModal = lazy(
  () => import('@/components/ServerResourcesModal'),
)
const SettingsModal = lazy(() => import('@/components/SettingsModal'))
const ShortcutModal = lazy(() => import('@/components/ShortcutModal'))

const initialRemotePort = 0
const initialLocalPort = 0
const initialId = 0

const CONCURRENCY_LIMIT = 8

async function runWithLimit<T, R>(
  items: T[],
  limit: number,
  worker: (item: T) => Promise<R>,
): Promise<R[]> {
  const results: R[] = new Array(items.length)
  let nextIndex = 0

  const runWorker = async () => {
    while (nextIndex < items.length) {
      const currentIndex = nextIndex
      nextIndex += 1
      results[currentIndex] = await worker(items[currentIndex])
    }
  }

  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, runWorker),
  )

  return results
}

const KFTray = () => {
  const [pollingInterval, setPollingInterval] = useState(0)
  const [configs, setConfigs] = useState<Config[]>([])
  const [isModalOpen, setIsModalOpen] = useState(false)
  const [isGitSyncModalOpen, setIsGitSyncModalOpen] = useState(false)
  const [selectedConfigs, setSelectedConfigs] = useState<Config[]>([])
  const [credentialsSaved, setCredentialsSaved] = useState(false)
  const [isEdit, setIsEdit] = useState(false)
  const [newConfig, setNewConfig] = useState<Config>({
    id: 0,
    service: '',
    context: '',
    local_port: 0,
    remote_port: 0,
    local_address: '127.0.0.1',
    auto_loopback_address: false,
    domain_enabled: false,
    namespace: '',
    workload_type: '',
    target: '',
    protocol: '',
    remote_address: '',
    alias: '',
    kubeconfig: 'default',
    is_running: false,
  })
  const cancelRef = React.useRef<HTMLElement>(null)
  const [isInitiating, setIsInitiating] = useState(false)
  const [isStopping, setIsStopping] = useState(false)
  const startAbortControllerRef = useRef<AbortController | null>(null)
  const stopAbortControllerRef = useRef<AbortController | null>(null)
  const [isAlertOpen, setIsAlertOpen] = useState(false)
  const [configToDelete, setConfigToDelete] = useState<number | undefined>()
  const [isAutoImportModalOpen, setIsAutoImportModalOpen] = useState(false)
  const [isShortcutModalOpen, setIsShortcutModalOpen] = useState(false)
  const [isServerResourcesModalOpen, setIsServerResourcesModalOpen] =
    useState(false)
  const [isSettingsModalOpen, setIsSettingsModalOpen] = useState(false)

  const pendingConfigActionsRef = useRef<Map<number, PortForwardAction>>(
    new Map(),
  )
  const [pendingConfigActions, setPendingConfigActions] = useState<
    Map<number, PortForwardAction>
  >(new Map())
  const configRefreshVersion = useRef(0)

  const markPending = (id: number, action: PortForwardAction) => {
    pendingConfigActionsRef.current.set(id, action)
    setPendingConfigActions(new Map(pendingConfigActionsRef.current))
  }

  const clearPending = (id: number) => {
    pendingConfigActionsRef.current.delete(id)
    setPendingConfigActions(new Map(pendingConfigActionsRef.current))
  }

  const fetchConfigsWithState = useCallback(async () => {
    try {
      const [configsResponse, configStates] = await Promise.all([
        invoke<Config[]>('get_configs_cmd'),
        invoke<Array<{ config_id: number; is_running: boolean }>>(
          'get_config_states',
        ),
      ])
      const statesById = new Map(
        configStates.map(state => [state.config_id, state.is_running]),
      )

      return configsResponse.map(config => ({
        ...config,
        is_running: statesById.get(config.id) ?? false,
      }))
    } catch (error) {
      console.error('Failed to fetch configs:', error)
      throw error
    }
  }, [])

  const updateConfigsWithState = useCallback(async () => {
    const version = ++configRefreshVersion.current
    try {
      const updatedConfigs = await fetchConfigsWithState()

      if (version === configRefreshVersion.current) {
        setConfigs(updatedConfigs)
      }
    } catch (error) {
      console.error('Error updating configs:', error)
    }
  }, [fetchConfigsWithState])

  const debouncedUpdateTimer = useRef<NodeJS.Timeout | null>(null)
  const debouncedUpdateConfigs = useCallback(() => {
    if (debouncedUpdateTimer.current) {
      clearTimeout(debouncedUpdateTimer.current)
    }
    debouncedUpdateTimer.current = setTimeout(() => {
      updateConfigsWithState()
    }, 100)
  }, [updateConfigsWithState])

  useEffect(() => {
    let isMounted = true

    const fetchConfigs = async () => {
      try {
        const configsWithState = await fetchConfigsWithState()

        if (isMounted) {
          setConfigs(configsWithState)
          console.log('configsWithState:', configsWithState)
        }
      } catch (error) {
        console.error('Failed to fetch configs:', error)
      }
    }

    fetchConfigs()

    let unsubscribe: (() => void) | undefined

    const setupListener = async () => {
      try {
        unsubscribe = await listen('config_state_changed', async () => {
          if (isMounted) {
            debouncedUpdateConfigs()
            console.log('config_state_changed')
          }
        })
      } catch (error) {
        console.error('Failed to setup event listener:', error)
      }
    }

    setupListener()

    return () => {
      isMounted = false
      if (unsubscribe) {
        unsubscribe()
      }
    }
  }, [fetchConfigsWithState, debouncedUpdateConfigs])

  const openModal = () => {
    setNewConfig({
      id: initialId,
      service: '',
      context: '',
      local_port: initialLocalPort,
      local_address: '127.0.0.1',
      auto_loopback_address: false,
      domain_enabled: false,
      remote_port: initialRemotePort,
      namespace: '',
      workload_type: '',
      target: '',
      protocol: '',
      remote_address: '',
      alias: '',
      kubeconfig: 'default',
      is_running: false,
    })
    setIsEdit(false)
    setIsModalOpen(true)
  }

  const closeModal = () => {
    setIsModalOpen(false)
    setIsEdit(false)
  }

  const closeGitSyncModal = () => {
    setIsGitSyncModalOpen(false)
  }

  const openGitSyncModal = () => {
    setIsGitSyncModalOpen(true)
  }

  const openShortcutModal = () => {
    setIsShortcutModalOpen(true)
  }

  const closeShortcutModal = () => {
    setIsShortcutModalOpen(false)
  }

  const openServerResourcesModal = () => {
    setIsServerResourcesModalOpen(true)
  }

  const closeServerResourcesModal = () => {
    setIsServerResourcesModalOpen(false)
  }

  const openSettingsModal = () => {
    setIsSettingsModalOpen(true)
  }

  const closeSettingsModal = () => {
    setIsSettingsModalOpen(false)
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target

    setNewConfig(prev => ({
      ...prev,
      [name]:
        name === 'local_port' || name === 'remote_port'
          ? Number(value || 0)
          : value,
    }))
  }

  const handleExportConfigs = async () => {
    try {
      await invoke('open_save_dialog')
      const json = await invoke('export_configs_cmd')

      if (typeof json !== 'string') {
        throw new Error('The exported config is not a string')
      }

      const filePath = await save({
        defaultPath: 'configs.json',
        filters: [{ name: 'JSON', extensions: ['json'] }],
      })

      await invoke('close_save_dialog')

      if (filePath) {
        await writeTextFile(filePath, json)
        toaster.success({
          title: 'Success',
          description: 'Configuration exported successfully.',
          duration: 1000,
        })
      }
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      console.error('Failed to export configs:', errorMessage)
      toaster.error({
        title: 'Failed to export configs',
        description: errorMessage,
        duration: 1000,
      })
    }
  }

  const handleImportConfigs = async () => {
    try {
      await invoke('open_save_dialog')
      const selected = await open({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        multiple: false,
      })

      await invoke('close_save_dialog')

      if (typeof selected === 'string') {
        const jsonContent = await readTextFile(selected)

        await invoke('import_configs_cmd', { json: jsonContent })
        toaster.success({
          title: 'Success',
          description: 'Configuration imported successfully.',
          duration: 1000,
        })
      } else {
        toaster.error({
          title: 'Error',
          description: 'Failed to import configurations.',
          duration: 1000,
        })
      }
    } catch (error) {
      console.error('Error during import:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to import configurations.',
        duration: 1000,
      })
    }
  }

  const handleEditConfig = async (id: number) => {
    try {
      const configToEdit = await invoke<Config>('get_config_cmd', { id })

      setNewConfig(configToEdit)
      setIsEdit(true)
      setIsModalOpen(true)
    } catch (error) {
      console.error(
        `Failed to fetch the config for editing with id ${id}:`,
        error,
      )
    }
  }

  const handleDuplicateConfig = async (id: number) => {
    try {
      const configToDuplicate = await invoke<Config>('get_config_cmd', { id })

      setNewConfig({
        ...configToDuplicate,
        id: 0,
        alias: `${configToDuplicate.alias}-copy`,
        is_running: false,
      })
      setIsEdit(false)
      setIsModalOpen(true)
    } catch (error) {
      console.error(
        `Failed to fetch the config for duplication with id ${id}:`,
        error,
      )
    }
  }

  const handleEditSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    try {
      await invoke('update_config_cmd', { config: newConfig })
      toaster.success({
        title: 'Success',
        description: 'Configuration updated successfully.',
        duration: 1000,
      })
      closeModal()
    } catch (error) {
      toaster.error({
        title: 'Error',
        description: `Failed to update configuration. ${error instanceof Error ? error.message : 'Unknown error'}`,
        duration: 1000,
      })
    }
  }

  const handleSaveConfig = async (_configToSave: Config) => {
    const wasRunning = Boolean(
      isEdit && configs.find(conf => conf.id === newConfig.id)?.is_running,
    )

    if (isEdit && pendingConfigActionsRef.current.has(newConfig.id)) {
      toaster.error({
        title: 'Error',
        description: 'This configuration is busy. Try again once it settles.',
        duration: 1000,
      })

      return
    }

    // Reserved for the whole transaction, not only when a restart is needed:
    // `update_config_cmd` does not share the backend lifecycle lock, so a start
    // accepted while it is in flight would use the pre-edit snapshot.
    if (isEdit) {
      markPending(newConfig.id, wasRunning ? 'stopping' : 'starting')
    }

    try {
      const updatedConfigToSave: Config = {
        ...newConfig,
        id: isEdit ? newConfig.id : 0,
      }

      if (wasRunning) {
        await stopPortForwardingForConfig(newConfig)
      }

      if (isEdit) {
        await invoke('update_config_cmd', { config: updatedConfigToSave })
      } else {
        await invoke('insert_config_cmd', { config: updatedConfigToSave })
      }

      if (wasRunning) {
        markPending(newConfig.id, 'starting')
        await startPortForwardingForConfig(newConfig)
      }

      toaster.success({
        title: 'Success',
        description: `Configuration ${isEdit ? 'updated' : 'added'} successfully.`,
        duration: 1000,
      })
      closeModal()
    } catch (error) {
      console.error(`Failed to ${isEdit ? 'update' : 'add'} config:`, error)
      toaster.error({
        title: 'Error',
        description: `Failed to ${isEdit ? 'update' : 'add'} configuration.`,
        duration: 1000,
      })
    } finally {
      if (isEdit) {
        clearPending(newConfig.id)
      }
      // The optimistic update only flips is_running, and the restart's version
      // bump discards any refresh that raced it.
      debouncedUpdateConfigs()
    }
  }

  const stopPortForwardingForConfig = async (config: Config) => {
    if (
      config.workload_type === 'expose' ||
      ((config.workload_type === 'service' || config.workload_type === 'pod') &&
        config.protocol === 'tcp')
    ) {
      await invoke('stop_port_forward_cmd', {
        serviceName: config.service,
        configId: config.id.toString(),
      })
    } else if (
      config.workload_type.startsWith('proxy') ||
      ((config.workload_type === 'service' || config.workload_type === 'pod') &&
        config.protocol === 'udp')
    ) {
      await invoke('stop_proxy_forward_cmd', {
        configId: config.id.toString(),
        namespace: config.namespace,
        serviceName: config.service,
        localPort: config.local_port,
        remoteAddress: config.remote_address,
        protocol: 'tcp',
      })
    } else {
      throw new Error(`Unsupported workload type: ${config.workload_type}`)
    }
    configRefreshVersion.current += 1
    setConfigs(current =>
      current.map(item =>
        item.id === config.id ? { ...item, is_running: false } : item,
      ),
    )
  }

  const startPortForwardingForConfig = async (config: Config) => {
    let responses: PortForwardResponse[]

    if (
      config.workload_type === 'expose' ||
      ((config.workload_type === 'service' || config.workload_type === 'pod') &&
        config.protocol === 'tcp')
    ) {
      responses = await invoke('start_port_forward_tcp_cmd', {
        configs: [config],
      })
    } else if (
      config.workload_type.startsWith('proxy') ||
      ((config.workload_type === 'service' || config.workload_type === 'pod') &&
        config.protocol === 'udp')
    ) {
      responses = await invoke('deploy_and_forward_pod_cmd', {
        configs: [config],
      })
    } else {
      throw new Error(`Unsupported workload type: ${config.workload_type}`)
    }

    const failure = responses.find(response => response.status !== 0)

    if (failure) {
      throw new Error(failure.stderr || 'Failed to start port forwarding.')
    }

    configRefreshVersion.current += 1
    setConfigs(current =>
      current.map(item =>
        item.id === config.id ? { ...item, is_running: true } : item,
      ),
    )
  }

  const toggleConfigForward = async (
    config: Config,
    action: PortForwardAction,
  ) => {
    if (pendingConfigActionsRef.current.has(config.id)) {
      return
    }
    markPending(config.id, action)
    try {
      if (action === 'starting') {
        await startPortForwardingForConfig(config)
      } else {
        await stopPortForwardingForConfig(config)
      }
    } catch (error) {
      await updateConfigsWithState()
      toaster.error({
        title:
          action === 'starting'
            ? 'Error starting port forwarding'
            : 'Error stopping port forwarding',
        description: error instanceof Error ? error.message : String(error),
        duration: 1000,
      })
    } finally {
      clearPending(config.id)
      debouncedUpdateConfigs()
    }
  }

  const abortStartOperation = useCallback(() => {
    if (startAbortControllerRef.current) {
      startAbortControllerRef.current.abort()
    }
    toaster.info({
      title: 'Aborted',
      description: 'Queued starts cancelled. Active starts will finish.',
      duration: 2000,
    })
    updateConfigsWithState()
  }, [updateConfigsWithState])

  const abortStopOperation = useCallback(() => {
    if (stopAbortControllerRef.current) {
      stopAbortControllerRef.current.abort()
    }
    toaster.info({
      title: 'Aborted',
      description: 'Queued stops cancelled. Active stops will finish.',
      duration: 2000,
    })
    updateConfigsWithState()
  }, [updateConfigsWithState])

  const runPortForwardBatch = async (
    candidates: Config[],
    action: PortForwardAction,
    successMessage?: string,
  ) => {
    const controllerRef =
      action === 'starting' ? startAbortControllerRef : stopAbortControllerRef
    if (controllerRef.current) {
      return
    }
    const targets = candidates.filter(
      config => !pendingConfigActionsRef.current.has(config.id),
    )
    if (targets.length === 0) {
      return
    }
    const controller = new AbortController()
    controllerRef.current = controller
    const setBusy = action === 'starting' ? setIsInitiating : setIsStopping
    const queued = new Set(targets.map(config => config.id))
    for (const config of targets) {
      pendingConfigActionsRef.current.set(config.id, action)
    }
    setPendingConfigActions(new Map(pendingConfigActionsRef.current))
    setBusy(true)

    const cancelQueued = () => {
      for (const id of queued) {
        pendingConfigActionsRef.current.delete(id)
      }
      queued.clear()
      setPendingConfigActions(new Map(pendingConfigActionsRef.current))
    }
    controller.signal.addEventListener('abort', cancelQueued, { once: true })
    try {
      const results = await runWithLimit(
        targets,
        CONCURRENCY_LIMIT,
        async config => {
          if (controller.signal.aborted) {
            return { id: config.id, error: null }
          }
          queued.delete(config.id)
          try {
            if (action === 'starting') {
              await startPortForwardingForConfig(config)
            } else {
              await stopPortForwardingForConfig(config)
            }
            return { id: config.id, error: null }
          } catch (error) {
            return { id: config.id, error }
          } finally {
            clearPending(config.id)
            debouncedUpdateConfigs()
          }
        },
      )
      const failures = results.filter(result => result.error != null)
      if (failures.length > 0) {
        const first = failures[0]
        const message =
          first.error instanceof Error
            ? first.error.message
            : String(first.error)
        toaster.error({
          title: action === 'starting' ? 'Start Failed' : 'Stop Failed',
          description:
            failures.length === 1
              ? `Config ${first.id}: ${message}`
              : `${failures.length} configs failed to ${action === 'starting' ? 'start' : 'stop'}`,
          duration: 3000,
        })
      } else if (successMessage && !controller.signal.aborted) {
        toaster.success({
          title: 'Success',
          description: successMessage,
          duration: 1000,
        })
      }
    } finally {
      controller.signal.removeEventListener('abort', cancelQueued)
      if (controllerRef.current === controller) {
        controllerRef.current = null
        setBusy(false)
      }
      await updateConfigsWithState()
    }
  }

  const handleDeleteConfig = async (id: number) => {
    setConfigToDelete(id)

    setIsAlertOpen(true)
  }

  const confirmDeleteConfig = async () => {
    if (typeof configToDelete !== 'number') {
      toaster.error({
        title: 'Error',
        description: 'Configuration id is undefined.',
        duration: 1000,
      })

      return
    }

    if (pendingConfigActionsRef.current.has(configToDelete)) {
      toaster.error({
        title: 'Error',
        description: 'This configuration is busy. Try again once it settles.',
        duration: 1000,
      })
      setIsAlertOpen(false)

      return
    }

    // Reserved until the refreshed list no longer carries the row: the backend
    // delete only removes the database row and does not serialize with starts.
    markPending(configToDelete, 'stopping')
    try {
      await invoke('delete_config_cmd', { id: configToDelete })
      await updateConfigsWithState()
      toaster.success({
        title: 'Success',
        description: 'Configuration deleted successfully.',
        duration: 1000,
      })
    } catch (error) {
      console.error('Failed to delete configuration:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to delete configuration: "unknown error"',
        duration: 1000,
      })
    } finally {
      clearPending(configToDelete)
    }
    setIsAlertOpen(false)
  }

  const deleteConfigs = async (ids: number[]): Promise<boolean> => {
    const busy = ids.filter(id => pendingConfigActionsRef.current.has(id))

    if (busy.length) {
      toaster.error({
        title: 'Error',
        description: `${busy.length} selected configuration(s) are busy. Try again once they settle.`,
        duration: 2000,
      })

      return false
    }

    for (const id of ids) {
      markPending(id, 'stopping')
    }
    try {
      await invoke('delete_configs_cmd', { ids })
      await updateConfigsWithState()
      toaster.success({
        title: 'Success',
        description: 'Configurations deleted successfully.',
        duration: 1000,
      })

      return true
    } catch (error) {
      console.error('Failed to delete configurations:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to delete configurations.',
        duration: 1000,
      })

      return false
    } finally {
      for (const id of ids) {
        clearPending(id)
      }
    }
  }

  const startSelectedPortForwarding = async () => {
    const configsToStart = selectedConfigs
      .map(selected => configs.find(c => c.id === selected.id))
      .filter(
        (config): config is Config =>
          config !== undefined && !config.is_running,
      )

    if (configsToStart.length > 0) {
      await runPortForwardBatch(configsToStart, 'starting')
    }
  }

  const stopSelectedPortForwarding = async () => {
    const configsToStop = selectedConfigs
      .map(selected => configs.find(c => c.id === selected.id))
      .filter((config): config is Config => config?.is_running === true)

    if (configsToStop.length > 0) {
      await runPortForwardBatch(
        configsToStop,
        'stopping',
        'Selected port forwards stopped successfully.',
      )
    }
  }

  const stopAllPortForwarding = async () => {
    const configsToStop = configs.filter(config => config.is_running)

    if (configsToStop.length > 0) {
      await runPortForwardBatch(
        configsToStop,
        'stopping',
        'Port forwarding stopped successfully for all configurations.',
      )
    }
  }

  const handleSetCredentialsSaved = useCallback((value: boolean) => {
    setCredentialsSaved(value)
  }, [])

  const handleSyncComplete = useCallback(() => {
    updateConfigsWithState()
  }, [updateConfigsWithState])

  const handleSyncFailure = useCallback((error: Error) => {
    console.error('Sync failed:', error)
    toaster.error({
      title: 'Sync Failed',
      description: error.message,
      duration: 3000,
    })
  }, [])

  const { syncStatus, updateSyncStatus } = useSyncManager({
    onSyncFailure: handleSyncFailure,
    onSyncComplete: handleSyncComplete,
    credentialsSaved,
  })

  const handleSetPollingInterval = useCallback(
    (value: number) => {
      setPollingInterval(value)
      updateSyncStatus({
        pollingInterval: value,
      })
    },
    [updateSyncStatus],
  )

  return (
    <Box
      position='fixed'
      width='100%'
      height='100%'
      maxHeight='100%'
      maxW='100%'
      overflow='hidden'
      bg='#111111'
      borderRadius='lg'
    >
      <VStack
        height='100%'
        width='100%'
        gap={0}
        position='relative'
        overflow='hidden'
      >
        {/* Main Content Area */}
        <Box
          flex={1}
          width='100%'
          height='100%'
          position='relative'
          overflow='hidden'
          bg='#111111'
        >
          {/* Port Forward Table */}
          <Box
            position='absolute'
            top={0}
            left={0}
            right={0}
            bottom='60px'
            overflow='auto'
            padding='5px'
          >
            <PortForwardTable
              configs={configs}
              initiatePortForwarding={configsToStart =>
                runPortForwardBatch(configsToStart, 'starting')
              }
              startSelectedPortForwarding={startSelectedPortForwarding}
              isInitiating={isInitiating}
              isStopping={isStopping}
              pendingConfigActions={pendingConfigActions}
              toggleConfigForward={toggleConfigForward}
              handleEditConfig={handleEditConfig}
              handleDuplicateConfig={handleDuplicateConfig}
              stopSelectedPortForwarding={stopSelectedPortForwarding}
              stopAllPortForwarding={stopAllPortForwarding}
              abortStartOperation={abortStartOperation}
              abortStopOperation={abortStopOperation}
              handleDeleteConfig={handleDeleteConfig}
              confirmDeleteConfig={confirmDeleteConfig}
              isAlertOpen={isAlertOpen}
              setIsAlertOpen={setIsAlertOpen}
              selectedConfigs={selectedConfigs}
              setSelectedConfigs={setSelectedConfigs}
              openSettingsModal={openSettingsModal}
              openServerResourcesModal={openServerResourcesModal}
            />
          </Box>

          {/* Footer Area */}
          <Box
            position='absolute'
            left={0}
            right={0}
            bottom={0}
            overflow='hidden'
            padding='5px'
            zIndex={1}
          >
            <Footer
              openModal={openModal}
              openGitSyncModal={openGitSyncModal}
              handleExportConfigs={handleExportConfigs}
              handleImportConfigs={handleImportConfigs}
              setCredentialsSaved={handleSetCredentialsSaved}
              credentialsSaved={credentialsSaved}
              isGitSyncModalOpen={isGitSyncModalOpen}
              selectedConfigs={selectedConfigs}
              setPollingInterval={handleSetPollingInterval}
              pollingInterval={pollingInterval}
              setSelectedConfigs={setSelectedConfigs}
              configs={configs}
              syncStatus={syncStatus}
              onSyncComplete={handleSyncComplete}
              openShortcutModal={openShortcutModal}
              setIsAutoImportModalOpen={setIsAutoImportModalOpen}
              deleteConfigs={deleteConfigs}
            />
          </Box>
        </Box>

        <Suspense fallback={null}>
          {isGitSyncModalOpen && (
            <GitSyncModal
              isGitSyncModalOpen={isGitSyncModalOpen}
              closeGitSyncModal={closeGitSyncModal}
              setCredentialsSaved={handleSetCredentialsSaved}
              credentialsSaved={credentialsSaved}
              setPollingInterval={handleSetPollingInterval}
              pollingInterval={pollingInterval}
            />
          )}

          {isModalOpen && (
            <AddConfigModal
              isModalOpen={isModalOpen}
              closeModal={closeModal}
              newConfig={newConfig}
              handleInputChange={handleInputChange}
              handleSaveConfig={handleSaveConfig}
              isEdit={isEdit}
              handleEditSubmit={handleEditSubmit}
              cancelRef={cancelRef as React.RefObject<HTMLElement>}
              setNewConfig={setNewConfig}
            />
          )}

          {isAutoImportModalOpen && (
            <AutoImportModal
              isOpen={isAutoImportModalOpen}
              onClose={() => setIsAutoImportModalOpen(false)}
            />
          )}

          {isShortcutModalOpen && (
            <ShortcutModal
              isOpen={isShortcutModalOpen}
              onClose={closeShortcutModal}
            />
          )}

          {isServerResourcesModalOpen && (
            <ServerResourcesModal
              isOpen={isServerResourcesModalOpen}
              onClose={closeServerResourcesModal}
            />
          )}

          {isSettingsModalOpen && (
            <SettingsModal
              isOpen={isSettingsModalOpen}
              onClose={closeSettingsModal}
            />
          )}
        </Suspense>
      </VStack>
    </Box>
  )
}

export default KFTray
