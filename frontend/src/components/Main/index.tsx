import React, { useEffect, useState } from 'react'

import { Box, useColorModeValue, VStack } from '@chakra-ui/react'
import { open, save } from '@tauri-apps/api/dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/api/fs'
import { invoke } from '@tauri-apps/api/tauri'

import { Config, Response, Status } from '../../types'
import AddConfigModal from '../AddConfigModal'
import useCustomToast from '../CustomToast'
import Footer from '../Footer'
import GitSyncModal from '../GitSyncModal'
import PortForwardTable from '../PortForwardTable'

const initalRemotePort = 0
const initialLocalPort = 0
const initialId = 0
const initialStatus = 0
const KFTray = () => {
  const toast = useCustomToast()
  const [pollingInterval, setPollingInterval] = useState(0)
  const [configs, setConfigs] = useState<Status[]>([])
  const [isModalOpen, setIsModalOpen] = useState(false)
  const [isGitSyncModalOpen, setIsGitSyncModalOpen] = useState(false)
  const [selectedConfigs, setSelectedConfigs] = useState<Status[]>([])
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
    protocol: '',
    remote_address: '',
    alias: '',
    kubeconfig: 'default',
  })

  const updateConfigRunningState = (id: number, isRunning: boolean) => {
    setConfigs(prevConfigs =>
      prevConfigs.map(config =>
        config.id === id ? { ...config, isRunning } : config,
      ),
    )

    if (isRunning) {
      setSelectedConfigs(prevSelectedConfigs =>
        prevSelectedConfigs.filter(config => config.id !== id),
      )
    }
  }
  const syncConfigsAndUpdateState = async () => {
    try {
      const updatedConfigs = await invoke<Status[]>('get_configs')

      if (!updatedConfigs) {
        return
      }

      setConfigs(updatedConfigs)
    } catch (error) {
      console.error('Error syncing configs:', error)
    }
  }

  const openModal = () => {
    setNewConfig({
      id: initialId,
      service: '',
      context: '',
      local_port: initialLocalPort,
      local_address: '127.0.0.1',
      domain_enabled: false,
      remote_port: initalRemotePort,
      namespace: '',
      workload_type: '',
      protocol: '',
      remote_address: '',
      alias: '',
      kubeconfig: 'default',
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

    let updatedValue: string | number

    if (name === 'local_port' || name === 'remote_port') {
      updatedValue = value === '' ? '' : Number(value)
    } else {
      updatedValue = value
    }

    setNewConfig(prev => ({
      ...prev,
      [name]: updatedValue,
    }))
  }
  const cancelRef = React.useRef<HTMLElement>(null)
  const [isInitiating, setIsInitiating] = useState(false)
  const [isStopping, setIsStopping] = useState(false)
  const [isPortForwarding, setIsPortForwarding] = useState(false)
  const [isAlertOpen, setIsAlertOpen] = useState(false)
  const [configToDelete, setConfigToDelete] = useState<number | undefined>()

  useEffect(() => {
    let isMounted = true

    const fetchConfigs = async () => {
      try {
        const configsResponse = await invoke<Status[]>('get_configs')

        if (isMounted) {
          setConfigs(
            configsResponse.map(config => ({
              ...config,
              isRunning: false,
            })),
          )
        }
      } catch (error) {
        console.error('Failed to fetch configs:', error)
      }
    }

    fetchConfigs()

    return () => {
      isMounted = false
    }
  }, [])

  const handleExportConfigs = async () => {
    try {
      await invoke('open_save_dialog')

      const json = await invoke('export_configs')

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
        filters: [
          {
            name: 'JSON',
            extensions: ['json'],
          },
        ],
        multiple: false,
      })

      await invoke('close_save_dialog')
      if (typeof selected === 'string') {
        const jsonContent = await readTextFile(selected)

        await invoke('import_configs', { json: jsonContent })
        const updatedConfigs = await invoke<Status[]>('get_configs')

        setConfigs(updatedConfigs)
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
      const configToEdit = await invoke<Config>('get_config', { id })

      setNewConfig({
        id: configToEdit.id,
        service: configToEdit.service,
        namespace: configToEdit.namespace,
        local_port: configToEdit.local_port,
        local_address: configToEdit.local_address,
        domain_enabled: configToEdit.domain_enabled,
        remote_port: configToEdit.remote_port,
        context: configToEdit.context,
        workload_type: configToEdit.workload_type,
        protocol: configToEdit.protocol,
        remote_address: configToEdit.remote_address,
        alias: configToEdit.alias,
        kubeconfig: configToEdit.kubeconfig,
      })
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
      const editedConfig = {
        id: newConfig.id,
        service: newConfig.service,
        context: newConfig.context,
        local_port: newConfig.local_port,
        local_address: newConfig.local_address,
        domain_enabled: newConfig.domain_enabled,
        remote_port: newConfig.remote_port,
        namespace: newConfig.namespace,
        workload_type: newConfig.workload_type,
        protocol: newConfig.protocol,
        remote_address: newConfig.remote_address,
        alias: newConfig.alias,
        kubeconfig: newConfig.kubeconfig,
      }

      await invoke('update_config', { config: editedConfig })

      const updatedConfigs = await invoke<Status[]>('get_configs')

      setConfigs(updatedConfigs)
      toast({
        title: 'Success',
        description: 'Configuration updated successfully.',
        status: 'success',
      })

      closeModal()
    } catch (error) {
      toast({
        title: 'Error',
        description: 'Failed to update configuration.',
        status: 'error',
      })
    }
  }

  const fetchAndUpdateConfigs = async () => {
    try {
      const updatedConfigs = await invoke<Status[]>('get_configs')

      setConfigs(updatedConfigs)
    } catch (error) {
      console.error('Failed to fetch updated configs:', error)
    }
  }
  // eslint-disable-next-line complexity
  const handleSaveConfig = async (_configToSave: Config) => {
    const updatedConfigToSave: Config = {
      id: isEdit ? newConfig.id : 0,
      service: newConfig.service,
      context: newConfig.context,
      local_port: newConfig.local_port,
      local_address: newConfig.local_address,
      domain_enabled: newConfig.domain_enabled,
      remote_port: newConfig.remote_port,
      namespace: newConfig.namespace,
      workload_type: newConfig.workload_type,
      protocol: newConfig.protocol,
      remote_address: newConfig.remote_address,
      alias: newConfig.alias,
      kubeconfig: newConfig.kubeconfig,
    }

    console.log('Sending config to save:', updatedConfigToSave)
    try {
      let wasRunning = false
      const originalConfigsRunningState = new Map()

      configs.forEach(conf =>
        originalConfigsRunningState.set(conf.id, conf.isRunning),
      )

      if (isEdit && originalConfigsRunningState.get(newConfig.id)) {
        wasRunning = true

        if (
          newConfig.workload_type === 'service' &&
          newConfig.protocol === 'tcp'
        ) {
          await invoke('stop_port_forward', {
            serviceName: newConfig.service,
            configId: newConfig.id.toString(),
          })
        } else if (
          newConfig.workload_type.startsWith('proxy') ||
          (newConfig.workload_type === 'service' &&
            newConfig.protocol === 'udp')
        ) {
          await invoke('stop_proxy_forward', {
            configId: newConfig.id.toString(),
            namespace: newConfig.namespace,
            serviceName: newConfig.service,
            localPort: newConfig.local_port,
            remoteAddress: newConfig.remote_address,
            protocol: 'tcp',
          })
        } else {
          throw new Error(
            `Unsupported workload type: ${newConfig.workload_type}`,
          )
        }
      }

      if (isEdit) {
        await invoke('update_config', { config: updatedConfigToSave })
      } else {
        await invoke('insert_config', { config: updatedConfigToSave })
      }

      let updatedConfigs = await invoke<Status[]>('get_configs')

      updatedConfigs = updatedConfigs.map(conf => ({
        ...conf,
        isRunning:
          conf.id === newConfig.id
            ? wasRunning
            : originalConfigsRunningState.get(conf.id) || false,
      }))

      if (wasRunning) {
        const updatedConfig = updatedConfigs.find(
          conf => conf.id === newConfig.id,
        )

        if (updatedConfig) {
          if (
            updatedConfig.workload_type === 'service' &&
            updatedConfig.protocol === 'tcp'
          ) {
            await invoke('start_port_forward', { configs: [updatedConfig] })
          } else if (
            updatedConfig.workload_type.startsWith('proxy') ||
            (updatedConfig.workload_type === 'service' &&
              updatedConfig.protocol === 'udp')
          ) {
            await invoke('deploy_and_forward_pod', { configs: [updatedConfig] })
          } else {
            throw new Error(
              `Unsupported workload type: ${updatedConfig.workload_type}`,
            )
          }
          updatedConfigs = updatedConfigs.map(conf =>
            conf.id === updatedConfig.id ? { ...conf, isRunning: true } : conf,
          )
        }
      }

      setConfigs(updatedConfigs)
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

  const initiatePortForwarding = async (configsToStart: Status[]) => {
    setIsInitiating(true)
    const errors = []

    for (const config of configsToStart) {
      try {
        await handlePortForwarding(config)
        updateConfigRunningState(config.id, true)
      } catch (error) {
        errors.push({ id: config.id, error })
        updateConfigRunningState(config.id, false)
      }
    }

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

  async function handlePortForwarding(config: Status) {
    switch (config.workload_type) {
    case 'service':
      if (config.protocol === 'tcp') {
        await invoke<Response>('start_port_forward', { configs: [config] })
      } else if (config.protocol === 'udp') {
        await invoke<Response>('deploy_and_forward_pod', {
          configs: [config],
        })
      }
      break
    case 'proxy':
      await invoke<Response>('deploy_and_forward_pod', { configs: [config] })
      break
    default:
      throw new Error(`Unsupported workload type: ${config.workload_type}`)
    }
  }
  const handleDeleteConfig = (id: number) => {
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
      await invoke('delete_config', { id: configToDelete })

      const configsAfterDeletion = await invoke<Status[]>('get_configs')
      const runningStateMap = new Map(
        configs.map(conf => [conf.id, conf.isRunning]),
      )

      const updatedConfigs = configsAfterDeletion.map(conf => ({
        ...conf,
        isRunning: runningStateMap.get(conf.id) ?? false,
      }))

      setConfigs(updatedConfigs)

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

  const stopPortForwarding = async () => {
    setIsStopping(true)
    try {
      const responses = await invoke<Response[]>('stop_all_port_forward')

      const allStopped = responses.every(res => res.status === initialStatus)

      if (allStopped) {
        const updatedConfigs = configs.map(config => ({
          ...config,
          isRunning: false,
        }))

        setConfigs(updatedConfigs)
        setIsPortForwarding(false)
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

  useEffect(() => {
    if (isModalOpen) {
      document.body.style.overflow = 'hidden'
    } else {
      document.body.style.overflow = 'unset'
    }
  }, [isModalOpen])
  const cardBg = useColorModeValue('gray.800', 'gray.800')

  return (
    <Box
      position='relative'
      width='100%'
      height='76vh'
      maxW='600px'
      overflow='hidden'
      borderRadius='20px'
      bg={cardBg}
      boxShadow={`
        /* Inset shadow for top & bottom inner border effect using dark gray */
        inset 0 2px 4px rgba(0, 0, 0, 0.3),
        inset 0 -2px 4px rgba(0, 0, 0, 0.3),
        /* Inset shadow for an inner border all around using dark gray */
        inset 0 0 0 4px rgba(45, 57, 81, 0.9)
      `}
    >
      <VStack
        css={{
          '&::-webkit-scrollbar': {
            width: '5px',
            background: 'transparent',
          },
          '&::-webkit-scrollbar-thumb': {
            background: '#555',
          },
          '&::-webkit-scrollbar-thumb:hover': {
            background: '#666',
          },
        }}
        height='78vh'
        w='100%'
        maxW='100%'
        overflowY='auto'
        padding='15px'
        mt='2px'
      >
        <PortForwardTable
          configs={configs}
          initiatePortForwarding={initiatePortForwarding}
          isInitiating={isInitiating}
          setIsInitiating={setIsInitiating}
          isStopping={isStopping}
          handleEditConfig={handleEditConfig}
          stopPortForwarding={stopPortForwarding}
          handleDeleteConfig={handleDeleteConfig}
          confirmDeleteConfig={confirmDeleteConfig}
          isAlertOpen={isAlertOpen}
          setIsAlertOpen={setIsAlertOpen}
          updateConfigRunningState={updateConfigRunningState}
          isPortForwarding={isPortForwarding}
          selectedConfigs={selectedConfigs}
          setSelectedConfigs={setSelectedConfigs}
        />

        <GitSyncModal
          isGitSyncModalOpen={isGitSyncModalOpen}
          closeGitSyncModal={closeGitSyncModal}
          onSettingsSaved={fetchAndUpdateConfigs}
          setCredentialsSaved={setCredentialsSaved}
          credentialsSaved={credentialsSaved}
          setPollingInterval={setPollingInterval}
          pollingInterval={pollingInterval}
        />
        <Footer
          openModal={openModal}
          openGitSyncModal={openGitSyncModal}
          handleExportConfigs={handleExportConfigs}
          handleImportConfigs={handleImportConfigs}
          onConfigsSynced={syncConfigsAndUpdateState}
          setCredentialsSaved={setCredentialsSaved}
          credentialsSaved={credentialsSaved}
          isGitSyncModalOpen={isGitSyncModalOpen}
          selectedConfigs={selectedConfigs}
          setPollingInterval={setPollingInterval}
          pollingInterval={pollingInterval}
          setSelectedConfigs={setSelectedConfigs}
          configs={configs}
          setConfigs={setConfigs}
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
      </VStack>
    </Box>
  )
}

export default KFTray
