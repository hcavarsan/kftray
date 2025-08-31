import React, { useCallback, useEffect, useRef, useState } from 'react'

import { Box, VStack } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs'

import AddConfigModal from '@/components/AddConfigModal'
import AutoImportModal from '@/components/AutoImportModal'
import Footer from '@/components/Footer'
import GitSyncModal from '@/components/GitSyncModal'
import PortForwardTable from '@/components/PortForwardTable'
import { toaster } from '@/components/ui/toaster'
import { useSyncManager } from '@/hooks/useSyncManager'
import { Config } from '@/types'

const initialRemotePort = 0
const initialLocalPort = 0
const initialId = 0

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
  const [isAlertOpen, setIsAlertOpen] = useState(false)
  const [configToDelete, setConfigToDelete] = useState<number | undefined>()
  const [isAutoImportModalOpen, setIsAutoImportModalOpen] = useState(false)
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
      (config.workload_type === 'service' || config.workload_type === 'pod') &&
      config.protocol === 'tcp'
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
    if (
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

  const initiatePortForwarding = async (configsToStart: Config[]) => {
    setIsInitiating(true)

    const portForwardingPromises = configsToStart.map(async config => {
      try {
        await handlePortForwarding(config)

        return { id: config.id, error: null }
      } catch (error) {
        return { id: config.id, error }
      }
    })

    const results = await Promise.allSettled(portForwardingPromises)

    const errors = results
      .map(result => (result.status === 'fulfilled' ? result.value : null))
      .filter(
        (result): result is { id: number; error: any } => result?.error != null,
      )

    if (errors.length > 0) {
      const errorMessage = errors
        .map(e => `Config ID: ${e.id}, Error: ${e.error}`)
        .join(', ')

      toaster.error({
        title: 'Error Starting Port Forwarding',
        description: `Some configs failed: ${errorMessage}`,
        duration: 1000,
      })
    }

    setIsInitiating(false)
  }
  const handlePortForwarding = async (config: Config) => {
    switch (config.workload_type) {
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

  const stopSelectedPortForwarding = async () => {
    const configsToStop = selectedConfigs
      .map(selected => configs.find(c => c.id === selected.id))
      .filter(
        (config): config is Config => config !== undefined && config.is_running,
      )

    if (configsToStop.length > 0) {
      setIsStopping(true)
      try {
        const stopPromises = configsToStop.map(config =>
          stopPortForwardingForConfig(config),
        )

        await Promise.all(stopPromises)
        toaster.success({
          title: 'Success',
          description: 'Selected port forwards stopped successfully.',
          duration: 1000,
        })
      } catch (error) {
        console.error('Error stopping selected port forwards:', error)
        toaster.error({
          title: 'Error',
          description: 'Failed to stop selected port forwards.',
          duration: 1000,
        })
      } finally {
        setIsStopping(false)
      }
    }
  }

  const stopAllPortForwarding = async () => {
    const configsToStop = configs.filter(config => config.is_running)

    if (configsToStop.length > 0) {
      setIsStopping(true)
      try {
        const stopPromises = configsToStop.map(config =>
          stopPortForwardingForConfig(config),
        )

        await Promise.all(stopPromises)
        toaster.success({
          title: 'Success',
          description:
            'Port forwarding stopped successfully for all configurations.',
          duration: 1000,
        })
      } catch (error) {
        console.error(
          'An error occurred while stopping port forwarding:',
          error,
        )
        toaster.error({
          title: 'Error',
          description: `An error occurred while stopping port forwarding: ${error}`,
          duration: 1000,
        })
      } finally {
        setIsStopping(false)
      }
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
              stopSelectedPortForwarding={stopSelectedPortForwarding}
              stopAllPortForwarding={stopAllPortForwarding}
              handleDeleteConfig={handleDeleteConfig}
              confirmDeleteConfig={confirmDeleteConfig}
              isAlertOpen={isAlertOpen}
              setIsAlertOpen={setIsAlertOpen}
              selectedConfigs={selectedConfigs}
              setSelectedConfigs={setSelectedConfigs}
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
            zIndex={10}
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
            />
          </Box>
        </Box>

        {/* Modals */}
        <GitSyncModal
          isGitSyncModalOpen={isGitSyncModalOpen}
          closeGitSyncModal={closeGitSyncModal}
          setCredentialsSaved={handleSetCredentialsSaved}
          credentialsSaved={credentialsSaved}
          setPollingInterval={handleSetPollingInterval}
          pollingInterval={pollingInterval}
        />

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

        <AutoImportModal
          isOpen={isAutoImportModalOpen}
          onClose={() => setIsAutoImportModalOpen(false)}
        />
      </VStack>
    </Box>
  )
}

export default KFTray
