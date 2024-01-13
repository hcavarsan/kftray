import React, { useEffect, useState } from 'react'

import { Box, Center, useColorModeValue, VStack } from '@chakra-ui/react'
import { open, save } from '@tauri-apps/api/dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/api/fs'
import { sendNotification } from '@tauri-apps/api/notification'
import { invoke } from '@tauri-apps/api/tauri'

import { Config, Response, Status } from '../../types'
import AddConfigModal from '../AddConfigModal'
import PortForwardTable from '../PortForwardTable'

const initalRemotePort = 0
const initialLocalPort = 0
const initialId = 0
const initialStatus = 0
const KFTray = () => {
  const [configs, setConfigs] = useState<Status[]>([])
  const [isModalOpen, setIsModalOpen] = useState(false)

  const [isEdit, setIsEdit] = useState(false)

  const [newConfig, setNewConfig] = useState<Config>({
    id: 0,
    service: '',
    context: '',
    local_port: 0,
    remote_port: 0,
    namespace: '',
    workload_type: '',
    protocol: '',
    remote_address: '',
    alias: '',
  })

  const updateConfigRunningState = (id: number, isRunning: boolean) => {
    setConfigs(currentConfigs =>
      currentConfigs.map(config => {
        return config.id === id ? { ...config, isRunning } : config
      }),
    )
  }

  const openModal = () => {
    setNewConfig({
      id: initialId,
      service: '',
      context: '',
      local_port: initialLocalPort,
      remote_port: initalRemotePort,
      namespace: '',
      workload_type: '',
      protocol: '',
      remote_address: '',
      alias: '',
    })
    setIsEdit(false)
    setIsModalOpen(true)
  }
  const closeModal = () => {
    setIsModalOpen(false)
    setIsEdit(false)
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
    const fetchConfigs = async () => {
      try {
        const configsResponse = await invoke<Status[]>('get_configs')

        setConfigs(
          configsResponse.map(config => ({
            ...config,
            isRunning: false,
          })),
        )
      } catch (error) {
        console.error('Failed to fetch configs:', error)
      }
    }

    fetchConfigs()
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
        await sendNotification({
          title: 'Success',
          body: 'Configuration exported successfully.',
          icon: 'success',
        })
      }
    } catch (error) {
      console.error('Failed to export configs:', error)
      await sendNotification({
        title: 'Error',
        body: 'Failed to export configs.',
        icon: 'error',
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

        await sendNotification({
          title: 'Success',
          body: 'Configurations imported successfully.',
          icon: 'success',
        })
      } else {
        console.log('No file was selected or the dialog was cancelled.')
      }
    } catch (error) {
      console.error('Error during import:', error)
      await sendNotification({
        title: 'Error',
        body: 'Failed to import configurations.',
        icon: 'error',
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
        remote_port: configToEdit.remote_port,
        context: configToEdit.context,
        workload_type: configToEdit.workload_type,
        protocol: configToEdit.protocol,
        remote_address: configToEdit.remote_address,
        alias: configToEdit.alias,
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
        remote_port: newConfig.remote_port,
        namespace: newConfig.namespace,
        workload_type: newConfig.workload_type,
        protocol: newConfig.protocol,
        remote_address: newConfig.remote_address,
        alias: newConfig.alias,
      }

      await invoke('update_config', { config: editedConfig })

      const updatedConfigs = await invoke<Status[]>('get_configs')

      setConfigs(updatedConfigs)

      await sendNotification({
        title: 'Success',
        body: 'Configuration updated successfully.',
        icon: 'success',
      })

      closeModal()
    } catch (error) {
      console.error('Failed to update config:', error)
      await sendNotification({
        title: 'Error',
        body: 'Failed to update configuration.',
        icon: 'error',
      })
    }
  }
  const handleSaveConfig = async (e: React.FormEvent) => {
    e.preventDefault()

    const configToSave = {
      id: isEdit ? newConfig.id : undefined,
      service: newConfig.service,
      context: newConfig.context,
      local_port: newConfig.local_port,
      remote_port: newConfig.remote_port,
      namespace: newConfig.namespace,
      workload_type: newConfig.workload_type,
      protocol: newConfig.protocol,
      remote_address: newConfig.remote_address,
      alias: newConfig.alias,
    }

    try {
      if (isEdit) {
        // Update existing config
        await invoke('update_config', { config: configToSave })
      } else {
        // Insert new config
        await invoke('insert_config', { config: configToSave })
      }

      const updatedConfigs = await invoke<Status[]>('get_configs')

      setConfigs(updatedConfigs)

      await sendNotification({
        title: 'Success',
        body: `Configuration ${isEdit ? 'updated' : 'added'} successfully.`,
        icon: 'success',
      })

      closeModal()
    } catch (error) {
      console.error(`Failed to ${isEdit ? 'update' : 'insert'} config:`, error)

      await sendNotification({
        title: 'Error',
        body: `Failed to ${
          isEdit ? 'update' : 'add'
        } configuration. Error: ${error}`,
        icon: 'error',
      })
    }
  }
  const initiatePortForwarding = async (configsToStart: Status[]) => {
    setIsInitiating(true)
    const errors = []

    console.log('Starting port forwarding for configs:', configsToStart)
    for (const config of configsToStart) {
      try {
        // Determine action based on the workload_type
        let response

        if (config.workload_type === 'service') {
          // Existing logic for initiating port forwarding with 'service' workload
          response = await invoke<Response>('start_port_forward', {
            configs: [config],
          })
        } else if (config.workload_type.startsWith('proxy')) {
          // Logic for initiating port forwarding with 'proxy' workload (new function invoked)
          response = await invoke<Response>('deploy_and_forward_pod', {
            configs: [config],
          })
        } else {
          throw new Error(`Unsupported workload type: ${config.workload_type}`)
        }
        updateConfigRunningState(config.id, true)
        console.log(
          'Port forwarding initiated for config:',
          config.id,
          response,
        )
      } catch (error) {
        console.error(
          `Error starting port forward for config id ${config.id}:`,
          error,
        )
        errors.push({ id: config.id, error })
        updateConfigRunningState(config.id, false)
      }
    }

    if (errors.length > 0) {
      const errorMessage = errors
      .map(e => `Config ID: ${e.id}, Error: ${e.error}`)
      .join(', ')

      await sendNotification({
        title: 'Error Starting Port Forwarding',
        body: `Some configs failed: ${errorMessage}`,
        icon: 'error',
      })
    }

    setIsInitiating(false)
  }

  const handleDeleteConfig = (id: number) => {
    setConfigToDelete(id)
    setIsAlertOpen(true)
  }

  const confirmDeleteConfig = async () => {
    if (typeof configToDelete !== 'number') {
      await sendNotification({
        title: 'Error',
        body: 'Configuration id is undefined.',
        icon: 'error',
      })

      return
    }

    try {
      await invoke('delete_config', { id: configToDelete })
      const updatedConfigs = await invoke<Status[]>('get_configs')

      setConfigs(updatedConfigs)

      await sendNotification({
        title: 'Success',
        body: 'Configuration deleted successfully.',
        icon: 'success',
      })
    } catch (error) {
      console.error('Failed to delete configuration:', error)
      await sendNotification({
        title: 'Error',
        body: 'Failed to delete configuration:", "unknown error"',
        icon: 'error',
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
        await sendNotification({
          title: 'Success',
          body: 'Port forwarding stopped successfully for all configurations.',
          icon: 'success',
        })
      } else {
        const errorMessages = responses
        .filter(res => res.status !== initialStatus)
        .map(res => `${res.service}: ${res.stderr}`)
        .join(', ')

        await sendNotification({
          title: 'Error',
          body: `Port forwarding failed for some configurations: ${errorMessages}`,
          icon: 'error',
        })
      }
    } catch (error) {
      console.error('An error occurred while stopping port forwarding:', error)
      await sendNotification({
        title: 'Error',
        body: `An error occurred while stopping port forwarding: ${error}`,
        icon: 'error',
      })
    }
    setIsStopping(false)
  }

  const cardBg = useColorModeValue('gray.800', 'gray.800')

  return (
    <Center h='100%' w='100%' overflow='hidden' margin='0'>
      <Box
        width='100%'
        height='75vh'
        maxH='95vh'
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
          h='100%'
          w='100%'
          maxW='100%'
          overflowY='auto'
          padding='15px'
          mt='2px'
        >
          <AddConfigModal
            isModalOpen={isModalOpen}
            closeModal={closeModal}
            newConfig={newConfig}
            handleInputChange={handleInputChange}
            handleSaveConfig={handleSaveConfig}
            isEdit={isEdit}
            handleEditSubmit={handleEditSubmit}
            cancelRef={cancelRef}
          />
          <PortForwardTable
            configs={configs}
            initiatePortForwarding={initiatePortForwarding}
            isInitiating={isInitiating}
            isStopping={isStopping}
            handleEditConfig={handleEditConfig}
            stopPortForwarding={stopPortForwarding}
            handleDeleteConfig={handleDeleteConfig}
            confirmDeleteConfig={confirmDeleteConfig}
            isAlertOpen={isAlertOpen}
            setIsAlertOpen={setIsAlertOpen}
            updateConfigRunningState={updateConfigRunningState}
            openModal={openModal}
            handleExportConfigs={handleExportConfigs}
            handleImportConfigs={handleImportConfigs}
            isPortForwarding={isPortForwarding}
          />
        </VStack>
      </Box>
    </Center>
  )
}

export default KFTray
