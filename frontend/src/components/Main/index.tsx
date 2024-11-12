import React, { useCallback, useEffect, useState } from 'react'

import { Box, VStack } from '@chakra-ui/react'
import { open, save } from '@tauri-apps/api/dialog'
import { listen } from '@tauri-apps/api/event'
import { readTextFile, writeTextFile } from '@tauri-apps/api/fs'
import { invoke } from '@tauri-apps/api/tauri'

import AddConfigModal from '@/components/AddConfigModal'
import AutoImportModal from '@/components/AutoImportModal'
import Footer from '@/components/Footer'
import GitSyncModal from '@/components/GitSyncModal'
import PortForwardTable from '@/components/PortForwardTable'
import { useCustomToast } from '@/components/ui/toaster'
import { Config, Response } from '@/types'

const initialRemotePort = 0
const initialLocalPort = 0
const initialId = 0
const initialStatus = 0

const KFTray = () => {
  const toast = useCustomToast()
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

    const unlisten = listen('config_state_changed', async () => {
      await updateConfigsWithState()
      console.log('config_state_changed')
    })

    return () => {
      isMounted = false
      unlisten.then(unsub => unsub())
    }
  }, [fetchConfigsWithState, updateConfigsWithState])

  const openModal = () => {
    setNewConfig({
      id: initialId,
      service: '',
      context: '',
      local_port: initialLocalPort,
      local_address: '127.0.0.1',
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
    const updatedValue =
      name === 'local_port' || name === 'remote_port'
        ? value === Number(0).toString()
          ? Number(0).toString()
          : Number(value)
        : value

    setNewConfig(prev => ({ ...prev, [name]: updatedValue }))
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
        toast({
          title: 'Success',
          description: 'Configuration exported successfully.',
          status: 'success',
        })
      }
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      console.error('Failed to export configs:', errorMessage)
      toast({
        title: 'Failed to export configs',
        description: errorMessage,
        status: 'error',
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
        toast({
          title: 'Success',
          description: 'Configuration imported successfully.',
          status: 'success',
        })
      } else {
        toast({
          title: 'Error',
          description: 'Failed to import configurations.',
          status: 'error',
        })
      }
    } catch (error) {
      console.error('Error during import:', error)
      toast({
        title: 'Error',
        description: 'Failed to import configurations.',
        status: 'error',
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
      toast({
        title: 'Success',
        description: 'Configuration updated successfully.',
        status: 'success',
      })
      closeModal()
    } catch (error) {
      toast({
        title: 'Error',
        description: `Failed to update configuration. ${error instanceof Error ? error.message : 'Unknown error'}`,
        status: 'error',
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

      toast({
        title: 'Success',
        description: `Configuration ${
          isEdit ? 'updated' : 'added'
        } successfully.`,
        status: 'success',
      })
      closeModal()
    } catch (error) {
      console.error(`Failed to ${isEdit ? 'update' : 'add'} config:`, error)
      toast({
        title: 'Error',
        description: `Failed to ${isEdit ? 'update' : 'add'} configuration.`,
        status: 'error',
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
    .filter(result => result && result.error) as { id: number; error: any }[]

    if (errors.length > 0) {
      const errorMessage = errors
      .map(e => `Config ID: ${e.id}, Error: ${e.error}`)
      .join(', ')

      toast({
        title: 'Error Starting Port Forwarding',
        description: `Some configs failed: ${errorMessage}`,
        status: 'error',
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
      toast({
        title: 'Error',
        description: 'Configuration id is undefined.',
        status: 'error',
      })

      return
    }

    try {
      await invoke('delete_config_cmd', { id: configToDelete })

      toast({
        title: 'Success',
        description: 'Configuration deleted successfully.',
        status: 'success',
      })
    } catch (error) {
      console.error('Failed to delete configuration:', error)
      toast({
        title: 'Error',
        description: 'Failed to delete configuration: "unknown error"',
        status: 'error',
      })
    }
    setIsAlertOpen(false)
  }

  const stopAllPortForwarding = async () => {
    setIsStopping(true)
    try {
      const responses = await invoke<Response[]>('stop_all_port_forward_cmd')
      const allStopped = responses.every(res => res.status === initialStatus)

      if (allStopped) {
        toast({
          title: 'Success',
          description:
            'Port forwarding stopped successfully for all configurations.',
          status: 'success',
        })
      } else {
        const errorMessages = responses
        .filter(res => res.status !== initialStatus)
        .map(res => `${res.service}: ${res.stderr}`)
        .join(', ')

        toast({
          title: 'Error',
          description: `Port forwarding failed for some configurations: ${errorMessages}`,
          status: 'error',
        })
      }
    } catch (error) {
      console.error('An error occurred while stopping port forwarding:', error)
      toast({
        title: 'Error',
        description: `An error occurred while stopping port forwarding: ${error}`,
        status: 'error',
      })
    }
    setIsStopping(false)
  }

  return (
    <Box
      position="fixed"
      width="100%"
      height="100%"
      maxHeight="100%"
      maxW="100%"
      overflow="hidden"
      bg="#111111"
      borderRadius="lg"
    >
      <VStack
        height="100%"
        width="100%"
        gap={0}
        position="relative"
        overflow="hidden"
      >
        {/* Main Content Area */}
        <Box
          flex={1}
          width="100%"
          height="100%"
          position="relative"
          overflow="hidden"
          bg="#111111"

        >
          {/* Port Forward Table */}
          <Box
            position="absolute"
            top={0}
            left={0}
            right={0}
            bottom={0}
            overflow="hidden"
            padding="5px"
          >
            <PortForwardTable
              configs={configs}
              initiatePortForwarding={initiatePortForwarding}
              isInitiating={isInitiating}
              setIsInitiating={setIsInitiating}
              isStopping={isStopping}
              handleEditConfig={handleEditConfig}
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
            position="absolute"

            left={0}
            right={0}
            bottom={0}
            overflow="hidden"
            padding="5px"

            zIndex={10}

          >
            <Footer
              openModal={openModal}
              openGitSyncModal={openGitSyncModal}
              handleExportConfigs={handleExportConfigs}
              handleImportConfigs={handleImportConfigs}
              setCredentialsSaved={setCredentialsSaved}
              credentialsSaved={credentialsSaved}
              isGitSyncModalOpen={isGitSyncModalOpen}
              selectedConfigs={selectedConfigs}
              setPollingInterval={setPollingInterval}
              pollingInterval={pollingInterval}
              setSelectedConfigs={setSelectedConfigs}
              configs={configs}
            />
          </Box>
        </Box>

        {/* Modals */}
        <GitSyncModal
          isGitSyncModalOpen={isGitSyncModalOpen}
          closeGitSyncModal={closeGitSyncModal}
          setCredentialsSaved={setCredentialsSaved}
          credentialsSaved={credentialsSaved}
          setPollingInterval={setPollingInterval}
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
          cancelRef={cancelRef}
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
