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
import { Config } from '@/types'

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

// eslint-disable-next-line max-statements
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
  const fetchConfigsWithState = useCallback(async () => {
    try {
      const configsResponse = await invoke<Config[]>('get_configs_cmd')
      const configStates = await invoke<Config[]>('get_config_states')

      return configsResponse.map(config => ({
        ...config,
        is_running:
          configStates.find(state => state.id === config.id)?.is_running ||
          false,
      }))
    } catch (error) {
      console.error('Failed to fetch configs:', error)
      throw error
    }
  }, [])

  const updateConfigsWithState = useCallback(async () => {
    try {
      const updatedConfigs = await fetchConfigsWithState()

      setConfigs(updatedConfigs)
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
    try {
      const updatedConfigToSave: Config = {
        ...newConfig,
        id: isEdit ? newConfig.id : 0,
      }
      let wasRunning = false
      const originalConfigsRunningState = new Map(
        configs.map(conf => [conf.id, conf.is_running]),
      )

      if (isEdit && originalConfigsRunningState.get(newConfig.id)) {
        wasRunning = true
        await stopPortForwardingForConfig(newConfig)
      }

      if (isEdit) {
        await invoke('update_config_cmd', { config: updatedConfigToSave })
      } else {
        await invoke('insert_config_cmd', { config: updatedConfigToSave })
      }

      if (wasRunning) {
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
  }

  const startPortForwardingForConfig = async (config: Config) => {
    if (config.workload_type === 'expose') {
      await invoke('start_port_forward_tcp_cmd', { configs: [config] })
    } else if (
      (config.workload_type === 'service' || config.workload_type === 'pod') &&
      config.protocol === 'tcp'
    ) {
      await invoke('start_port_forward_tcp_cmd', { configs: [config] })
    } else if (
      config.workload_type.startsWith('proxy') ||
      ((config.workload_type === 'service' || config.workload_type === 'pod') &&
        config.protocol === 'udp')
    ) {
      await invoke('deploy_and_forward_pod_cmd', { configs: [config] })
    } else {
      throw new Error(`Unsupported workload type: ${config.workload_type}`)
    }
  }

  const abortStartOperation = useCallback(() => {
    if (startAbortControllerRef.current) {
      startAbortControllerRef.current.abort()
      startAbortControllerRef.current = null
    }
    setIsInitiating(false)
    toaster.info({
      title: 'Aborted',
      description: 'Start operation was cancelled',
      duration: 2000,
    })
    updateConfigsWithState()
  }, [updateConfigsWithState])

  const abortStopOperation = useCallback(() => {
    if (stopAbortControllerRef.current) {
      stopAbortControllerRef.current.abort()
      stopAbortControllerRef.current = null
    }
    setIsStopping(false)
    toaster.info({
      title: 'Aborted',
      description: 'Stop operation was cancelled',
      duration: 2000,
    })
    updateConfigsWithState()
  }, [updateConfigsWithState])

  const START_TIMEOUT_MS = 60000
  const PER_CONFIG_TIMEOUT_MS = 30000

  const withConfigTimeout = <T, >(
    promise: Promise<T>,
    configId: number,
    ms: number,
  ): Promise<T> => {
    return new Promise((resolve, reject) => {
      const timeoutId = setTimeout(() => {
        reject(new Error(`Config ${configId} timed out after ${ms / 1000}s`))
      }, ms)

      promise
        .then(result => {
          clearTimeout(timeoutId)
          resolve(result)
        })
        .catch(err => {
          clearTimeout(timeoutId)
          reject(err)
        })
    })
  }

  const initiatePortForwarding = async (configsToStart: Config[]) => {
    if (startAbortControllerRef.current) {
      startAbortControllerRef.current.abort()
    }
    const abortController = new AbortController()


    startAbortControllerRef.current = abortController
    const abortSignal = abortController.signal

    setIsInitiating(true)

    let globalTimeoutId: NodeJS.Timeout | null = null

    try {
      const portForwardingPromises = configsToStart.map(async config => {
        if (abortSignal.aborted) {
          return { id: config.id, error: new Error('Aborted'), aborted: true }
        }
        try {
          await withConfigTimeout(
            handlePortForwarding(config),
            config.id,
            PER_CONFIG_TIMEOUT_MS,
          )
          return { id: config.id, error: null, aborted: false }
        } catch (error) {
          return { id: config.id, error, aborted: abortSignal.aborted }
        }
      })

      const timeoutPromise = new Promise<never>((_, reject) => {
        globalTimeoutId = setTimeout(() => {
          reject(new Error('Start operation timed out'))
        }, START_TIMEOUT_MS)

        abortSignal.addEventListener(
          'abort',
          () => {
            if (globalTimeoutId) {
              clearTimeout(globalTimeoutId)
              globalTimeoutId = null
            }
            reject(new Error('Aborted'))
          },
          { once: true },
        )
      })

      const results = await Promise.race([
        Promise.allSettled(portForwardingPromises),
        timeoutPromise,
      ])

      if (globalTimeoutId) {
        clearTimeout(globalTimeoutId)
        globalTimeoutId = null
      }

      if (!abortSignal.aborted) {
        const errors: Array<{ id: number; error: unknown }> = []


        for (const result of results) {
          if (result.status === 'fulfilled') {
            const value = result.value


            if (value && value.error != null && !value.aborted) {
              errors.push({ id: value.id, error: value.error })
            }
          }
        }

        if (errors.length > 0) {
          const errorCount = errors.length
          const firstError = errors[0]
          const errorMessage =
            firstError.error instanceof Error
              ? firstError.error.message
              : String(firstError.error)
          const isTimeout = errorMessage.includes('timed out')

          toaster.error({
            title: isTimeout ? 'Connection Timeout' : 'Start Failed',
            description:
              errorCount === 1
                ? `Config ${firstError.id}: ${errorMessage}`
                : `${errorCount} configs failed to start`,
            duration: 3000,
          })
        }
      }
    } catch (error) {
      if (globalTimeoutId) {
        clearTimeout(globalTimeoutId)
        globalTimeoutId = null
      }

      if (error instanceof Error && error.message !== 'Aborted') {
        console.error('Error during port forwarding:', error)
        toaster.warning({
          title: error.message.includes('timed out') ? 'Timeout' : 'Error',
          description: error.message.includes('timed out')
            ? 'Some port forwards may still be starting in the background.'
            : `Start operation failed: ${error.message}`,
          duration: 2000,
        })
      }
    } finally {
      if (startAbortControllerRef.current === abortController) {
        startAbortControllerRef.current = null
      }
      setIsInitiating(false)
      await updateConfigsWithState()
    }
  }
  const handlePortForwarding = async (config: Config) => {
    switch (config.workload_type) {
      case 'expose':
        await invoke<Response>('start_port_forward_tcp_cmd', {
          configs: [config],
        })
        break
      case 'service':
      case 'pod':
        if (config.protocol === 'tcp') {
          await invoke<Response>('start_port_forward_tcp_cmd', {
            configs: [config],
          })
        } else if (config.protocol === 'udp') {
          await invoke<Response>('deploy_and_forward_pod_cmd', {
            configs: [config],
          })
        }
        break
      case 'proxy':
        await invoke<Response>('deploy_and_forward_pod_cmd', {
          configs: [config],
        })
        break
      default:
        throw new Error(`Unsupported workload type: ${config.workload_type}`)
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

    try {
      await invoke('delete_config_cmd', { id: configToDelete })
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
    }
    setIsAlertOpen(false)
  }

  const startSelectedPortForwarding = async () => {
    const configsToStart = selectedConfigs
      .map(selected => configs.find(c => c.id === selected.id))
      .filter(
        (config): config is Config =>
          config !== undefined && !config.is_running,
      )

    if (configsToStart.length > 0) {
      await initiatePortForwarding(configsToStart)
    }
  }

  const STOP_TIMEOUT_MS = 30000

  const executeStopOperation = async (
    configsToStop: Config[],
    successMessage: string,
  ) => {
    if (stopAbortControllerRef.current) {
      stopAbortControllerRef.current.abort()
    }
    const abortController = new AbortController()


    stopAbortControllerRef.current = abortController
    const abortSignal = abortController.signal

    setIsStopping(true)
    let globalTimeoutId: NodeJS.Timeout | null = null

    try {
      const stopPromises = configsToStop.map(config =>
        stopPortForwardingForConfig(config),
      )

      const timeoutPromise = new Promise<never>((_, reject) => {
        globalTimeoutId = setTimeout(() => {
          reject(new Error('Stop operation timed out'))
        }, STOP_TIMEOUT_MS)

        abortSignal.addEventListener(
          'abort',
          () => {
            if (globalTimeoutId) {
              clearTimeout(globalTimeoutId)
              globalTimeoutId = null
            }
            reject(new Error('Aborted'))
          },
          { once: true },
        )
      })

      await Promise.race([Promise.allSettled(stopPromises), timeoutPromise])

      if (globalTimeoutId) {
        clearTimeout(globalTimeoutId)
        globalTimeoutId = null
      }

      if (!abortSignal.aborted) {
        toaster.success({
          title: 'Success',
          description: successMessage,
          duration: 1000,
        })
      }
    } catch (error) {
      if (globalTimeoutId) {
        clearTimeout(globalTimeoutId)
        globalTimeoutId = null
      }

      if (error instanceof Error && error.message !== 'Aborted') {
        console.error('Error stopping port forwards:', error)
        const isTimeout = error.message.includes('timed out')

        toaster.warning({
          title: isTimeout ? 'Partial Stop' : 'Error',
          description: isTimeout
            ? 'Some port forwards may not have stopped cleanly.'
            : 'Failed to stop port forwards.',
          duration: 2000,
        })
      }
    } finally {
      if (stopAbortControllerRef.current === abortController) {
        stopAbortControllerRef.current = null
      }
      setIsStopping(false)
      await updateConfigsWithState()
    }
  }

  const stopSelectedPortForwarding = async () => {
    const configsToStop = selectedConfigs
      .map(selected => configs.find(c => c.id === selected.id))
      .filter(
        (config): config is Config => config !== undefined && config.is_running,
      )

    if (configsToStop.length > 0) {
      await executeStopOperation(
        configsToStop,
        'Selected port forwards stopped successfully.',
      )
    }
  }

  const stopAllPortForwarding = async () => {
    const configsToStop = configs.filter(config => config.is_running)

    if (configsToStop.length > 0) {
      await executeStopOperation(
        configsToStop,
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
            bottom={0}
            overflow='auto'
            padding='5px'
          >
            <PortForwardTable
              configs={configs}
              initiatePortForwarding={initiatePortForwarding}
              startSelectedPortForwarding={startSelectedPortForwarding}
              isInitiating={isInitiating}
              setIsInitiating={setIsInitiating}
              isStopping={isStopping}
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
