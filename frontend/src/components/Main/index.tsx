import React, { useRef, useState } from 'react'

import { Box, useColorModeValue, VStack } from '@chakra-ui/react'
import { open, save } from '@tauri-apps/api/dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/api/fs'
import { invoke } from '@tauri-apps/api/tauri'

import useConfigStore from '../../store'
import { Config, Response, Status } from '../../types'
import AddConfigModal from '../AddConfigModal'
import useCustomToast from '../CustomToast'
import Footer from '../Footer'
import GitSyncModal from '../GitSyncModal'
import PortForwardTable from '../PortForwardTable'

const KFTray = () => {
  const {
    configs,
    setConfigs,
    isInitiating,
    setIsInitiating,
    isStopping,
    setIsStopping,
    isPortForwarding,
    setIsPortForwarding,
    syncConfigsAndUpdateState,
    updateConfigRunningState,
  } = useConfigStore()

  const [isModalOpen, setIsModalOpen] = useState(false)
  const [isGitSyncModalOpen, setIsGitSyncModalOpen] = useState(false)
  const [newConfig, setNewConfig] = useState<Config>({
    id: 0,
    service: '',
    context: '',
    local_port: 0,
    local_address: '127.0.0.1',
    domain_enabled: false,
    remote_port: 0,
    namespace: '',
    workload_type: '',
    protocol: '',
    remote_address: '',
    alias: '',
    kubeconfig: 'default',
  })
  const [isEdit, setIsEdit] = useState(false)
  const toast = useCustomToast()
  const cardBg = useColorModeValue('gray.800', 'gray.800')
  const cancelRef = useRef<HTMLElement>(null)
  const [isAlertOpen, setIsAlertOpen] = useState(false)
  const [configToDelete, setConfigToDelete] = useState<number | undefined>()
  const [selectedConfigs, setSelectedConfigs] = useState<Status[]>([])
  const [credentialsSaved, setCredentialsSaved] = useState(false)
  const [pollingInterval, setPollingInterval] = useState(0)

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target
    let updatedValue: string | number = value

    if (name === 'local_port' || name === 'remote_port') {
      updatedValue = value === '' ? '' : Number(value)
    }

    setNewConfig((prev: Config) => ({
      ...prev,
      [name]: updatedValue,
    }))
  }

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
        filters: [{ name: 'JSON', extensions: ['json'] }],
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
      await invoke('update_config', { config: newConfig })
      const updatedConfigs = await invoke<Status[]>('get_configs')

      setConfigs(updatedConfigs)
      toast({
        title: 'Success',
        description: 'Configuration updated successfully.',
        status: 'success',
      })
      setIsModalOpen(false)
    } catch (error) {
      toast({
        title: 'Error',
        description: 'Failed to update configuration.',
        status: 'error',
      })
    }
  }

  const handleSaveConfig = async (_configToSave: Config) => {
    try {
      if (isEdit) {
        await invoke('update_config', { config: newConfig })
      } else {
        await invoke('insert_config', { config: newConfig })
      }
      const updatedConfigs = await invoke<Status[]>('get_configs')

      setConfigs(updatedConfigs)
      toast({
        title: 'Success',
        description: `Configuration ${isEdit ? 'updated' : 'added'} successfully.`,
        status: 'success',
      })
      setIsModalOpen(false)
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

  const handlePortForwarding = async (config: Status) => {
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

      setConfigs(configsAfterDeletion)
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
      const allStopped = responses.every(res => res.status === 0)

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
        .filter(res => res.status !== 0)
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
      position='fixed'
      width='100%'
      height='100%'
      maxHeight='100%'
      maxW='100%'
      overflow='hidden'
      borderRadius='20px'
      bg={cardBg}
      boxShadow={`
        inset 0 2px 4px rgba(0, 0, 0, 0.3),
        inset 0 -2px 4px rgba(0, 0, 0, 0.3),
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
        height='100%'
        maxH='100%'
        w='100%'
        maxW='100%'
        overflow='hidden'
        padding='15px'
        position='fixed'
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
          closeGitSyncModal={() => setIsGitSyncModalOpen(false)}
          onSettingsSaved={syncConfigsAndUpdateState}
          setCredentialsSaved={setCredentialsSaved}
          credentialsSaved={credentialsSaved}
          setPollingInterval={setPollingInterval}
          pollingInterval={pollingInterval}
        />
        <AddConfigModal
          isModalOpen={isModalOpen}
          closeModal={() => setIsModalOpen(false)}
          newConfig={newConfig}
          handleInputChange={handleInputChange}
          handleSaveConfig={handleSaveConfig}
          isEdit={isEdit}
          handleEditSubmit={handleEditSubmit}
          cancelRef={cancelRef}
          setNewConfig={setNewConfig}
        />
        <Footer
          openModal={() => setIsModalOpen(true)}
          openGitSyncModal={() => setIsGitSyncModalOpen(true)}
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
      </VStack>
    </Box>
  )
}

export default KFTray
